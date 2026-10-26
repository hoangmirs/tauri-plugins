import Tauri
import UIKit

class SetArgs: Decodable {
  let name: String?
}

/// UIKit reads and changes the icon only on the main thread. A UIKit call
/// off it is what crashed Recess 1.2.0, so each method hops there first.
class HomeIconPlugin: Plugin {
  @objc public func supported(_ invoke: Invoke) {
    DispatchQueue.main.async {
      invoke.resolve(["supported": UIApplication.shared.supportsAlternateIcons])
    }
  }

  /// An empty answer for the primary icon: a nil inside the answer would
  /// not turn into JSON.
  @objc public func current(_ invoke: Invoke) {
    DispatchQueue.main.async {
      if let name = UIApplication.shared.alternateIconName {
        invoke.resolve(["name": name])
      } else {
        invoke.resolve([String: Any]())
      }
    }
  }

  /// `nil` asks for the primary icon. iOS shows its own alert after a change.
  @objc public func set(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SetArgs.self)
    DispatchQueue.main.async {
      UIApplication.shared.setAlternateIconName(args.name) { error in
        if let error = error {
          invoke.reject(error.localizedDescription)
        } else {
          invoke.resolve()
        }
      }
    }
  }
}

@_cdecl("init_plugin_home_icon")
func initPlugin() -> Plugin {
  return HomeIconPlugin()
}
