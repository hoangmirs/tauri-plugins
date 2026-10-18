package dev.hoangmirs.ads

import android.app.Activity
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.ads.AdError
import com.google.android.gms.ads.AdRequest
import com.google.android.gms.ads.FullScreenContentCallback
import com.google.android.gms.ads.LoadAdError
import com.google.android.gms.ads.MobileAds
import com.google.android.gms.ads.RequestConfiguration
import com.google.android.gms.ads.rewarded.RewardedAd
import com.google.android.gms.ads.rewarded.RewardedAdLoadCallback
import com.google.android.ump.ConsentInformation
import com.google.android.ump.ConsentRequestParameters
import com.google.android.ump.UserMessagingPlatform
import kotlin.concurrent.thread

@InvokeArg
class SettingsArgs {
  var childDirected: Boolean = true
  var maxRating: String = "G"
}

@InvokeArg
class UnitArgs {
  var unitId: String = ""
}

private fun result(text: String) = JSObject().put("result", text)

@TauriPlugin
class AdsPlugin(private val activity: Activity) : Plugin(activity) {
  /** One rewarded ad, loaded before the player asks for it. */
  private var rewarded: RewardedAd? = null
  /** The `showRewarded` call waiting for the ad to close. */
  private var showing: Invoke? = null
  private var earned = false
  private val native by lazy {
    NativeCard(activity) { event -> trigger("native", JSObject().put("event", event)) }
  }

  @Command
  fun showNative(invoke: Invoke) {
    val slot = invoke.parseArgs(SlotArgs::class.java)
    activity.runOnUiThread {
      native.show(slot)
      invoke.resolve()
    }
  }

  @Command
  fun hideNative(invoke: Invoke) {
    activity.runOnUiThread {
      native.hide()
      invoke.resolve()
    }
  }

  @Command
  fun init(invoke: Invoke) {
    val args = invoke.parseArgs(SettingsArgs::class.java)
    val config = RequestConfiguration.Builder()
      .setTagForChildDirectedTreatment(
        if (args.childDirected) RequestConfiguration.TAG_FOR_CHILD_DIRECTED_TREATMENT_TRUE
        else RequestConfiguration.TAG_FOR_CHILD_DIRECTED_TREATMENT_FALSE
      )
      .setMaxAdContentRating(rating(args.maxRating))
      .build()
    MobileAds.setRequestConfiguration(config)
    // Google asks for the start off the main thread: it reads from disk.
    thread { MobileAds.initialize(activity) { Log.i("ads", "[ads] started") } }
    invoke.resolve()
  }

  /** Google's form shows only where the law asks for it. The answer says
   *  whether ads may load. */
  @Command
  fun consent(invoke: Invoke) {
    activity.runOnUiThread {
      val info = UserMessagingPlatform.getConsentInformation(activity)
      info.requestConsentInfoUpdate(
        activity,
        ConsentRequestParameters.Builder().build(),
        {
          UserMessagingPlatform.loadAndShowConsentFormIfRequired(activity) { error ->
            val answer = when {
              error != null -> "failed"
              info.consentStatus == ConsentInformation.ConsentStatus.NOT_REQUIRED -> "not_required"
              info.canRequestAds() -> "obtained"
              else -> "failed"
            }
            invoke.resolve(result(answer))
          }
        },
        { invoke.resolve(result("failed")) },
      )
    }
  }

  @Command
  fun loadRewarded(invoke: Invoke) {
    val args = invoke.parseArgs(UnitArgs::class.java)
    activity.runOnUiThread {
      if (rewarded != null) {
        invoke.resolve(result("ready"))
        return@runOnUiThread
      }
      RewardedAd.load(activity, args.unitId, AdRequest.Builder().build(), object : RewardedAdLoadCallback() {
        override fun onAdLoaded(ad: RewardedAd) {
          rewarded = ad
          invoke.resolve(result("ready"))
        }

        override fun onAdFailedToLoad(error: LoadAdError) {
          invoke.resolve(result("failed"))
        }
      })
    }
  }

  @Command
  fun showRewarded(invoke: Invoke) {
    activity.runOnUiThread {
      val ad = rewarded
      if (ad == null || showing != null) {
        invoke.resolve(result("failed"))
        return@runOnUiThread
      }
      rewarded = null
      showing = invoke
      earned = false
      ad.fullScreenContentCallback = object : FullScreenContentCallback() {
        override fun onAdDismissedFullScreenContent() {
          Log.i("ads", "[ads] rewarded ${if (earned) "earned" else "closed"}")
          showing?.resolve(result(if (earned) "earned" else "closed"))
          showing = null
        }

        override fun onAdFailedToShowFullScreenContent(error: AdError) {
          showing?.resolve(result("failed"))
          showing = null
        }
      }
      ad.show(activity) { earned = true }
    }
  }

  private fun rating(text: String) = when (text) {
    "PG" -> RequestConfiguration.MAX_AD_CONTENT_RATING_PG
    "T" -> RequestConfiguration.MAX_AD_CONTENT_RATING_T
    "MA" -> RequestConfiguration.MAX_AD_CONTENT_RATING_MA
    else -> RequestConfiguration.MAX_AD_CONTENT_RATING_G
  }
}
