import CoreMotion
import Tauri
import UIKit

class StartArgs: Decodable {
  let kind: String
  let hz: Int
}

class StopArgs: Decodable {
  let kind: String
}

struct Kinds: Encodable {
  let kinds: [String]
}

struct MotionReading: Encodable {
  let kind = "motion"
  let g: [Double]
  let r: [Double]
  let a: [Double]
  let at: Double
}

struct HeadingReading: Encodable {
  let kind = "heading"
  let deg: Double
  let at: Double
}

/// One device-motion stream serves both kinds: gravity, rotation rate and
/// user acceleration do not depend on the attitude's reference frame, so the
/// stream takes the north-referenced frame only while `heading` runs.
class SensorsPlugin: Plugin {
  private let motion = CMMotionManager()
  // Read and changed on the main queue only.
  private var intervals: [String: TimeInterval] = [:]
  private var lastSent: [String: TimeInterval] = [:]
  private var northReferenced = false
  private var maskBeforeLock: UIInterfaceOrientationMask?

  private static var hasHeading: Bool {
    CMMotionManager.availableAttitudeReferenceFrames().contains(.xMagneticNorthZVertical)
  }

  @objc public func available(_ invoke: Invoke) {
    var kinds: [String] = []
    if motion.isDeviceMotionAvailable { kinds.append("motion") }
    if motion.isDeviceMotionAvailable && Self.hasHeading { kinds.append("heading") }
    invoke.resolve(Kinds(kinds: kinds))
  }

  @objc public func start(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(StartArgs.self)
    guard args.kind == "motion" || args.kind == "heading" else {
      invoke.reject("no sensor kind \(args.kind)")
      return
    }
    DispatchQueue.main.async {
      self.intervals[args.kind] = 1.0 / Double(min(max(args.hz, 1), 60))
      self.restart()
      invoke.resolve()
    }
  }

  @objc public func stop(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(StopArgs.self)
    DispatchQueue.main.async {
      if self.intervals.removeValue(forKey: args.kind) != nil {
        self.restart()
      }
      invoke.resolve()
    }
  }

  private func restart() {
    motion.stopDeviceMotionUpdates()
    lastSent = [:]
    northReferenced = intervals["heading"] != nil && Self.hasHeading
    guard motion.isDeviceMotionAvailable, let fastest = intervals.values.min() else { return }
    motion.deviceMotionUpdateInterval = fastest
    // Tauri's listener table has no lock, so `trigger` runs on the main queue.
    let onReading: CMDeviceMotionHandler = { [weak self] reading, _ in
      guard let self, let reading else { return }
      self.send(reading)
    }
    if northReferenced {
      motion.startDeviceMotionUpdates(using: .xMagneticNorthZVertical, to: .main, withHandler: onReading)
    } else {
      motion.startDeviceMotionUpdates(to: .main, withHandler: onReading)
    }
  }

  private func send(_ reading: CMDeviceMotion) {
    let at = reading.timestamp * 1000
    if due("motion", reading.timestamp) {
      let g = reading.gravity
      let r = reading.rotationRate
      let a = reading.userAcceleration
      try? trigger(
        "reading",
        data: MotionReading(g: [g.x, g.y, g.z], r: [r.x, r.y, r.z], a: [a.x, a.y, a.z], at: at))
    }
    if northReferenced, reading.heading >= 0,
      due("heading", reading.timestamp)
    {
      try? trigger("reading", data: HeadingReading(deg: reading.heading, at: at))
    }
  }

  /// Both kinds share the fastest kind's interval, so the slower one skips
  /// readings. The slack keeps a reading that lands a little early.
  private func due(_ kind: String, _ time: TimeInterval) -> Bool {
    guard let interval = intervals[kind] else { return false }
    if let last = lastSent[kind], time - last < interval * 0.9 { return false }
    lastSent[kind] = time
    return true
  }

  @objc public func lockRotation(_ invoke: Invoke) {
    DispatchQueue.main.async {
      defer { invoke.resolve() }
      guard let window = Self.keyWindow(), let scene = window.windowScene,
        let controller = window.rootViewController,
        let mask = Self.mask(of: scene.interfaceOrientation)
      else { return }
      if self.maskBeforeLock == nil {
        self.maskBeforeLock = controller.supportedInterfaceOrientations
      }
      Self.setSupported(controller, mask)
      if #available(iOS 16.0, *) {
        controller.setNeedsUpdateOfSupportedInterfaceOrientations()
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: mask)) { _ in }
      }
    }
  }

  @objc public func unlockRotation(_ invoke: Invoke) {
    DispatchQueue.main.async {
      defer { invoke.resolve() }
      guard let before = self.maskBeforeLock else { return }
      self.maskBeforeLock = nil
      guard let controller = Self.keyWindow()?.rootViewController else { return }
      Self.setSupported(controller, before)
      if #available(iOS 16.0, *) {
        controller.setNeedsUpdateOfSupportedInterfaceOrientations()
      }
    }
  }

  private static func keyWindow() -> UIWindow? {
    let windows = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
    return windows.first { $0.isKeyWindow } ?? windows.first
  }

  private static func mask(of orientation: UIInterfaceOrientation) -> UIInterfaceOrientationMask? {
    switch orientation {
    case .portrait: return .portrait
    case .portraitUpsideDown: return .portraitUpsideDown
    case .landscapeLeft: return .landscapeLeft
    case .landscapeRight: return .landscapeRight
    default: return nil
    }
  }

  /// UIKit asks the root controller which orientations it allows, and a plain
  /// controller has no setter for that. tao's root controller answers from a
  /// value its own `setSupportedInterfaceOrientations:` stores, the method
  /// tao's `set_valid_orientations` calls.
  private static func setSupported(_ controller: UIViewController, _ mask: UIInterfaceOrientationMask) {
    let selector = NSSelectorFromString("setSupportedInterfaceOrientations:")
    guard controller.responds(to: selector) else { return }
    typealias Setter = @convention(c) (AnyObject, Selector, UInt) -> Void
    unsafeBitCast(controller.method(for: selector), to: Setter.self)(controller, selector, mask.rawValue)
  }

  deinit {
    motion.stopDeviceMotionUpdates()
  }
}

@_cdecl("init_plugin_sensors")
func initPlugin() -> Plugin {
  return SensorsPlugin()
}
