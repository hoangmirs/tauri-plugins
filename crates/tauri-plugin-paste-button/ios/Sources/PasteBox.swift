import UIKit
import UniformTypeIdentifiers
import WebKit

/// One system paste button over the box the page draws. It follows the same
/// plan as the ads plugin's native card: it rides in the page's own scroll
/// view, and a catcher over the web view hands it its taps.
@available(iOS 16.0, *)
final class PasteBox {
  private let webView: () -> WKWebView?
  private let receiver: Receiver
  private var control: UIPasteControl?
  /// The colours, corner and label the control was made with. Its
  /// configuration is fixed at creation, so a change makes a new control.
  private var look: Look?
  private var args: PlaceArgs?
  /// Set by `hide`, cleared by `place`.
  private var hidden = true

  /// The visible part of the page's scroll box, for a button that cannot ride
  /// in the scroll box itself. It keeps the button off a fixed bar.
  private let clip: UIView = {
    let view = PassThrough()
    view.clipsToBounds = true
    view.backgroundColor = .clear
    return view
  }()
  /// The same clip for a button that rides in the scroll box. It moves with
  /// each scroll, so the button scrolls with the page and is still cut at the
  /// clip. Without it the ads card slid over a top bar the page pins inside
  /// the box: WebKit draws that bar in the box too, under any view added there.
  private let ride: UIView = {
    let view = PassThrough()
    view.clipsToBounds = true
    view.backgroundColor = .clear
    return view
  }()
  private var riding: UIScrollView?
  /// Over the web view. WebKit's own hit test never returns a view it did not
  /// make, so a tap on the button would reach the page and never the button.
  /// This view hands a touch on the button's visible part to the button.
  private lazy var catcher = Catcher { [weak self] point, from in self?.target(point, from: from) }
  private var scrolled: NSKeyValueObservation?

  init(send: @escaping (String) -> Void, webView: @escaping () -> WKWebView?) {
    self.webView = webView
    receiver = Receiver(send: send)
  }

  func place(_ args: PlaceArgs) {
    self.args = args
    hidden = false
    let wanted = Look(args)
    if control == nil || look != wanted { make(wanted) }
    lay()
  }

  func hide() {
    hidden = true
    control?.isHidden = true
    clip.isHidden = true
    // Out of the page's scroll view, not only hidden: a view WebKit did not
    // make there, even hidden, kept every sideways row on the next screen from
    // taking a swipe. `place` puts it back.
    ride.removeFromSuperview()
    scrolled = nil
    riding = nil
  }

  /// A new control in `look`, in the old one's place.
  private func make(_ look: Look) {
    let config = UIPasteControl.Configuration()
    config.baseBackgroundColor = Self.hex(look.tint)
    config.baseForegroundColor = Self.hex(look.ink)
    config.cornerStyle = .fixed
    config.cornerRadius = look.radius
    config.displayMode = look.label ? .iconAndLabel : .iconOnly
    let made = UIPasteControl(configuration: config)
    // The control holds its target weakly. The receiver lives as long as this box.
    made.target = receiver
    let parent = control?.superview
    control?.removeFromSuperview()
    parent?.addSubview(made)
    control = made
    self.look = look
  }

  private func lay() {
    guard let control, let args, let web = webView() ?? Self.findWebView() else { return }

    // WebKit gives each scrolling box of the page its own scroll view. The
    // button rides in the one as tall as the page's box, so it scrolls with the
    // page and never lags.
    // A box that does not overflow has no scroll view of its own. The button
    // then sits over the page, clipped to the box's visible part.
    if let scroller = Self.scroller(in: web.scrollView, height: CGFloat(args.scrollHeight), skip: web.scrollView) {
      if ride.superview !== scroller { scroller.addSubview(ride) }
      if control.superview !== ride { ride.addSubview(control) }
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
      ride.removeFromSuperview()
      let top = CGFloat(args.clipTop ?? 0)
      let bottom = CGFloat(args.clipBottom ?? Double(web.bounds.height))
      if clip.superview !== web { web.addSubview(clip) }
      clip.frame = CGRect(x: 0, y: top, width: web.bounds.width, height: max(0, bottom - top))
      if control.superview !== clip { clip.addSubview(control) }
      control.frame = CGRect(x: args.vx, y: args.vy - top, width: args.w, height: args.h)
      clip.isHidden = hidden
    }
    control.isHidden = hidden
    hold(over: web)
  }

  /// Moves the riding clip to the part of the scroll box on screen, and the
  /// button inside it to its place in the box's content.
  private func follow() {
    guard let args, let control, let scroller = riding, let web = webView() ?? Self.findWebView() else { return }
    let shown = scroller.convert(CGPoint(x: 0, y: scroller.contentOffset.y), to: web).y
    let top = CGFloat(args.clipTop ?? Double(shown))
    let bottom = CGFloat(args.clipBottom ?? Double(shown + scroller.bounds.height))
    let y = scroller.contentOffset.y + top - shown
    let band = CGRect(x: 0, y: y, width: scroller.bounds.width, height: max(0, bottom - top))
    let place = CGRect(x: args.x, y: args.y, width: args.w, height: args.h)
    // The clip covers the button's visible part only. Over the whole visible
    // page, WebKit handed a swipe on a sideways row under it to the page.
    let seen = band.intersection(place)
    if seen.isNull || seen.isEmpty {
      ride.frame = .zero
      return
    }
    ride.frame = seen
    control.frame = CGRect(x: place.minX - seen.minX, y: place.minY - seen.minY, width: args.w, height: args.h)
  }

  private func hold(over web: WKWebView) {
    guard let host = web.superview else { return }
    if catcher.superview !== host || host.subviews.last !== catcher {
      host.addSubview(catcher)
    }
    catcher.frame = web.frame
  }

  /// The control's view under `point`, when the point is on the control's
  /// visible part.
  private func target(_ point: CGPoint, from view: UIView) -> UIView? {
    guard !hidden, let control, !control.isHidden, control.window != nil else { return nil }
    let box = riding != nil ? ride : clip
    guard !box.isHidden, box.bounds.contains(view.convert(point, to: box)) else { return nil }
    return control.hitTest(view.convert(point, to: control), with: nil)
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

  /// A `#rrggbb` or `#rrggbbaa` colour. Anything else is `nil`, and the
  /// control keeps the system's own colour.
  static func hex(_ text: String) -> UIColor? {
    var digits = text.trimmingCharacters(in: .whitespaces)
    if digits.hasPrefix("#") { digits.removeFirst() }
    guard digits.count == 6 || digits.count == 8, let value = UInt64(digits, radix: 16) else { return nil }
    let rgba = digits.count == 6 ? value << 8 | 0xff : value
    return UIColor(
      red: CGFloat((rgba >> 24) & 0xff) / 255,
      green: CGFloat((rgba >> 16) & 0xff) / 255,
      blue: CGFloat((rgba >> 8) & 0xff) / 255,
      alpha: CGFloat(rgba & 0xff) / 255)
  }
}

/// What the control is made with.
private struct Look: Equatable {
  let tint: String
  let ink: String
  let radius: CGFloat
  let label: Bool

  init(_ args: PlaceArgs) {
    tint = args.tint
    ink = args.ink
    radius = CGFloat(args.radius)
    label = args.label
  }
}

/// The control's target. It takes plain text, text and a link, and sends the
/// first item as text. An empty or unreadable paste sends "", so the page can
/// say there was nothing to paste.
@available(iOS 16.0, *)
private final class Receiver: NSObject, UIPasteConfigurationSupporting {
  private let send: (String) -> Void

  var pasteConfiguration: UIPasteConfiguration? = UIPasteConfiguration(
    acceptableTypeIdentifiers: [UTType.plainText.identifier, UTType.text.identifier, UTType.url.identifier])

  init(send: @escaping (String) -> Void) {
    self.send = send
  }

  func paste(itemProviders: [NSItemProvider]) {
    let done: (String) -> Void = { [send] text in DispatchQueue.main.async { send(text) } }
    guard let first = itemProviders.first else {
      done("")
      return
    }
    if first.canLoadObject(ofClass: String.self) {
      _ = first.loadObject(ofClass: String.self) { text, _ in done(text ?? "") }
    } else if first.canLoadObject(ofClass: URL.self) {
      _ = first.loadObject(ofClass: URL.self) { url, _ in done(url?.absoluteString ?? "") }
    } else {
      done("")
    }
  }
}

/// A clip over the page that takes no touch itself. Only the button inside it
/// does: a plain view the size of the visible page took every tap meant for
/// the page under it.
private final class PassThrough: UIView {
  override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
    let hit = super.hitTest(point, with: event)
    return hit === self ? nil : hit
  }
}

/// Over the web view, as large as it. It takes no touch of its own and hands
/// a touch on the button to the button, wherever the button sits.
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
