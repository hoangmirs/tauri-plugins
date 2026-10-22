import Tauri
import UIKit

class SetArgs: Decodable {
  let on: Bool
}

class KeepAwakePlugin: Plugin {
  /// UIKit reads the switch only on the main thread. It holds while the app
  /// is in front, and iOS lets the screen sleep again once the app leaves.
  @objc public func set(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SetArgs.self)
    DispatchQueue.main.async {
      UIApplication.shared.isIdleTimerDisabled = args.on
      invoke.resolve()
    }
  }
}

@_cdecl("init_plugin_keep_awake")
func initPlugin() -> Plugin {
  return KeepAwakePlugin()
}
