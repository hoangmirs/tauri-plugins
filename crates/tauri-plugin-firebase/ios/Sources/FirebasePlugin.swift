import FirebaseAnalytics
import FirebaseCore
import FirebaseCrashlytics
import FirebaseMessaging
import FirebaseRemoteConfig
import Tauri
import UIKit
import UserNotifications

class ConfigureArgs: Decodable {
  let minimumFetchIntervalSeconds: Double
}

class SignalsArgs: Decodable {
  let signals: [String: Double?]
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

struct PushToken: Encodable {
  let token: String
}

class FirebasePlugin: Plugin {
  private var configured = false
  /// The one listener for Remote Config publishes. The SDK serves every
  /// listener from one stream, but a second would send each publish twice.
  private var updates: ConfigUpdateListenerRegistration?

  /// Runs when the plugin is created, before Rust asks `configure`. Tauri
  /// calls `load(webview:)` only once a webview exists, which is after
  /// `configure` for a plugin registered on the app's builder. A check there
  /// would read "absent" for the whole session.
  func start() {
    // `FirebaseApp.configure()` stops the app when the plist is missing, and an
    // app without its Firebase file must still start on its defaults.
    guard Bundle.main.path(forResource: "GoogleService-Info", ofType: "plist") != nil else { return }
    FirebasePlugin.live = self
    PushReceiver.install()
    if FirebaseApp.app() == nil { FirebaseApp.configure() }
    configured = true
    Messaging.messaging().delegate = self
  }

  /// The plugin `PushReceiver` and Messaging report to. The app delegate
  /// receives a silent push whether or not a webview exists.
  private static weak var live: FirebasePlugin?

  static func emit(pushData entry: [String: Any]) {
    // Tauri's listener table has no lock.
    DispatchQueue.main.async {
      live?.trigger("pushData", data: JSTypes.coerceDictionaryToJSObject(entry) ?? [:])
    }
  }

  /// The FCM token, once iOS gave Firebase an APNs token. Firebase's app
  /// delegate proxy passes that token on, so the app must leave
  /// `FirebaseAppDelegateProxyEnabled` unset. The app asks for notification
  /// permission itself, before this.
  @objc public func pushToken(_ invoke: Invoke) {
    guard configured else {
      invoke.resolve()
      return
    }
    UNUserNotificationCenter.current().getNotificationSettings { settings in
      guard settings.authorizationStatus != .denied else {
        invoke.resolve()
        return
      }
      DispatchQueue.main.async {
        // The app ships `FirebaseMessagingAutoInitEnabled` NO, so Firebase makes
        // no token before the player opts in. Asking turns it on.
        Messaging.messaging().isAutoInitEnabled = true
        UIApplication.shared.registerForRemoteNotifications()
        Self.waitForAPNs(until: Date().addingTimeInterval(10)) { ready in
          guard ready else {
            invoke.resolve()
            return
          }
          // Deprecated for `register(completion:)`, which answers the installation
          // id. The push service sends to the token.
          Messaging.messaging().token { token, _ in
            if let token { invoke.resolve(["token": token]) } else { invoke.resolve() }
          }
        }
      }
    }
  }

  /// Polls on the main thread, where Firebase's proxy writes the APNs token.
  private static func waitForAPNs(until deadline: Date, then done: @escaping (Bool) -> Void) {
    if Messaging.messaging().apnsToken != nil {
      done(true)
    } else if Date() >= deadline {
      done(false)
    } else {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { waitForAPNs(until: deadline, then: done) }
    }
  }

  @objc public func deletePushToken(_ invoke: Invoke) {
    guard configured else {
      invoke.resolve()
      return
    }
    Messaging.messaging().deleteToken { _ in
      // Left on, Firebase would make a new token at the next start.
      Messaging.messaging().isAutoInitEnabled = false
      invoke.resolve()
    }
  }

  @objc public func takePushData(_ invoke: Invoke) {
    invoke.resolve(["entries": PushQueue.take()])
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
        // The SDK calls this on its own queue, and Tauri's listener table has
        // no lock.
        DispatchQueue.main.async {
          try? self?.trigger("configUpdate", data: ConfigUpdate(keys: Array(update.updatedKeys)))
        }
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

  @objc public func setCustomSignals(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(SignalsArgs.self) else {
      invoke.resolve()
      return
    }
    let signals = args.signals.mapValues { $0.map { CustomSignalValue.double($0) } }
    Task {
      // Firebase refuses the whole set when one signal breaks a limit. The conditions then see none.
      try? await RemoteConfig.remoteConfig().setCustomSignals(signals)
      invoke.resolve()
    }
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

extension FirebasePlugin: MessagingDelegate {
  public func messaging(_ messaging: Messaging, didReceiveRegistrationToken fcmToken: String?) {
    guard let fcmToken else { return }
    DispatchQueue.main.async { [weak self] in
      try? self?.trigger("pushToken", data: PushToken(token: fcmToken))
    }
  }
}

@_cdecl("init_plugin_firebase")
func initPlugin() -> Plugin {
  let plugin = FirebasePlugin()
  plugin.start()
  return plugin
}
