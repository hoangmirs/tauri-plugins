import FirebaseAnalytics
import FirebaseCore
import FirebaseRemoteConfig
import Tauri
import WebKit

class FirebasePlugin: Plugin {
  private var configured = false

  @objc public override func load(webview: WKWebView) {
    // `FirebaseApp.configure()` stops the app when the plist is missing, and an
    // app without its Firebase file must still start on its defaults.
    guard Bundle.main.path(forResource: "GoogleService-Info", ofType: "plist") != nil else { return }
    if FirebaseApp.app() == nil { FirebaseApp.configure() }
    configured = true
  }

  @objc public func mode(_ invoke: Invoke) {
    invoke.resolve(["mode": configured ? "native" : "absent"])
  }
}

@_cdecl("init_plugin_firebase")
func initPlugin() -> Plugin {
  return FirebasePlugin()
}
