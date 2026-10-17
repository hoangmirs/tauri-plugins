import FirebaseAnalytics
import FirebaseCore
import FirebaseRemoteConfig
import Tauri

class ConfigureArgs: Decodable {
  let minimumFetchIntervalSeconds: Double
}

class CollectionArgs: Decodable {
  let enabled: Bool
}

class FirebasePlugin: Plugin {
  private var configured = false

  /// Runs when the plugin is created, before Rust asks `configure`. Tauri
  /// calls `load(webview:)` only once a webview exists, which is after
  /// `configure` for a plugin registered on the app's builder. A check there
  /// would read "absent" for the whole session.
  func start() {
    // `FirebaseApp.configure()` stops the app when the plist is missing, and an
    // app without its Firebase file must still start on its defaults.
    guard Bundle.main.path(forResource: "GoogleService-Info", ofType: "plist") != nil else { return }
    if FirebaseApp.app() == nil { FirebaseApp.configure() }
    configured = true
  }

  @objc public func configure(_ invoke: Invoke) {
    guard configured else {
      invoke.resolve(["mode": "absent"])
      return
    }
    if let args = try? invoke.parseArgs(ConfigureArgs.self) {
      let settings = RemoteConfigSettings()
      settings.minimumFetchInterval = args.minimumFetchIntervalSeconds
      RemoteConfig.remoteConfig().configSettings = settings
    }
    invoke.resolve(["mode": "native"])
  }

  @objc public func setDefaults(_ invoke: Invoke) {
    let given = (try? invoke.getArgs())?.getObject("defaults") ?? [:]
    var defaults: [String: NSObject] = [:]
    for (key, value) in given {
      if let text = value as? String { defaults[key] = text as NSString }
    }
    RemoteConfig.remoteConfig().setDefaults(defaults)
    invoke.resolve()
  }

  @objc public func fetch(_ invoke: Invoke) {
    RemoteConfig.remoteConfig().fetch { status, _ in
      switch status {
      case .success: invoke.resolve(["result": "fetched"])
      case .throttled: invoke.resolve(["result": "throttled"])
      default: invoke.resolve(["result": "failed"])
      }
    }
  }

  @objc public func activate(_ invoke: Invoke) {
    RemoteConfig.remoteConfig().activate { changed, error in
      invoke.resolve(["changed": error == nil && changed])
    }
  }

  @objc public func getAll(_ invoke: Invoke) {
    let config = RemoteConfig.remoteConfig()
    var keys = Set<String>()
    for source in [RemoteConfigSource.remote, .default, .static] {
      keys.formUnion(config.allKeys(from: source))
    }
    var values: [String: Any] = [:]
    for key in keys {
      let value = config.configValue(forKey: key)
      values[key] = ["value": value.stringValue, "source": Self.name(of: value.source)]
    }
    invoke.resolve(["values": values])
  }

  @objc public func logEvent(_ invoke: Invoke) {
    let args = try? invoke.getArgs()
    guard let name = args?.getString("name") else {
      invoke.resolve()
      return
    }
    var params: [String: Any] = [:]
    for (key, value) in args?.getObject("params") ?? [:] {
      if let text = value as? String {
        params[key] = text
      } else if let number = value as? NSNumber {
        params[key] = number
      }
    }
    Analytics.logEvent(name, parameters: params.isEmpty ? nil : params)
    invoke.resolve()
  }

  @objc public func setCollection(_ invoke: Invoke) {
    if let args = try? invoke.parseArgs(CollectionArgs.self) {
      Analytics.setAnalyticsCollectionEnabled(args.enabled)
    }
    invoke.resolve()
  }

  private static func name(of source: RemoteConfigSource) -> String {
    switch source {
    case .remote: return "remote"
    case .default: return "default"
    default: return "static"
    }
  }
}

@_cdecl("init_plugin_firebase")
func initPlugin() -> Plugin {
  let plugin = FirebasePlugin()
  plugin.start()
  return plugin
}
