package dev.hoangmirs.ads

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.FrameLayout
import android.widget.TextView
import app.tauri.annotation.InvokeArg
import com.google.android.gms.ads.AdListener
import com.google.android.gms.ads.AdLoader
import com.google.android.gms.ads.AdRequest
import com.google.android.gms.ads.LoadAdError
import com.google.android.gms.ads.nativead.MediaView
import com.google.android.gms.ads.nativead.NativeAd
import com.google.android.gms.ads.nativead.NativeAdOptions
import com.google.android.gms.ads.nativead.NativeAdView
import kotlin.math.max

@InvokeArg
class SlotArgs {
  var unitId: String = ""
  var x: Double = 0.0
  var y: Double = 0.0
  var vx: Double = 0.0
  var vy: Double = 0.0
  var w: Double = 0.0
  var h: Double = 0.0
  var scrollHeight: Double = 0.0
  var clipTop: Double? = null
  var clipBottom: Double? = null
  var ink: String = ""
  var muted: String = ""
  var button: String = ""
  var edge: String = ""
}

/**
 * One native ad, drawn on a clear view over the box the page draws. Android's
 * WebView has no native view for a scrolling box of the page, so the card
 * sits over the WebView at the box's place on screen, and each `show` moves
 * it. Google's validator asks for a media view of 120 dp or more and no asset
 * on top of another, so each asset sits in its own band.
 */
class NativeCard(private val activity: Activity, private val send: (String) -> Unit) {
  private var loader: AdLoader? = null
  private var card: NativeAdView? = null
  /** The visible part of the page's scroll box. The card sits inside it, so it
   *  never draws over the tab bar or past the screen's edge. */
  private val clip = FrameLayout(activity).apply { clipChildren = true }
  private var slot: SlotArgs? = null
  /** Set by `hide`. An ad that arrives after the page left the slot stays
   *  hidden until the next `show`. */
  private var hidden = true

  private val label = TextView(activity)
  private val headline = TextView(activity)
  private val line = TextView(activity)
  private val action = TextView(activity)
  private val density = activity.resources.displayMetrics.density

  fun show(slot: SlotArgs) {
    this.slot = slot
    hidden = false
    if (loader == null) {
      val options = NativeAdOptions.Builder()
        .setAdChoicesPlacement(NativeAdOptions.ADCHOICES_TOP_RIGHT)
        .build()
      loader = AdLoader.Builder(activity, slot.unitId)
        .forNativeAd { ad ->
          build(ad)
          send("native_loaded")
          place()
        }
        .withAdListener(object : AdListener() {
          override fun onAdFailedToLoad(error: LoadAdError) = send("native_failed")
          override fun onAdImpression() = send("native_shown")
          override fun onAdClicked() = send("native_tapped")
        })
        .withNativeAdOptions(options)
        .build()
        .also { it.loadAd(AdRequest.Builder().build()) }
    }
    place()
  }

  fun hide() {
    hidden = true
    clip.visibility = View.GONE
  }

  private fun px(dp: Double) = (dp * density).toInt()

  private fun build(ad: NativeAd) {
    val card = NativeAdView(activity)
    card.setBackgroundColor(Color.TRANSPARENT)

    val media = MediaView(activity)
    media.clipToOutline = true
    media.background = GradientDrawable().apply { cornerRadius = 18 * density }
    card.mediaView = media

    label.text = " Ad "
    label.textSize = 15f
    label.typeface = Typeface.DEFAULT_BOLD
    label.setTextColor(Color.rgb(41, 33, 0))
    label.background = GradientDrawable().apply {
      cornerRadius = 8 * density
      setColor(Color.rgb(255, 212, 77))
    }

    headline.textSize = 17f
    headline.typeface = Typeface.DEFAULT_BOLD
    headline.maxLines = 1
    headline.ellipsize = TextUtils.TruncateAt.END
    headline.text = ad.headline
    card.headlineView = headline

    line.textSize = 15f
    line.maxLines = 1
    line.ellipsize = TextUtils.TruncateAt.END
    line.text = ad.advertiser ?: ad.body
    if (ad.advertiser != null) card.advertiserView = line else card.bodyView = line

    // Google takes the tap on the whole card. The button only shows the words.
    action.text = ad.callToAction
    action.textSize = 16f
    action.typeface = Typeface.DEFAULT_BOLD
    action.gravity = Gravity.CENTER
    action.setPadding(px(14.0), 0, px(14.0), 0)
    card.callToActionView = action

    for (view in listOf(media, label, headline, line, action)) card.addView(view)
    card.setNativeAd(ad)
    this.card = card
  }

  private fun place() {
    val card = card ?: return
    val slot = slot ?: return
    val web = webView(activity.window.decorView) ?: return
    val root = web.parent as? ViewGroup ?: return

    val ink = rgb(slot.ink) ?: Color.BLACK
    headline.setTextColor(ink)
    line.setTextColor(rgb(slot.muted) ?: Color.DKGRAY)
    action.setTextColor(ink)
    action.background = GradientDrawable().apply {
      cornerRadius = 16 * density
      setColor(rgb(slot.button) ?: Color.LTGRAY)
      setStroke(px(2.0), rgb(slot.edge) ?: Color.GRAY)
    }

    if (clip.parent !== root) {
      (clip.parent as? ViewGroup)?.removeView(clip)
      root.addView(clip)
    }
    if (card.parent !== clip) {
      (card.parent as? ViewGroup)?.removeView(card)
      clip.addView(card)
    }
    val top = slot.clipTop ?: 0.0
    val bottom = slot.clipBottom ?: (web.height / density).toDouble()
    clip.layoutParams = FrameLayout.LayoutParams(web.width, px(max(0.0, bottom - top)))
    clip.x = web.x
    clip.y = web.y + px(top)
    card.layoutParams = FrameLayout.LayoutParams(px(slot.w), px(slot.h))
    card.x = px(slot.vx).toFloat()
    card.y = px(slot.vy - top).toFloat()
    clip.visibility = if (hidden) View.GONE else View.VISIBLE

    val side = 120.0
    val pad = max(8.0, (slot.h - side) / 2)
    val tx = pad + side + 14
    val tw = slot.w - pad - tx
    fun put(view: View, x: Double, y: Double, w: Int, h: Double) {
      view.layoutParams = FrameLayout.LayoutParams(w, px(h)).apply {
        leftMargin = px(x)
        topMargin = px(y)
      }
    }
    put(card.mediaView!!, pad, pad, px(side), side)
    put(label, tx, pad + 4, ViewGroup.LayoutParams.WRAP_CONTENT, 24.0)
    put(headline, tx, pad + 32, px(tw), 22.0)
    put(line, tx, pad + 56, px(tw), 20.0)
    put(action, tx, slot.h - pad - 40, ViewGroup.LayoutParams.WRAP_CONTENT, 40.0)
    action.minWidth = px(96.0)
  }

  private fun webView(view: View): WebView? {
    if (view is WebView) return view
    if (view is ViewGroup) {
      for (i in 0 until view.childCount) webView(view.getChildAt(i))?.let { return it }
    }
    return null
  }

  /** A CSS `rgb(r, g, b)` or `rgba(…)` colour. */
  private fun rgb(css: String): Int? {
    val parts = Regex("[0-9.]+").findAll(css).map { it.value.toDouble() }.toList()
    if (parts.size < 3) return null
    return Color.rgb(parts[0].toInt(), parts[1].toInt(), parts[2].toInt())
  }
}
