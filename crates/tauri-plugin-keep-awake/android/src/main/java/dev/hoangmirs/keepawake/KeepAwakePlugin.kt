package dev.hoangmirs.keepawake

import android.app.Activity
import android.view.WindowManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class SetArgs {
  var on: Boolean = false
}

/** The window flag holds only while the activity is in front, so the phone
 *  sleeps as usual once the app goes to the background. */
@TauriPlugin
class KeepAwakePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun set(invoke: Invoke) {
    val args = invoke.parseArgs(SetArgs::class.java)
    // A window's flags change only on the thread that made the window.
    activity.runOnUiThread {
      if (args.on) {
        activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      } else {
        activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      }
      invoke.resolve()
    }
  }
}
