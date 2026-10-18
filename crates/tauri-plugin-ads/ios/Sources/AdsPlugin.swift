import GoogleMobileAds
import Tauri
import UIKit
import WebKit

class SettingsArgs: Decodable {
  let childDirected: Bool
  let maxRating: String
}

class AdsPlugin: Plugin {
  @objc public func `init`(_ invoke: Invoke) {
    let args = try? invoke.parseArgs(SettingsArgs.self)
    let config = MobileAds.shared.requestConfiguration
    config.tagForChildDirectedTreatment = NSNumber(value: args?.childDirected ?? true)
    config.maxAdContentRating = Self.rating(args?.maxRating ?? "G")
    MobileAds.shared.start { _ in
      NSLog("[ads] started")
    }
    invoke.resolve()
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
