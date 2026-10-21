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

class MutedArgs: Decodable {
  let muted: Bool
}

/// Google tells apps not to show an ad loaded more than an hour ago.
let staleAfter: TimeInterval = 60 * 60

/// What Google says one ad earned, as the `paid` event's data.
func paidData(_ format: String, _ value: AdValue) -> JSObject {
  let precision: String
  switch value.precision {
  case .estimated: precision = "estimated"
  case .publisherProvided: precision = "publisher"
  case .precise: precision = "precise"
  default: precision = "unknown"
  }
  return [
    "format": format,
    "micros": value.value.multiplying(byPowerOf10: 6).intValue,
    "currency": value.currencyCode,
    "precision": precision,
  ]
}

class AdsPlugin: Plugin, FullScreenContentDelegate {
  /// One rewarded ad, loaded before the player asks for it, with its unit
  /// and the time it loaded.
  private var rewarded: RewardedAd?
  private var rewardedUnit: String?
  private var rewardedAt = Date.distantPast
  /// Set once the SDK starts. It starts after consent, as Google asks.
  private var started = false
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
    webView: { [weak self] in self?.web },
    paid: { [weak self] value in self?.trigger("paid", data: paidData("native", value)) })

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
    invoke.resolve()
  }

  private func start() {
    guard !started else { return }
    started = true
    MobileAds.shared.start { _ in
      NSLog("[ads] started")
    }
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
          // Offline, say. A consent from an earlier session still counts.
          let ok = ConsentInformation.shared.canRequestAds
          if ok { self.start() }
          invoke.resolve(["result": ok ? "obtained" : "failed"])
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
          if result != "failed" { self.start() }
          invoke.resolve(["result": result])
        }
      }
    }
  }

  @objc public func privacyOptions(_ invoke: Invoke) {
    let required = ConsentInformation.shared.privacyOptionsRequirementStatus == .required
    invoke.resolve(["result": required ? "required" : "not_required"])
  }

  @objc public func showPrivacyOptions(_ invoke: Invoke) {
    DispatchQueue.main.async {
      ConsentForm.presentPrivacyOptionsForm(from: Self.topController()) { error in
        invoke.resolve(["result": error == nil ? "shown" : "failed"])
      }
    }
  }

  @objc public func setMuted(_ invoke: Invoke) {
    let muted = (try? invoke.parseArgs(MutedArgs.self))?.muted ?? false
    DispatchQueue.main.async {
      MobileAds.shared.isApplicationMuted = muted
      invoke.resolve()
    }
  }

  @objc public func loadRewarded(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(UnitArgs.self) else {
      invoke.resolve(["result": "failed"])
      return
    }
    DispatchQueue.main.async {
      self.loadRewarded(args.unitId) { invoke.resolve(["result": $0 ? "ready" : "failed"]) }
    }
  }

  /// Keeps an ad that is fresh and from `unit`, and loads one otherwise.
  private func loadRewarded(_ unit: String, done: @escaping (Bool) -> Void) {
    let fresh = Date().timeIntervalSince(rewardedAt) < staleAfter
    if rewarded != nil, rewardedUnit == unit, fresh {
      done(true)
      return
    }
    rewarded = nil
    RewardedAd.load(with: unit, request: Request()) { ad, error in
      guard let ad, error == nil else {
        done(false)
        return
      }
      ad.fullScreenContentDelegate = self
      ad.paidEventHandler = { [weak self] value in
        self?.trigger("paid", data: paidData("rewarded", value))
      }
      self.rewarded = ad
      self.rewardedUnit = unit
      self.rewardedAt = Date()
      done(true)
    }
  }

  @objc public func showRewarded(_ invoke: Invoke) {
    DispatchQueue.main.async {
      guard let ad = self.rewarded, self.showing == nil, let root = Self.topController() else {
        invoke.resolve(["result": "failed"])
        return
      }
      if Date().timeIntervalSince(self.rewardedAt) >= staleAfter, let unit = self.rewardedUnit {
        self.rewarded = nil
        invoke.resolve(["result": "failed"])
        self.loadRewarded(unit) { _ in }
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
