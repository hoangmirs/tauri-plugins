package dev.hoangmirs.silentmode

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import android.os.Build
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Answers how the phone's ringer is set, and sends `change` when it moves:
 *  a player flipping the phone to silent mid-game hears it go quiet at once,
 *  with nothing asked for before each sound. */
@TauriPlugin
class SilentModePlugin(private val activity: Activity) : Plugin(activity) {
  private val audio = activity.getSystemService(Context.AUDIO_SERVICE) as AudioManager
  private var receiver: BroadcastReceiver? = null

  private fun now(): JSObject = JSObject().apply { put("mode", Modes.name(audio.ringerMode)) }

  override fun load(webView: WebView) {
    super.load(webView)
    val r =
      object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
          trigger("change", now())
        }
      }
    val filter = IntentFilter(AudioManager.RINGER_MODE_CHANGED_ACTION)
    // A system broadcast still reaches a receiver no other app may send to.
    if (Build.VERSION.SDK_INT >= 33) {
      activity.registerReceiver(r, filter, Context.RECEIVER_NOT_EXPORTED)
    } else {
      activity.registerReceiver(r, filter)
    }
    receiver = r
  }

  override fun onDestroy() {
    receiver?.let { runCatching { activity.unregisterReceiver(it) } }
    receiver = null
    super.onDestroy()
  }

  @Command
  fun state(invoke: Invoke) {
    invoke.resolve(now())
  }
}
