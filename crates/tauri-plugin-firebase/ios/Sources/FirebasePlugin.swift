import FirebaseAnalytics
import FirebaseCore
import FirebaseCrashlytics
import FirebaseRemoteConfig
import Tauri

class ConfigureArgs: Decodable {
  let minimumFetchIntervalSeconds: Double
}

class CollectionArgs: Decodable {
  let enabled: Bool
}

class CrashKeysArgs: Decodable {
  let keys: [String: String]
}

class CrashLogArgs: Decodable {
  let line: String
}

class CrashFrameArgs: Decodable {
  let symbol: String
  let file: String
  let line: Int
}

class CrashRecordArgs: Decodable {
  let name: String
  let message: String
  let frames: [CrashFrameArgs]
}

struct ConfigUpdate: Encodable {
  let keys: [String]
}

class FirebasePlugin: Plugin {
  private var configured = false
  /// The one listener for Remote Config publishes. A second would hold a
  /// second connection, and fetch twice on every publish.
  private var updates: ConfigUpdateListenerRegistration?

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

  /// Each publish reaches the webview as `configUpdate` once the SDK fetched
  /// it. The values are not active: the app activates them, as after `fetch`.
  @objc public func listen(_ invoke: Invoke) {
    if configured && updates == nil {
      updates = RemoteConfig.remoteConfig().addOnConfigUpdateListener { [weak self] update, error in
        guard error == nil, let update else { return }
        try? self?.trigger("configUpdate", data: ConfigUpdate(keys: Array(update.updatedKeys)))
      }
    }
    invoke.resolve()
  }

  @objc public func unlisten(_ invoke: Invoke) {
    updates?.remove()
    updates = nil
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

  // Rust calls the four crash methods from its panic hook, which can run on the
  // main thread and blocks until the invoke resolves. Each one calls
  // Crashlytics and resolves on the calling thread, so the hook cannot wait on
  // itself.
  @objc public func crashSetCollection(_ invoke: Invoke) {
    if configured, let args = try? invoke.parseArgs(CollectionArgs.self) {
      Crashlytics.crashlytics().setCrashlyticsCollectionEnabled(args.enabled)
    }
    invoke.resolve()
  }

  @objc public func crashSetKeys(_ invoke: Invoke) {
    if configured, let args = try? invoke.parseArgs(CrashKeysArgs.self) {
      let crashlytics = Crashlytics.crashlytics()
      for (key, value) in args.keys {
        crashlytics.setCustomValue(value, forKey: key)
      }
    }
    invoke.resolve()
  }

  @objc public func crashLog(_ invoke: Invoke) {
    if configured, let args = try? invoke.parseArgs(CrashLogArgs.self) {
      Crashlytics.crashlytics().log(args.line)
    }
    invoke.resolve()
  }

  @objc public func crashRecord(_ invoke: Invoke) {
    if configured, let args = try? invoke.parseArgs(CrashRecordArgs.self) {
      let exception = ExceptionModel(name: args.name, reason: args.message)
      exception.stackTrace = args.frames.map {
        StackFrame(symbol: $0.symbol, file: $0.file, line: $0.line)
      }
      Crashlytics.crashlytics().record(exceptionModel: exception)
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
