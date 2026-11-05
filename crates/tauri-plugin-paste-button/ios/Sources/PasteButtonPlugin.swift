import Tauri
import UIKit
import WebKit

class PlaceArgs: Decodable {
  /// The box's place in the scroll box's content.
  let x: Double
  let y: Double
  let w: Double
  let h: Double
  /// The box's place on screen now.
  let vx: Double
  let vy: Double
  /// The scroll box's content height, which picks the native scroll view the
  /// button rides in.
  let scrollHeight: Double
  /// The visible part of the scroll box on screen. A bar the page pins inside
  /// the box, such as a top bar, lies outside it.
  let clipTop: Double?
  let clipBottom: Double?
  /// The button's fill, and its icon and label, as `#rrggbb` or `#rrggbbaa`.
  let tint: String
  let ink: String
  let radius: Double
  /// The icon and the system's own word, or the icon alone.
  let label: Bool
}

/// UIKit runs only on the main thread, so each method hops there first.
class PasteButtonPlugin: Plugin {
  /// The web view this plugin belongs to. With two iPad windows, a search of
  /// every window could find the other one.
  private weak var web: WKWebView?
  /// A `PasteBox`, kept untyped because the class exists only on iOS 16.
  private var made: AnyObject?

  override func load(webview: WKWebView) {
    web = webview
  }

  @available(iOS 16.0, *)
  private var box: PasteBox {
    if let box = made as? PasteBox { return box }
    let box = PasteBox(
      send: { [weak self] text in self?.trigger("paste", data: ["text": text]) },
      webView: { [weak self] in self?.web })
    made = box
    return box
  }

  @objc public func supported(_ invoke: Invoke) {
    if #available(iOS 16.0, *) {
      invoke.resolve(["supported": true])
    } else {
      invoke.resolve(["supported": false])
    }
  }

  @objc public func place(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PlaceArgs.self)
    DispatchQueue.main.async {
      if #available(iOS 16.0, *) { self.box.place(args) }
      invoke.resolve()
    }
  }

  @objc public func hide(_ invoke: Invoke) {
    DispatchQueue.main.async {
      if #available(iOS 16.0, *) { (self.made as? PasteBox)?.hide() }
      invoke.resolve()
    }
  }
}

@_cdecl("init_plugin_paste_button")
func initPlugin() -> Plugin {
  return PasteButtonPlugin()
}
