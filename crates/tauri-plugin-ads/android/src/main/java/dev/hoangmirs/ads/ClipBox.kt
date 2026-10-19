package dev.hoangmirs.ads

import android.content.Context
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout
import kotlin.math.abs

/**
 * The visible part of the page's scroll box, which holds the card. The card
 * sits over the WebView, so a drag that starts on it would not reach the page,
 * and its lift could count as a tap on the ad. Once a touch moves up or down
 * past the touch slop, the box cancels the card's touch and replays the drag
 * to the WebView, which scrolls the page as if the card were not there.
 */
class ClipBox(context: Context, private val page: () -> View?) : FrameLayout(context) {
  private val slop = ViewConfiguration.get(context).scaledTouchSlop
  private var down: MotionEvent? = null
  private var forwarding = false

  init {
    clipChildren = true
  }

  override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
    val target = page()
    if (ev.actionMasked == MotionEvent.ACTION_DOWN) {
      down?.recycle()
      down = MotionEvent.obtain(ev)
      forwarding = false
    }
    val start = down
    if (!forwarding && target != null && start != null && ev.actionMasked == MotionEvent.ACTION_MOVE) {
      val dy = abs(ev.rawY - start.rawY)
      if (dy > slop && dy > abs(ev.rawX - start.rawX)) {
        forwarding = true
        val cancel = MotionEvent.obtain(ev).apply { action = MotionEvent.ACTION_CANCEL }
        super.dispatchTouchEvent(cancel)
        cancel.recycle()
        replay(target, start)
      }
    }
    if (forwarding && target != null) {
      replay(target, ev)
      if (ev.actionMasked == MotionEvent.ACTION_UP || ev.actionMasked == MotionEvent.ACTION_CANCEL) {
        forwarding = false
      }
      return true
    }
    return super.dispatchTouchEvent(ev)
  }

  /** Sends the event to the page at the same place on screen. */
  private fun replay(target: View, ev: MotionEvent) {
    val at = IntArray(2)
    target.getLocationOnScreen(at)
    val copy = MotionEvent.obtain(ev)
    copy.setLocation(ev.rawX - at[0], ev.rawY - at[1])
    target.dispatchTouchEvent(copy)
    copy.recycle()
  }
}
