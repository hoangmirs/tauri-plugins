package dev.hoangmirs.launch

import android.app.Activity
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.FrameLayout
import android.widget.ImageView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class LaunchConfig {
  var image: String = "LaunchFrame"
  var background: String = "#ffffff"
  var timeoutMs: Long = 3000
}

/**
 * The launch frame again, over the web view, from the moment the web view
 * loads until the page calls `ready`. Android takes its own launch screen down
 * as soon as the activity draws, a moment before the page paints.
 */
@TauriPlugin
class LaunchPlugin(private val activity: Activity) : Plugin(activity) {
  private var cover: View? = null
  private val main = Handler(Looper.getMainLooper())

  override fun load(webView: WebView) {
    val config = runCatching { getConfig(LaunchConfig::class.java) }.getOrNull() ?: LaunchConfig()
    val root = activity.findViewById<ViewGroup>(android.R.id.content) ?: return
    val frame = FrameLayout(activity)
    frame.setBackgroundColor(runCatching { Color.parseColor(config.background) }.getOrDefault(Color.WHITE))
    // A drawable's name is lower case. The iOS asset name may not be.
    val id = activity.resources.getIdentifier(config.image.lowercase(), "drawable", activity.packageName)
    if (id != 0) {
      val image = ImageView(activity)
      image.setImageResource(id)
      frame.addView(
        image,
        FrameLayout.LayoutParams(
          ViewGroup.LayoutParams.WRAP_CONTENT,
          ViewGroup.LayoutParams.WRAP_CONTENT,
          Gravity.CENTER,
        ),
      )
    }
    // Takes no touches: a tap goes to the page, which already shows the same frame.
    frame.isClickable = false
    root.addView(frame, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    cover = frame
    // A page that never calls `ready` must not leave the app covered.
    main.postDelayed({ lift() }, config.timeoutMs.coerceIn(0, 10_000))
  }

  @Command
  fun ready(invoke: Invoke) {
    activity.runOnUiThread {
      lift()
      invoke.resolve()
    }
  }

  private fun lift() {
    val frame = cover ?: return
    (frame.parent as? ViewGroup)?.removeView(frame)
    cover = null
  }
}
