import FirebaseMessaging
import UIKit
import UserNotifications

/// The silent pushes the app's code has not read yet. They live in the app's
/// own defaults, so an app the system stopped still finds them at its next
/// start.
enum PushQueue {
  static let max = 20
  static let maxAgeMillis: Int64 = 24 * 60 * 60 * 1000
  private static let key = "dev.hoangmirs.firebase.push.queue"
  private static let lock = NSLock()

  static func nowMillis() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

  static func add(_ entry: [String: Any]) {
    lock.lock()
    defer { lock.unlock() }
    write(kept(read() + [entry]))
  }

  /// The queue, oldest first. It is empty afterwards.
  static func take() -> [[String: Any]] {
    lock.lock()
    defer { lock.unlock() }
    let entries = kept(read())
    write([])
    return entries
  }

  private static func kept(_ entries: [[String: Any]]) -> [[String: Any]] {
    let now = nowMillis()
    let fresh = entries.filter { now - (($0["at"] as? NSNumber)?.int64Value ?? 0) < maxAgeMillis }
    return Array(fresh.suffix(max))
  }

  private static func read() -> [[String: Any]] {
    guard let data = UserDefaults.standard.data(forKey: key),
      let entries = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
    else { return [] }
    return entries
  }

  private static func write(_ entries: [[String: Any]]) {
    guard let data = try? JSONSerialization.data(withJSONObject: entries) else { return }
    UserDefaults.standard.set(data, forKey: key)
  }
}

/// Gives the app delegate `application(_:didReceiveRemoteNotification:fetchCompletionHandler:)`,
/// the only way iOS hands a silent push to an app. The app's notification
/// plugin owns `UNUserNotificationCenter`'s delegate, so this never touches it.
enum PushReceiver {
  private typealias Done = @convention(block) (UIBackgroundFetchResult) -> Void
  private typealias Original = @convention(c) (AnyObject, Selector, UIApplication, NSDictionary, Done) -> Void
  private typealias Receive = @convention(block) (AnyObject, UIApplication, NSDictionary, @escaping Done) -> Void

  private static let selector = #selector(
    UIApplicationDelegate.application(_:didReceiveRemoteNotification:fetchCompletionHandler:))
  private static var installed = false

  /// Runs once the app has a delegate (see `AfterLaunch`). When Firebase's
  /// app delegate proxy came later, it finds this method on the delegate's
  /// class and calls it with a handler of its own. When Firebase proxied the
  /// delegate first, the delegate's class is Firebase's subclass, and this
  /// method wraps Firebase's own. Either way the system's handler is called once.
  static func install() {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { install() }
      return
    }
    guard !installed, let app = AfterLaunch.application(), let delegate = app.delegate,
      let cls = object_getClass(delegate)
    else { return }
    installed = true
    let original = class_getInstanceMethod(cls, selector).map {
      unsafeBitCast(method_getImplementation($0), to: Original.self)
    }
    let receive: Receive = { target, application, info, done in
      handle(info as? [AnyHashable: Any] ?? [:]) { result in
        // An original implementation owns the handler and calls it itself.
        if let original {
          original(target, selector, application, info, done)
        } else {
          done(result)
        }
      }
    }
    let imp = imp_implementationWithBlock(receive)
    if class_addMethod(cls, selector, imp, "v@:@@@?") {
      AfterLaunch.setAgain(delegate, on: app)
    } else if let method = class_getInstanceMethod(cls, selector) {
      method_setImplementation(method, imp)
    }
  }

  private static func handle(
    _ info: [AnyHashable: Any], then done: @escaping (UIBackgroundFetchResult) -> Void
  ) {
    guard let action = info["action"] as? String else {
      done(.noData)
      return
    }
    let data =
      (info["data"] as? String).flatMap {
        try? JSONSerialization.jsonObject(with: Data($0.utf8), options: .fragmentsAllowed)
      } ?? NSNull()
    if action == "clear" {
      clear(group: (data as? [String: Any])?["group"] as? String, then: done)
      return
    }
    let entry: [String: Any] = ["action": action, "data": data, "at": PushQueue.nowMillis()]
    PushQueue.add(entry)
    FirebasePlugin.emit(pushData: entry)
    done(.newData)
  }

  private static func clear(group: String?, then done: @escaping (UIBackgroundFetchResult) -> Void) {
    // An empty group would match every notification without a thread id.
    guard let group, !group.isEmpty else {
      done(.noData)
      return
    }
    let center = UNUserNotificationCenter.current()
    center.getDeliveredNotifications { delivered in
      let ids = delivered.filter { $0.request.content.threadIdentifier == group }.map { $0.request.identifier }
      center.removeDeliveredNotifications(withIdentifiers: ids)
      // This callback runs on a background queue, and the next handler in the
      // chain expects the main thread.
      DispatchQueue.main.async { done(ids.isEmpty ? .noData : .newData) }
    }
  }
}

/// Runs code once the app has its delegate. A Tauri app creates its plugins
/// in `Builder::build()`, before `UIApplicationMain` creates the application
/// and its delegate, so a method added to the delegate at plugin start finds
/// no delegate at all.
enum AfterLaunch {
  private static var observer: NSObjectProtocol?

  /// Runs `then` on the main thread: at once when the app already has its
  /// delegate, else when UIKit posts `didFinishLaunchingNotification`.
  static func run(_ then: @escaping () -> Void) {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { run(then) }
      return
    }
    if application()?.delegate != nil {
      then()
      return
    }
    guard observer == nil else { return }
    observer = NotificationCenter.default.addObserver(
      forName: UIApplication.didFinishLaunchingNotification, object: nil, queue: .main
    ) { _ in
      if let observer { NotificationCenter.default.removeObserver(observer) }
      observer = nil
      then()
    }
  }

  /// Sets the delegate again, because UIKit reads which methods a delegate has
  /// only when the delegate is set. The application holds the delegate that
  /// `UIApplicationMain` made only until the delegate changes, so without
  /// `kept` the first `nil` frees it and the app crashes.
  static func setAgain(_ delegate: UIApplicationDelegate, on app: UIApplication) {
    kept = delegate
    app.delegate = nil
    app.delegate = delegate
  }

  private static var kept: UIApplicationDelegate?

  /// The application, or nil before `UIApplicationMain` made it. Swift declares
  /// `UIApplication.shared` never nil, so it cannot be read that early.
  static func application() -> UIApplication? {
    let shared = NSSelectorFromString("sharedApplication")
    return (UIApplication.self as AnyObject).perform(shared)?.takeUnretainedValue() as? UIApplication
  }
}

/// Gives the app delegate the two methods through which iOS answers
/// `registerForRemoteNotifications()`. The APNs token goes to Firebase
/// Messaging, which needs it before it makes a token. Firebase's app delegate
/// proxy would pass the token itself, but it tries once, in
/// `FirebaseApp.configure()`, and in a Tauri app that runs before the app has
/// a delegate. When the proxy did start, Messaging gets the token twice, which
/// does no harm.
enum APNsReceiver {
  private typealias Original = @convention(c) (AnyObject, Selector, UIApplication, AnyObject) -> Void
  private typealias Receive = @convention(block) (AnyObject, UIApplication, AnyObject) -> Void

  private static let registered = #selector(
    UIApplicationDelegate.application(_:didRegisterForRemoteNotificationsWithDeviceToken:))
  private static let failed = #selector(
    UIApplicationDelegate.application(_:didFailToRegisterForRemoteNotificationsWithError:))
  private static var installed = false

  /// Whether iOS refused the last registration. Read and written on the main thread.
  static var refused = false

  static func install() {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { install() }
      return
    }
    guard !installed, let app = AfterLaunch.application(), let delegate = app.delegate,
      let cls = object_getClass(delegate)
    else { return }
    installed = true
    let addedToken = hook(cls, registered) { token in
      guard let token = token as? Data else { return }
      refused = false
      Messaging.messaging().apnsToken = token
    }
    let addedFailure = hook(cls, failed) { error in
      refused = true
      NSLog("tauri-plugin-firebase: iOS refused to register for remote notifications: %@", "\(error)")
    }
    if addedToken || addedFailure { AfterLaunch.setAgain(delegate, on: app) }
  }

  /// Adds `selector` to `cls`, or wraps the method it has. Answers whether it added one.
  private static func hook(_ cls: AnyClass, _ selector: Selector, _ body: @escaping (AnyObject) -> Void) -> Bool {
    let original = class_getInstanceMethod(cls, selector).map {
      unsafeBitCast(method_getImplementation($0), to: Original.self)
    }
    let receive: Receive = { target, application, value in
      original?(target, selector, application, value)
      body(value)
    }
    let imp = imp_implementationWithBlock(receive)
    if class_addMethod(cls, selector, imp, "v@:@@") { return true }
    if let method = class_getInstanceMethod(cls, selector) { method_setImplementation(method, imp) }
    return false
  }
}
