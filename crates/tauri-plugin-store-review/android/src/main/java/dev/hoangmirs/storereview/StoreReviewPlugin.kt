package dev.hoangmirs.storereview

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import com.google.android.play.core.review.ReviewManagerFactory

@TauriPlugin
class StoreReviewPlugin(private val activity: Activity) : Plugin(activity) {
  private val manager = ReviewManagerFactory.create(activity)

  // Play accepts one ReviewInfo for one prompt. A kept one shows nothing the
  // second time, so each call asks Play for a new one.
  @Command
  fun requestReview(invoke: Invoke) {
    manager.requestReviewFlow().addOnCompleteListener { info ->
      if (!info.isSuccessful) {
        invoke.reject(info.exception?.message ?: "Play gave no review info")
        return@addOnCompleteListener
      }
      manager.launchReviewFlow(activity, info.result).addOnCompleteListener { invoke.resolve() }
    }
  }
}
