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
  private let paid: (AdValue) -> Void
  private var loader: AdLoader?
  private var loaderUnit: String?
  private var ad: NativeAd?
  /// When the current ad loaded and when it recorded its impression. A show
  /// after a hide gets a new ad once either is old enough.
  private var loadedAt = Date.distantPast
  private var impressionAt: Date?
  /// A failed load tries again after these waits, then after the last one
  /// each time. `tries` counts the failures in a row, and `round` drops a
  /// wait from an older loader.
  private let waits: [TimeInterval] = [30, 120, 600]
  private var tries = 0
  private var round = 0
  /// No retry runs once the app has sent no show for this long.
  private let idle: TimeInterval = 30 * 60
  private var lastShow = Date.distantPast
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
  /// Over the web view. WebKit's own hit test never returns a view it did not
  /// make, so a tap on the card reached the page and never the ad. This view
  /// hands a touch on the card's visible part to the card.
  private lazy var catcher = Catcher { [weak self] point, from in self?.target(point, from: from) }
  private var scrolled: NSKeyValueObservation?

  init(
    send: @escaping (String) -> Void, webView: @escaping () -> WKWebView?,
    paid: @escaping (AdValue) -> Void
  ) {
    self.send = send
    self.webView = webView
    self.paid = paid
  }

  func show(_ slot: SlotArgs) {
    let returning = hidden
    self.slot = slot
    hidden = false
    lastShow = Date()
    if loader == nil || loaderUnit != slot.unitId {
      start(slot.unitId)
    } else if returning, ad != nil, due() {
      load()
    }
    place()
  }

  /// Whether the current ad has had its time: an impression a minute ago or
  /// more, or a load over an hour ago.
  private func due() -> Bool {
    if Date().timeIntervalSince(loadedAt) >= staleAfter { return true }
    guard let impressionAt else { return false }
    return Date().timeIntervalSince(impressionAt) >= 60
  }

  /// A new loader for `unit`. The old ad and its card go.
  private func start(_ unit: String) {
    round += 1
    tries = 0
    drop()
    let choices = NativeAdViewAdOptions()
    choices.preferredAdChoicesPosition = .topRightCorner
    let loader = AdLoader(
      adUnitID: unit, rootViewController: AdsPlugin.topController(),
      adTypes: [.native], options: [choices])
    loader.delegate = self
    self.loader = loader
    loaderUnit = unit
    load()
  }

  private func load() {
    guard let loader, !loader.isLoading else { return }
    loader.load(Request())
  }

  /// Lets the current ad go. The card stays empty until the next ad.
  private func drop() {
    card?.removeFromSuperview()
    card?.nativeAd = nil
    card = nil
    ad = nil
    impressionAt = nil
  }

  func hide() {
    hidden = true
    card?.isHidden = true
    clip.isHidden = true
    ride.isHidden = true
  }

  func adLoader(_ adLoader: AdLoader, didReceive nativeAd: NativeAd) {
    guard adLoader === loader else { return }
    // The old ad stays on the card until this one arrives.
    drop()
    ad = nativeAd
    loadedAt = Date()
    tries = 0
    nativeAd.delegate = self
    nativeAd.paidEventHandler = { [weak self] value in self?.paid(value) }
    build(nativeAd)
    send("native_loaded")
    place()
  }

  func adLoader(_ adLoader: AdLoader, didFailToReceiveAdWithError error: Error) {
    guard adLoader === loader else { return }
    // A failed refresh keeps the ad on the card. Only a card with no ad says so.
    if ad == nil { send("native_failed") }
    let wait = waits[min(tries, waits.count - 1)]
    tries += 1
    let current = round
    DispatchQueue.main.asyncAfter(deadline: .now() + wait) { [weak self] in
      guard let self, self.round == current else { return }
      guard Date().timeIntervalSince(self.lastShow) < self.idle else { return }
      self.load()
    }
  }

  func nativeAdDidRecordImpression(_ nativeAd: NativeAd) {
    if nativeAd === ad { impressionAt = Date() }
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
    hold(over: web)

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

  private func hold(over web: WKWebView) {
    guard let host = web.superview else { return }
    if catcher.superview !== host || host.subviews.last !== catcher {
      host.addSubview(catcher)
    }
    catcher.frame = web.frame
  }

  /// The card's view under `point`, when the point is on the card's visible
  /// part. The card stays where it is, so a drag that starts on it still
  /// reaches the scroll box around it.
  private func target(_ point: CGPoint, from view: UIView) -> UIView? {
    guard !hidden, let card, !card.isHidden, card.window != nil else { return nil }
    let box = riding != nil ? ride : clip
    guard !box.isHidden, box.bounds.contains(view.convert(point, to: box)) else { return nil }
    return card.hitTest(view.convert(point, to: card), with: nil)
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

/// Over the web view, as large as it. It takes no touch of its own and hands
/// a touch on the card to the card, wherever the card sits.
private final class Catcher: UIView {
  private let find: (CGPoint, UIView) -> UIView?

  init(find: @escaping (CGPoint, UIView) -> UIView?) {
    self.find = find
    super.init(frame: .zero)
    backgroundColor = .clear
  }

  required init?(coder: NSCoder) { nil }

  override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
    find(point, self)
  }
}
