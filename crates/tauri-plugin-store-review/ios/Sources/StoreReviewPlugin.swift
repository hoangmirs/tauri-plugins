import StoreKit
import Tauri
import UIKit

class StoreReviewPlugin: Plugin {
  @objc public func requestReview(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let scene =
        UIApplication.shared.connectedScenes
        .first { $0.activationState == .foregroundActive } as? UIWindowScene
      guard let scene else {
        invoke.reject("No window scene is active in the foreground")
        return
      }
      if #available(iOS 18, *) {
        AppStore.requestReview(in: scene)
      } else {
        SKStoreReviewController.requestReview(in: scene)
      }
      invoke.resolve()
    }
  }
}

@_cdecl("init_plugin_store_review")
func initPlugin() -> Plugin {
  return StoreReviewPlugin()
}
