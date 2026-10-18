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

  init(send: @escaping (String) -> Void) {
    self.send = send
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

    label.text = " Ad "
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
    guard let card, let slot, let web = Self.webView() else { return }
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
    if let scroller = Self.scroller(in: web.scrollView, height: CGFloat(slot.scrollHeight), skip: web.scrollView) {
      if card.superview !== scroller { scroller.addSubview(card) }
      card.frame = CGRect(x: slot.x, y: slot.y, width: w, height: h)
    } else {
      if card.superview !== web { web.addSubview(card) }
      card.frame = CGRect(x: slot.vx, y: slot.vy, width: w, height: h)
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

  private static func webView() -> WKWebView? {
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
