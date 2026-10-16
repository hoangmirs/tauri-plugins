package dev.hoangmirs.firebase

import android.app.Activity
import android.os.Bundle
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.firebase.FirebaseApp
import com.google.firebase.analytics.FirebaseAnalytics
import com.google.firebase.remoteconfig.FirebaseRemoteConfig
import com.google.firebase.remoteconfig.FirebaseRemoteConfigFetchThrottledException
import com.google.firebase.remoteconfig.FirebaseRemoteConfigSettings

@InvokeArg
class ConfigureArgs {
  var minimumFetchIntervalSeconds: Long = 900
}

@InvokeArg
class CollectionArgs {
  var enabled: Boolean = false
}

@TauriPlugin
class FirebasePlugin(private val activity: Activity) : Plugin(activity) {
  // An app without google-services.json has no default FirebaseApp, and every
  // Firebase call would throw. It must still start on its defaults.
  private var configured = false

  override fun load(webView: WebView) {
    configured = FirebaseApp.getApps(activity).isNotEmpty() ||
      FirebaseApp.initializeApp(activity) != null
  }

  private fun config() = FirebaseRemoteConfig.getInstance()

  @Command
  fun configure(invoke: Invoke) {
    if (!configured) {
      invoke.resolve(JSObject().put("mode", "absent"))
      return
    }
    val args = invoke.parseArgs(ConfigureArgs::class.java)
    val settings = FirebaseRemoteConfigSettings.Builder()
      .setMinimumFetchIntervalInSeconds(args.minimumFetchIntervalSeconds)
      .build()
    config().setConfigSettingsAsync(settings)
    invoke.resolve(JSObject().put("mode", "native"))
  }

  @Command
  fun setDefaults(invoke: Invoke) {
    val given = invoke.getArgs().optJSONObject("defaults")
    val defaults = mutableMapOf<String, Any>()
    given?.keys()?.forEach { key -> given.optString(key, null)?.let { defaults[key] = it } }
    config().setDefaultsAsync(defaults).addOnCompleteListener { invoke.resolve() }
  }

  @Command
  fun fetch(invoke: Invoke) {
    config().fetch().addOnCompleteListener { task ->
      val result = when {
        task.isSuccessful -> "fetched"
        task.exception is FirebaseRemoteConfigFetchThrottledException -> "throttled"
        else -> "failed"
      }
      invoke.resolve(JSObject().put("result", result))
    }
  }

  @Command
  fun activate(invoke: Invoke) {
    config().activate().addOnCompleteListener { task ->
      invoke.resolve(JSObject().put("changed", task.isSuccessful && task.result == true))
    }
  }

  @Command
  fun getAll(invoke: Invoke) {
    val values = JSObject()
    for ((key, value) in config().all) {
      val source = when (value.source) {
        FirebaseRemoteConfig.VALUE_SOURCE_REMOTE -> "remote"
        FirebaseRemoteConfig.VALUE_SOURCE_DEFAULT -> "default"
        else -> "static"
      }
      values.put(key, JSObject().put("value", value.asString()).put("source", source))
    }
    invoke.resolve(JSObject().put("values", values))
  }

  @Command
  fun logEvent(invoke: Invoke) {
    val args = invoke.getArgs()
    val name = args.optString("name", "")
    if (name.isEmpty()) {
      invoke.resolve()
      return
    }
    val params = Bundle()
    val given = args.optJSONObject("params")
    given?.keys()?.forEach { key ->
      when (val v = given.get(key)) {
        is String -> params.putString(key, v)
        is Int -> params.putLong(key, v.toLong())
        is Long -> params.putLong(key, v)
        is Number -> params.putDouble(key, v.toDouble())
      }
    }
    FirebaseAnalytics.getInstance(activity).logEvent(name, params)
    invoke.resolve()
  }

  @Command
  fun setCollection(invoke: Invoke) {
    val args = invoke.parseArgs(CollectionArgs::class.java)
    FirebaseAnalytics.getInstance(activity).setAnalyticsCollectionEnabled(args.enabled)
    invoke.resolve()
  }
}
