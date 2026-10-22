import GoogleMobileAds
import Tauri
import UIKit
import WebKit

class SlotArgs: Decodable {
  let unitId: String
  let x: Double
  let y: Double
  let vx: Double
  let vy: Double
  let w: Double
  let h: Double
  let scrollHeight: Double
  /// The visible part of the scroll box on screen. A bar the page pins inside
  /// the box, such as a top bar, lies outside it.
  let clipTop: Double?
  let clipBottom: Double?
  /// The words on the ad label, in the player's language.
  let label: String?
  let ink: String
  let muted: String
  let button: String
  let edge: String
}

/// One native ad, drawn on a clear view over the box the page draws. Google's
/// validator asks for a media view of 120 points or more and no asset on top
/// of another, so each asset sits in its own band.
class NativeCard: NSObject, NativeAdLoaderDelegate, NativeAdDelegate {
  private let send: (String) -> Void
  private var loader: AdLoader?
  private var ad: NativeAd?
  private var card: NativeAdView?
  private var slot: SlotArgs?
  /// Set by `hide`. An ad that arrives after the page left the slot stays
  /// hidden until the next `show`.
  private var hidden = true

  private let label = UILabel()
  private let headline = UILabel()
  private let line = UILabel()
  private let action = UIButton(type: .custom)
  private let webView: () -> WKWebView?
  /// The visible part of the page's scroll box, for a card that cannot ride
  /// in the scroll box itself. It keeps the card off the tab bar.
  private let clip: UIView = {
    let view = PassThrough()
    view.clipsToBounds = true
    view.backgroundColor = .clear
    return view
  }()
  /// The same clip for a card that rides in the scroll box. It moves with each
  /// scroll, so the card scrolls with the page and is still cut at the clip.
  /// Without it the card slid over a top bar the page pins inside the box:
  /// WebKit draws that bar in the box too, under any view added there.
  private let ride: UIView = {
    let view = PassThrough()
    view.clipsToBounds = true
    view.backgroundColor = .clear
    return view
  }()
  private var riding: UIScrollView?
  private var scrolled: NSKeyValueObservation?

  init(send: @escaping (String) -> Void, webView: @escaping () -> WKWebView?) {
    self.send = send
    self.webView = webView
  }

  func show(_ slot: SlotArgs) {
    self.slot = slot
    hidden = false
    if loader == nil {
      let choices = NativeAdViewAdOptions()
      choices.preferredAdChoicesPosition = .topRightCorner
      let loader = AdLoader(
        adUnitID: slot.unitId, rootViewController: AdsPlugin.topController(),
        adTypes: [.native], options: [choices])
      loader.delegate = self
      loader.load(Request())
      self.loader = loader
    }
    place()
  }

  func hide() {
    hidden = true
    card?.isHidden = true
    clip.isHidden = true
    ride.isHidden = true
  }

  func adLoader(_ adLoader: AdLoader, didReceive nativeAd: NativeAd) {
    ad = nativeAd
    nativeAd.delegate = self
    build(nativeAd)
    send("native_loaded")
    place()
  }

  func adLoader(_ adLoader: AdLoader, didFailToReceiveAdWithError error: Error) {
    send("native_failed")
  }

  func nativeAdDidRecordImpression(_ nativeAd: NativeAd) {
    send("native_shown")
  }

  func nativeAdDidRecordClick(_ nativeAd: NativeAd) {
    send("native_tapped")
  }

  private func build(_ nativeAd: NativeAd) {
    let card = NativeAdView()
    card.backgroundColor = .clear

    let media = MediaView()
    media.contentMode = .scaleAspectFit
    media.layer.cornerRadius = 18
    media.clipsToBounds = true
    media.mediaContent = nativeAd.mediaContent
    card.mediaView = media

    label.font = Self.rounded(15, .heavy)
    label.textColor = UIColor(red: 0.16, green: 0.13, blue: 0, alpha: 1)
    label.backgroundColor = UIColor(red: 1, green: 0.83, blue: 0.30, alpha: 1)
    label.layer.cornerRadius = 8
    label.clipsToBounds = true

    headline.font = Self.rounded(17, .heavy)
    headline.text = nativeAd.headline
    card.headlineView = headline

    line.font = Self.rounded(15, .semibold)
    line.text = nativeAd.advertiser ?? nativeAd.body
    if nativeAd.advertiser != nil { card.advertiserView = line } else { card.bodyView = line }

    // Google takes the tap on the whole card. The button only shows the words.
    action.setTitle(nativeAd.callToAction, for: .normal)
    action.titleLabel?.font = Self.rounded(16, .heavy)
    action.layer.cornerRadius = 16
    action.layer.borderWidth = 2
    action.isUserInteractionEnabled = false
    card.callToActionView = action

    for view in [media, label, headline, line, action] { card.addSubview(view) }
    card.nativeAd = nativeAd
    self.card = card
  }

  private func place() {
    guard let card, let slot, let web = webView() ?? Self.findWebView() else { return }
    label.text = " \(slot.label ?? "Ad") "
    let w = CGFloat(slot.w), h = CGFloat(slot.h)

    let ink = Self.rgb(slot.ink) ?? .label
    headline.textColor = ink
    line.textColor = Self.rgb(slot.muted) ?? .secondaryLabel
    action.setTitleColor(ink, for: .normal)
    action.backgroundColor = Self.rgb(slot.button) ?? .secondarySystemBackground
    action.layer.borderColor = (Self.rgb(slot.edge) ?? .separator).cgColor

    // WebKit gives each scrolling box of the page its own scroll view. The
    // card rides in the one as tall as the page's box, so it scrolls with the
    // page and never lags.
    // A box that does not overflow has no scroll view of its own. The card
    // then sits over the page, clipped to the box's visible part.
    if let scroller = Self.scroller(in: web.scrollView, height: CGFloat(slot.scrollHeight), skip: web.scrollView) {
      if ride.superview !== scroller { scroller.addSubview(ride) }
      if card.superview !== ride { ride.addSubview(card) }
      if riding !== scroller {
        riding = scroller
        scrolled = scroller.observe(\.contentOffset) { [weak self] _, _ in self?.follow() }
      }
      follow()
      clip.isHidden = true
      ride.isHidden = hidden
    } else {
      scrolled = nil
      riding = nil
      ride.isHidden = true
      let top = CGFloat(slot.clipTop ?? 0)
      let bottom = CGFloat(slot.clipBottom ?? Double(web.bounds.height))
      if clip.superview !== web { web.addSubview(clip) }
      clip.frame = CGRect(x: 0, y: top, width: web.bounds.width, height: max(0, bottom - top))
      if card.superview !== clip { clip.addSubview(card) }
      card.frame = CGRect(x: slot.vx, y: slot.vy - top, width: w, height: h)
      clip.isHidden = hidden
    }
    card.isHidden = hidden

    let side: CGFloat = 120
    let pad = max(8, (h - side) / 2)
    card.mediaView?.frame = CGRect(x: pad, y: pad, width: side, height: side)
    let tx = pad + side + 14, tw = w - pad - tx
    label.sizeToFit()
    label.frame = CGRect(x: tx, y: pad + 4, width: label.bounds.width, height: 24)
    headline.frame = CGRect(x: tx, y: pad + 32, width: tw, height: 22)
    line.frame = CGRect(x: tx, y: pad + 56, width: tw, height: 20)
    action.sizeToFit()
    action.frame = CGRect(x: tx, y: h - pad - 40, width: max(action.bounds.width + 28, 96), height: 40)
  }

  /// Moves the riding clip to the part of the scroll box on screen, and the
  /// card inside it to its place in the box's content.
  private func follow() {
    guard let slot, let card, let scroller = riding, let web = webView() ?? Self.findWebView() else { return }
    let shown = scroller.convert(CGPoint(x: 0, y: scroller.contentOffset.y), to: web).y
    let top = CGFloat(slot.clipTop ?? Double(shown))
    let bottom = CGFloat(slot.clipBottom ?? Double(shown + scroller.bounds.height))
    let y = scroller.contentOffset.y + top - shown
    ride.frame = CGRect(x: 0, y: y, width: scroller.bounds.width, height: max(0, bottom - top))
    card.frame = CGRect(x: slot.x, y: slot.y - y, width: slot.w, height: slot.h)
  }

  /// Used only before the plugin hears of its web view.
  private static func findWebView() -> WKWebView? {
    UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
      .lazy
      .compactMap { find(in: $0) }
      .first
  }

  private static func find(in view: UIView) -> WKWebView? {
    if let web = view as? WKWebView { return web }
    for sub in view.subviews { if let web = find(in: sub) { return web } }
    return nil
  }

  private static func scroller(in view: UIView, height: CGFloat, skip: UIScrollView) -> UIScrollView? {
    for sub in view.subviews {
      if let scroll = sub as? UIScrollView, scroll !== skip, abs(scroll.contentSize.height - height) < 2 {
        return scroll
      }
      if let found = scroller(in: sub, height: height, skip: skip) { return found }
    }
    return nil
  }

  /// A CSS `rgb(r, g, b)` or `rgba(…)` colour.
  private static func rgb(_ css: String) -> UIColor? {
    let parts = css.split(whereSeparator: { !"0123456789.".contains($0) }).compactMap { Double($0) }
    guard parts.count >= 3 else { return nil }
    return UIColor(red: parts[0] / 255, green: parts[1] / 255, blue: parts[2] / 255, alpha: 1)
  }

  private static func rounded(_ size: CGFloat, _ weight: UIFont.Weight) -> UIFont {
    let font = UIFont.systemFont(ofSize: size, weight: weight)
    guard let desc = font.fontDescriptor.withDesign(.rounded) else { return font }
    return UIFont(descriptor: desc, size: size)
  }
}

/// A clip over the page that takes no touch itself. Only the card inside it
/// does: a plain view the size of the visible page took every tap meant for
/// the page under it.
private final class PassThrough: UIView {
  override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
    let hit = super.hitTest(point, with: event)
    return hit === self ? nil : hit
  }
}
