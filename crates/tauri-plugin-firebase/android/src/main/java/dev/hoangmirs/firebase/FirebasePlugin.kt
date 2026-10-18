package dev.hoangmirs.firebase

import android.app.Activity
import android.os.Bundle
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.firebase.FirebaseApp
import com.google.firebase.analytics.FirebaseAnalytics
import com.google.firebase.crashlytics.FirebaseCrashlytics
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

@InvokeArg
class CrashKeysArgs {
  var keys: Map<String, String> = emptyMap()
}

@InvokeArg
class CrashLogArgs {
  var line: String = ""
}

@InvokeArg
class CrashFrameArgs {
  var symbol: String = ""
  var file: String = ""
  var line: Int = 0
}

@InvokeArg
class CrashRecordArgs {
  var name: String = ""
  var message: String = ""
  var frames: List<CrashFrameArgs> = emptyList()
}

// Crashlytics groups a non-fatal by its stack, so every frame carries the
// error's name as its class. A record with no frames still gets one frame,
// named after the error, so it groups by name too.
class RecordedError(name: String, message: String, frames: List<CrashFrameArgs>) : Exception(message) {
  init {
    stackTrace = frames
      .map { StackTraceElement(name, it.symbol, it.file, it.line) }
      .ifEmpty { listOf(StackTraceElement(name, name, null, -1)) }
      .toTypedArray()
  }
}

@TauriPlugin
class FirebasePlugin(private val activity: Activity) : Plugin(activity) {
  // An app without google-services.json has no default FirebaseApp, and every
  // Firebase call would throw. It must still start on its defaults. Decided
  // here, not in `load`: Tauri calls `load` only once a webview exists, which
  // is after Rust asks `configure` for a plugin registered on the builder.
  private val configured = FirebaseApp.getApps(activity).isNotEmpty() ||
    FirebaseApp.initializeApp(activity) != null

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

  @Command
  fun crashSetCollection(invoke: Invoke) = crash(invoke) {
    it.setCrashlyticsCollectionEnabled(invoke.parseArgs(CollectionArgs::class.java).enabled)
  }

  @Command
  fun crashSetKeys(invoke: Invoke) = crash(invoke) { crashlytics ->
    for ((key, value) in invoke.parseArgs(CrashKeysArgs::class.java).keys) {
      crashlytics.setCustomKey(key, value)
    }
  }

  @Command
  fun crashLog(invoke: Invoke) = crash(invoke) {
    it.log(invoke.parseArgs(CrashLogArgs::class.java).line)
  }

  @Command
  fun crashRecord(invoke: Invoke) = crash(invoke) {
    val args = invoke.parseArgs(CrashRecordArgs::class.java)
    it.recordException(RecordedError(args.name, args.message, args.frames))
  }

  // Rust's panic hook blocks until each crash method resolves. Resolve here on
  // the calling thread, never in a Task listener or a post to a looper, so the
  // hook returns at once. A failed parse or call is dropped: the hook has
  // nowhere to report it.
  private fun crash(invoke: Invoke, call: (FirebaseCrashlytics) -> Unit) {
    if (configured) {
      try {
        call(FirebaseCrashlytics.getInstance())
      } catch (e: Exception) {
      }
    }
    invoke.resolve()
  }
}
