import Foundation
import Security
import Tauri

struct KeyArgs: Decodable {
  let key: String
}

struct SetArgs: Decodable {
  let key: String
  let value: String
}

struct Kept: Encodable {
  let value: String?
}

/// One Keychain item a key, this device only and never synchronized, so it
/// stays when the app is deleted and never leaves the phone.
class KeepPlugin: Plugin {
  private let service = (Bundle.main.bundleIdentifier ?? "app") + ".keep"

  private func query(_ key: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: key,
      kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
    ]
  }

  @objc public func get(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(KeyArgs.self) else {
      invoke.reject("get needs a key")
      return
    }
    var ask = query(args.key)
    ask[kSecReturnData as String] = true
    ask[kSecMatchLimit as String] = kSecMatchLimitOne
    var found: CFTypeRef?
    let status = SecItemCopyMatching(ask as CFDictionary, &found)
    if status == errSecItemNotFound {
      invoke.resolve(Kept(value: nil))
    } else if status == errSecSuccess, let data = found as? Data {
      invoke.resolve(Kept(value: String(data: data, encoding: .utf8)))
    } else {
      invoke.reject("The Keychain refused the read (\(status))")
    }
  }

  @objc public func set(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(SetArgs.self) else {
      invoke.reject("set needs a key and a value")
      return
    }
    let data = Data(args.value.utf8)
    let fields: [String: Any] = [
      kSecValueData as String: data,
      kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
    ]
    var status = SecItemUpdate(query(args.key) as CFDictionary, fields as CFDictionary)
    if status == errSecItemNotFound {
      let add = query(args.key).merging(fields) { _, new in new }
      status = SecItemAdd(add as CFDictionary, nil)
    }
    if status == errSecSuccess {
      invoke.resolve()
    } else {
      invoke.reject("The Keychain refused the write (\(status))")
    }
  }

  @objc public func remove(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(KeyArgs.self) else {
      invoke.reject("remove needs a key")
      return
    }
    let status = SecItemDelete(query(args.key) as CFDictionary)
    if status == errSecSuccess || status == errSecItemNotFound {
      invoke.resolve()
    } else {
      invoke.reject("The Keychain refused the removal (\(status))")
    }
  }
}

@_cdecl("init_plugin_keep")
func initPlugin() -> Plugin {
  return KeepPlugin()
}
