package dev.hoangmirs.purchase

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.android.billingclient.api.AcknowledgePurchaseParams
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClient.BillingResponseCode
import com.android.billingclient.api.BillingClient.ProductType
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingFlowParams
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.ConsumeParams
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams

@InvokeArg
class IdsArgs {
  var ids: List<String> = emptyList()
}

@InvokeArg
class BuyArgs {
  var id: String = ""
  var consumable: Boolean = false
}

@InvokeArg
class FinishArgs {
  var tx: String = ""
}

@TauriPlugin
class PurchasePlugin(private val activity: Activity) : Plugin(activity) {
  // Play answers a purchase sheet through the client's one listener, not
  // through the call that opened it, so the open sheet's invoke waits here.
  private var buying: Invoke? = null
  private var buyingId = ""

  // Ids the app buys as consumables. Such a purchase is left open for `finish`
  // to consume, because consuming it before the app claims it loses the pack.
  private val consumables: MutableSet<String> = java.util.concurrent.ConcurrentHashMap.newKeySet()

  private fun isConsumable(p: Purchase) = p.products.any { it in consumables }

  private val client: BillingClient = BillingClient.newBuilder(activity)
    .setListener { result, purchases -> onPurchases(result, purchases) }
    .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
    .build()

  private val waiting = mutableListOf<(BillingResult) -> Unit>()

  // One connection serves every call. Calls made while it opens wait for it,
  // and a dropped connection opens again on the next call.
  private fun ready(then: (BillingResult) -> Unit) {
    if (client.isReady) {
      then(BillingResult.newBuilder().setResponseCode(BillingResponseCode.OK).build())
      return
    }
    synchronized(waiting) {
      waiting.add(then)
      if (waiting.size > 1) return
    }
    client.startConnection(object : BillingClientStateListener {
      override fun onBillingSetupFinished(result: BillingResult) {
        val all = synchronized(waiting) { waiting.toList().also { waiting.clear() } }
        all.forEach { it(result) }
      }

      override fun onBillingServiceDisconnected() {}
    })
  }

  private fun details(ids: List<String>, then: (BillingResult, List<ProductDetails>) -> Unit) {
    val products = ids.map {
      QueryProductDetailsParams.Product.newBuilder().setProductId(it).setProductType(ProductType.INAPP).build()
    }
    val params = QueryProductDetailsParams.newBuilder().setProductList(products).build()
    client.queryProductDetailsAsync(params) { result, found -> then(result, found.productDetailsList) }
  }

  private fun failed(invoke: Invoke, result: BillingResult) =
    invoke.reject(result.debugMessage.ifEmpty { "Play answered ${result.responseCode}" })

  private fun state(name: String, proof: String? = null, tx: String? = null) = JSObject().apply {
    put("state", name)
    if (proof != null) put("proof", proof)
    if (tx != null) put("tx", tx)
  }

  private fun bought(p: Purchase) =
    if (isConsumable(p)) state("bought", p.purchaseToken, p.purchaseToken) else state("bought", p.purchaseToken)

  @Command
  fun products(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    ready { ok ->
      if (ok.responseCode != BillingResponseCode.OK) return@ready failed(invoke, ok)
      details(args.ids) { result, found ->
        if (result.responseCode != BillingResponseCode.OK) return@details failed(invoke, result)
        val out = JSArray()
        for (d in found) {
          val offer = d.oneTimePurchaseOfferDetails ?: continue
          out.put(JSObject().apply {
            put("id", d.productId)
            put("price", offer.formattedPrice)
          })
        }
        invoke.resolve(JSObject().put("products", out))
      }
    }
  }

  @Command
  fun buy(invoke: Invoke) {
    val args = invoke.parseArgs(BuyArgs::class.java)
    if (args.consumable) consumables.add(args.id) else consumables.remove(args.id)
    // Play's sheet covers the app while it is open, so a second buy means Play
    // never answered the first. Refusing it would block buying until a restart.
    buying?.let {
      buying = null
      it.resolve(state("failed"))
    }
    ready { ok ->
      if (ok.responseCode != BillingResponseCode.OK) return@ready invoke.resolve(state("failed"))
      details(listOf(args.id)) { result, found ->
        val product = found.firstOrNull()
        if (result.responseCode != BillingResponseCode.OK || product == null) {
          return@details invoke.resolve(state("failed"))
        }
        val params = BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(product)
        // setOfferToken throws on an empty token, and a product with one
        // offer may give none.
        product.oneTimePurchaseOfferDetails?.offerToken?.takeIf { it.isNotEmpty() }?.let { params.setOfferToken(it) }
        val flow = BillingFlowParams.newBuilder().setProductDetailsParamsList(listOf(params.build())).build()
        activity.runOnUiThread {
          buying = invoke
          buyingId = args.id
          val launched = client.launchBillingFlow(activity, flow)
          if (launched.responseCode != BillingResponseCode.OK) {
            buying = null
            invoke.resolve(state(if (launched.responseCode == BillingResponseCode.USER_CANCELED) "cancelled" else "failed"))
          }
        }
      }
    }
  }

  private fun onPurchases(result: BillingResult, purchases: List<Purchase>?) {
    // Purchases also arrive with no sheet open, for example a pending payment
    // that has now gone through. A non-consumable is acknowledged all the same.
    // A consumable answers the open sheet when it is the one just bought, and
    // otherwise reaches the app as `arrived`.
    val sheetId = buyingId
    val sheetOpen = buying != null && result.responseCode == BillingResponseCode.OK
    purchases?.forEach { p ->
      if (!isConsumable(p)) return@forEach acknowledge(p)
      val answersSheet = sheetOpen && sheetId in p.products
      if (p.purchaseState == Purchase.PurchaseState.PURCHASED && !answersSheet) {
        p.products.filter { it in consumables }.forEach { id ->
          trigger("arrived", JSObject().put("id", id).put("tx", p.purchaseToken))
        }
      }
    }
    val invoke = buying ?: return
    val id = buyingId
    buying = null
    when (result.responseCode) {
      BillingResponseCode.OK -> {
        val p = purchases?.firstOrNull()
        invoke.resolve(
          when (p?.purchaseState) {
            Purchase.PurchaseState.PURCHASED -> bought(p)
            Purchase.PurchaseState.PENDING -> state("pending")
            else -> state("failed")
          },
        )
      }
      BillingResponseCode.USER_CANCELED -> invoke.resolve(state("cancelled"))
      // Bought before, on this account: answer with that purchase's token.
      BillingResponseCode.ITEM_ALREADY_OWNED -> held({ invoke.resolve(state("failed")) }) { list ->
        val p = list.firstOrNull { id in it.products }
        invoke.resolve(if (p != null) bought(p) else state("failed"))
      }
      else -> invoke.resolve(state("failed"))
    }
  }

  // Play refunds a purchase that is not acknowledged within three days.
  private fun acknowledge(p: Purchase) {
    if (p.purchaseState != Purchase.PurchaseState.PURCHASED || p.isAcknowledged) return
    val params = AcknowledgePurchaseParams.newBuilder().setPurchaseToken(p.purchaseToken).build()
    client.acknowledgePurchase(params) {}
  }

  // queryPurchasesAsync leaves out refunded purchases, and answers from Play's
  // copy on the phone when it is offline. An error goes to fail, never to then
  // as an empty list: an app reads empty as "owns nothing" and locks what the
  // player paid for.
  private fun held(fail: (BillingResult) -> Unit, then: (List<Purchase>) -> Unit) {
    val params = QueryPurchasesParams.newBuilder().setProductType(ProductType.INAPP).build()
    client.queryPurchasesAsync(params) { result, list ->
      if (result.responseCode != BillingResponseCode.OK) return@queryPurchasesAsync fail(result)
      then(list.filter { it.purchaseState == Purchase.PurchaseState.PURCHASED })
    }
  }

  @Command
  fun owned(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    ready { ok ->
      if (ok.responseCode != BillingResponseCode.OK) return@ready failed(invoke, ok)
      held({ failed(invoke, it) }) { list ->
        val out = JSArray()
        for (p in list) {
          if (!isConsumable(p)) acknowledge(p)
          for (id in p.products) {
            if (id in args.ids) {
              out.put(JSObject().apply {
                put("id", id)
                put("proof", p.purchaseToken)
              })
            }
          }
        }
        invoke.resolve(JSObject().put("owned", out))
      }
    }
  }

  // The tx is the purchase token. A purchase that is already consumed, or that
  // Play no longer knows, counts as finished, so a second call does not fail
  // a claim that went through.
  @Command
  fun finish(invoke: Invoke) {
    val args = invoke.parseArgs(FinishArgs::class.java)
    ready { ok ->
      if (ok.responseCode != BillingResponseCode.OK) return@ready failed(invoke, ok)
      val params = ConsumeParams.newBuilder().setPurchaseToken(args.tx).build()
      client.consumeAsync(params) { result, _ ->
        when (result.responseCode) {
          BillingResponseCode.OK, BillingResponseCode.ITEM_NOT_OWNED -> invoke.resolve(JSObject())
          else -> failed(invoke, result)
        }
      }
    }
  }

  @Command
  fun unfinished(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    consumables.addAll(args.ids)
    ready { ok ->
      if (ok.responseCode != BillingResponseCode.OK) return@ready failed(invoke, ok)
      held({ failed(invoke, it) }) { list ->
        val out = JSArray()
        for (p in list) {
          for (id in p.products) {
            if (id in args.ids) {
              out.put(JSObject().apply {
                put("id", id)
                put("tx", p.purchaseToken)
              })
            }
          }
        }
        invoke.resolve(JSObject().put("open", out))
      }
    }
  }

  // Play keeps an account's purchases on every phone it signs in to, so
  // restoring is reading them again.
  @Command
  fun restore(invoke: Invoke) = owned(invoke)
}
