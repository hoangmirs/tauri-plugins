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

  /// Runs before `FirebaseApp.configure()`, so Firebase's app delegate proxy
  /// finds this method on the delegate's class and calls it with a handler of
  /// its own. When Firebase proxied the delegate first, the delegate's class
  /// is Firebase's subclass, and this method wraps Firebase's own. Either way
  /// the system's handler is called once.
  static func install() {
    guard Thread.isMainThread else {
      DispatchQueue.main.async { install() }
      return
    }
    let app = UIApplication.shared
    guard !installed, let delegate = app.delegate, let cls = object_getClass(delegate) else { return }
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
      // UIKit reads which methods a delegate has only when the delegate is set.
      app.delegate = nil
      app.delegate = delegate
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
    guard let group else {
      done(.noData)
      return
    }
    let center = UNUserNotificationCenter.current()
    center.getDeliveredNotifications { delivered in
      let ids = delivered.filter { $0.request.content.threadIdentifier == group }.map { $0.request.identifier }
      center.removeDeliveredNotifications(withIdentifiers: ids)
      done(ids.isEmpty ? .noData : .newData)
    }
  }
}
