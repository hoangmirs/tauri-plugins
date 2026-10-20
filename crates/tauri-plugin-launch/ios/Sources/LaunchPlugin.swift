import Tauri
import UIKit
import WebKit

struct LaunchConfig: Decodable {
  let image: String?
  let background: String?
  let timeoutMs: Double?
}

/// The launch frame again, over the web view, from the moment the web view
/// loads until the page calls `ready`. iOS takes its own launch screen down
/// as soon as the app has a window, a second or so before the page paints.
class LaunchPlugin: Plugin {
  private var cover: UIView?

  override func load(webview: WKWebView) {
    let config = try? parseConfig(LaunchConfig.self)
    let frame = UIView(frame: webview.bounds)
    frame.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    frame.backgroundColor = Self.colour(config?.background ?? "#ffffff")
    frame.isUserInteractionEnabled = false
    if let image = UIImage(named: config?.image ?? "LaunchFrame") {
      let view = UIImageView(image: image)
      // The launch storyboard centres the image in the whole screen, safe area
      // or not. Centring it the same way keeps it from moving at the hand-off.
      view.center = CGPoint(x: frame.bounds.midX, y: frame.bounds.midY)
      view.autoresizingMask = [
        .flexibleLeftMargin, .flexibleRightMargin, .flexibleTopMargin, .flexibleBottomMargin,
      ]
      frame.addSubview(view)
    }
    webview.addSubview(frame)
    cover = frame
    // A page that never calls `ready` must not leave the app covered.
    let limit = min(max(config?.timeoutMs ?? 3000, 0), 10_000) / 1000
    DispatchQueue.main.asyncAfter(deadline: .now() + limit) { [weak self] in
      self?.lift()
    }
  }

  @objc public func ready(_ invoke: Invoke) {
    DispatchQueue.main.async {
      self.lift()
      invoke.resolve()
    }
  }

  /// Takes the frame away at once: the page under it already shows the same
  /// frame, so a fade would only blur the two.
  private func lift() {
    cover?.removeFromSuperview()
    cover = nil
  }

  private static func colour(_ hex: String) -> UIColor {
    let digits = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
    guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return .white }
    return UIColor(
      red: CGFloat((value >> 16) & 0xff) / 255,
      green: CGFloat((value >> 8) & 0xff) / 255,
      blue: CGFloat(value & 0xff) / 255,
      alpha: 1)
  }
}

@_cdecl("init_plugin_launch")
func initPlugin() -> Plugin {
  return LaunchPlugin()
}
