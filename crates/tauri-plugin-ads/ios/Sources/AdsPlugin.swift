import GoogleMobileAds
import Tauri
import UIKit
import UserMessagingPlatform
import WebKit

class SettingsArgs: Decodable {
  let childDirected: Bool
  let maxRating: String
}

class UnitArgs: Decodable {
  let unitId: String
}

class AdsPlugin: Plugin, FullScreenContentDelegate {
  /// One rewarded ad, loaded before the player asks for it.
  private var rewarded: RewardedAd?
  /// The `showRewarded` call waiting for the ad to close.
  private var showing: Invoke?
  private var earned = false
  /// The web view this plugin belongs to. With two iPad windows, a search of
  /// every window could find the other one.
  private weak var web: WKWebView?
  /// From `init`. A child cannot give consent, so the consent form is told.
  private var childDirected = true
  private lazy var native = NativeCard(
    send: { [weak self] event in self?.trigger("native", data: ["event": event]) },
    webView: { [weak self] in self?.web })

  override func load(webview: WKWebView) {
    web = webview
  }

  @objc public func `init`(_ invoke: Invoke) {
    let args = try? invoke.parseArgs(SettingsArgs.self)
    childDirected = args?.childDirected ?? true
    let config = MobileAds.shared.requestConfiguration
    config.tagForChildDirectedTreatment = NSNumber(value: args?.childDirected ?? true)
    config.tagForUnderAgeOfConsent = NSNumber(value: args?.childDirected ?? true)
    config.maxAdContentRating = Self.rating(args?.maxRating ?? "G")
    MobileAds.shared.start { _ in
      NSLog("[ads] started")
    }
    invoke.resolve()
  }

  @objc public func showNative(_ invoke: Invoke) {
    guard let slot = try? invoke.parseArgs(SlotArgs.self) else {
      invoke.resolve()
      return
    }
    DispatchQueue.main.async {
      self.native.show(slot)
      invoke.resolve()
    }
  }

  @objc public func hideNative(_ invoke: Invoke) {
    DispatchQueue.main.async {
      self.native.hide()
      invoke.resolve()
    }
  }

  /// Google's form shows only where the law asks for it. The answer says
  /// whether ads may load.
  @objc public func consent(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let parameters = RequestParameters()
      parameters.isTaggedForUnderAgeOfConsent = self.childDirected
      ConsentInformation.shared.requestConsentInfoUpdate(with: parameters) { error in
        guard error == nil else {
          invoke.resolve(["result": "failed"])
          return
        }
        ConsentForm.loadAndPresentIfRequired(from: Self.topController()) { error in
          let info = ConsentInformation.shared
          let result: String
          if error != nil {
            result = "failed"
          } else if info.consentStatus == .notRequired {
            result = "not_required"
          } else {
            result = info.canRequestAds ? "obtained" : "failed"
          }
          invoke.resolve(["result": result])
        }
      }
    }
  }

  @objc public func loadRewarded(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(UnitArgs.self) else {
      invoke.resolve(["result": "failed"])
      return
    }
    DispatchQueue.main.async {
      if self.rewarded != nil {
        invoke.resolve(["result": "ready"])
        return
      }
      RewardedAd.load(with: args.unitId, request: Request()) { ad, error in
        guard let ad, error == nil else {
          invoke.resolve(["result": "failed"])
          return
        }
        ad.fullScreenContentDelegate = self
        self.rewarded = ad
        invoke.resolve(["result": "ready"])
      }
    }
  }

  @objc public func showRewarded(_ invoke: Invoke) {
    DispatchQueue.main.async {
      guard let ad = self.rewarded, self.showing == nil, let root = Self.topController() else {
        invoke.resolve(["result": "failed"])
        return
      }
      self.rewarded = nil
      self.showing = invoke
      self.earned = false
      ad.present(from: root) { self.earned = true }
    }
  }

  func adDidDismissFullScreenContent(_ ad: FullScreenPresentingAd) {
    NSLog("[ads] rewarded %@", earned ? "earned" : "closed")
    showing?.resolve(["result": earned ? "earned" : "closed"])
    showing = nil
  }

  func ad(_ ad: FullScreenPresentingAd, didFailToPresentFullScreenContentWithError error: Error) {
    showing?.resolve(["result": "failed"])
    showing = nil
  }

  /// The controller on top of the app's window, which a full-screen ad
  /// presents from.
  static func topController() -> UIViewController? {
    let windows = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
    var top = (windows.first { $0.isKeyWindow } ?? windows.first)?.rootViewController
    while let next = top?.presentedViewController { top = next }
    return top
  }

  private static func rating(_ text: String) -> GADMaxAdContentRating {
    switch text {
    case "PG": return .parentalGuidance
    case "T": return .teen
    case "MA": return .matureAudience
    default: return .general
    }
  }
}

@_cdecl("init_plugin_ads")
func initPlugin() -> Plugin {
  return AdsPlugin()
}
