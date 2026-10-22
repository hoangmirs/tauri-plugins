package dev.hoangmirs.ads

import android.app.Activity
import android.os.SystemClock
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.ads.AdError
import com.google.android.gms.ads.AdRequest
import com.google.android.gms.ads.AdValue
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

@InvokeArg
class MutedArgs {
  var muted: Boolean = false
}

private fun result(text: String) = JSObject().put("result", text)

/** Google tells apps not to show an ad loaded more than an hour ago. */
const val STALE_AFTER_MS = 60 * 60 * 1000L

/** What Google says one ad earned, as the `paid` event's data. */
fun paidData(format: String, value: AdValue): JSObject = JSObject()
  .put("format", format)
  .put("micros", value.valueMicros)
  .put("currency", value.currencyCode)
  .put(
    "precision",
    when (value.precisionType) {
      AdValue.PrecisionType.ESTIMATED -> "estimated"
      AdValue.PrecisionType.PUBLISHER_PROVIDED -> "publisher"
      AdValue.PrecisionType.PRECISE -> "precise"
      else -> "unknown"
    },
  )

@TauriPlugin
class AdsPlugin(private val activity: Activity) : Plugin(activity) {
  /** One rewarded ad, loaded before the player asks for it, with its unit
   *  and the time it loaded. */
  private var rewarded: RewardedAd? = null
  private var rewardedUnit: String? = null
  private var rewardedAt = 0L
  /** Set once the SDK starts. It starts after consent, as Google asks. */
  private var started = false
  /** The `showRewarded` call waiting for the ad to close. */
  private var showing: Invoke? = null
  private var earned = false
  /** From `init`. A child cannot give consent, so the consent form is told. */
  private var childDirected = true
  private val native by lazy {
    NativeCard(
      activity,
      send = { event -> trigger("native", JSObject().put("event", event)) },
      paid = { value -> trigger("paid", paidData("native", value)) },
    )
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
    childDirected = args.childDirected
    val config = RequestConfiguration.Builder()
      .setTagForChildDirectedTreatment(
        if (args.childDirected) RequestConfiguration.TAG_FOR_CHILD_DIRECTED_TREATMENT_TRUE
        else RequestConfiguration.TAG_FOR_CHILD_DIRECTED_TREATMENT_FALSE
      )
      .setTagForUnderAgeOfConsent(
        if (args.childDirected) RequestConfiguration.TAG_FOR_UNDER_AGE_OF_CONSENT_TRUE
        else RequestConfiguration.TAG_FOR_UNDER_AGE_OF_CONSENT_FALSE
      )
      .setMaxAdContentRating(rating(args.maxRating))
      .build()
    MobileAds.setRequestConfiguration(config)
    invoke.resolve()
  }

  private fun start() {
    if (started) return
    started = true
    // Google asks for the start off the main thread: it reads from disk.
    thread { MobileAds.initialize(activity) { Log.i("ads", "[ads] started") } }
  }

  /** Google's form shows only where the law asks for it. The answer says
   *  whether ads may load. */
  @Command
  fun consent(invoke: Invoke) {
    activity.runOnUiThread {
      val info = UserMessagingPlatform.getConsentInformation(activity)
      info.requestConsentInfoUpdate(
        activity,
        ConsentRequestParameters.Builder().setTagForUnderAgeOfConsent(childDirected).build(),
        {
          UserMessagingPlatform.loadAndShowConsentFormIfRequired(activity) { error ->
            val answer = when {
              error != null -> "failed"
              info.consentStatus == ConsentInformation.ConsentStatus.NOT_REQUIRED -> "not_required"
              info.canRequestAds() -> "obtained"
              else -> "failed"
            }
            if (answer != "failed") start()
            invoke.resolve(result(answer))
          }
        },
        {
          // Offline, say. A consent from an earlier session still counts.
          val ok = info.canRequestAds()
          if (ok) start()
          invoke.resolve(result(if (ok) "obtained" else "failed"))
        },
      )
    }
  }

  @Command
  fun privacyOptions(invoke: Invoke) {
    activity.runOnUiThread {
      val status = UserMessagingPlatform.getConsentInformation(activity).privacyOptionsRequirementStatus
      val required = status == ConsentInformation.PrivacyOptionsRequirementStatus.REQUIRED
      invoke.resolve(result(if (required) "required" else "not_required"))
    }
  }

  @Command
  fun showPrivacyOptions(invoke: Invoke) {
    activity.runOnUiThread {
      UserMessagingPlatform.showPrivacyOptionsForm(activity) { error ->
        invoke.resolve(result(if (error == null) "shown" else "failed"))
      }
    }
  }

  @Command
  fun setMuted(invoke: Invoke) {
    val args = invoke.parseArgs(MutedArgs::class.java)
    activity.runOnUiThread {
      MobileAds.setAppMuted(args.muted)
      invoke.resolve()
    }
  }

  @Command
  fun loadRewarded(invoke: Invoke) {
    val args = invoke.parseArgs(UnitArgs::class.java)
    activity.runOnUiThread {
      loadRewarded(args.unitId) { ok -> invoke.resolve(result(if (ok) "ready" else "failed")) }
    }
  }

  /** Keeps an ad that is fresh and from `unit`, and loads one otherwise. */
  private fun loadRewarded(unit: String, done: (Boolean) -> Unit) {
    val fresh = SystemClock.elapsedRealtime() - rewardedAt < STALE_AFTER_MS
    if (rewarded != null && rewardedUnit == unit && fresh) {
      done(true)
      return
    }
    rewarded = null
    RewardedAd.load(activity, unit, AdRequest.Builder().build(), object : RewardedAdLoadCallback() {
      override fun onAdLoaded(ad: RewardedAd) {
        ad.setOnPaidEventListener { value -> trigger("paid", paidData("rewarded", value)) }
        rewarded = ad
        rewardedUnit = unit
        rewardedAt = SystemClock.elapsedRealtime()
        done(true)
      }

      override fun onAdFailedToLoad(error: LoadAdError) {
        done(false)
      }
    })
  }

  @Command
  fun showRewarded(invoke: Invoke) {
    activity.runOnUiThread {
      val ad = rewarded
      if (ad == null || showing != null) {
        invoke.resolve(result("failed"))
        return@runOnUiThread
      }
      val unit = rewardedUnit
      if (SystemClock.elapsedRealtime() - rewardedAt >= STALE_AFTER_MS && unit != null) {
        rewarded = null
        invoke.resolve(result("failed"))
        loadRewarded(unit) {}
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
