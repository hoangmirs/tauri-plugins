import Foundation
import Security

// The Keychain work for both platforms: `VaultPlugin.swift` calls it on iOS,
// and `macos/Bridge.swift` on the Mac. Nothing here imports Tauri, so the
// Mac's build compiles this file as it is.

struct KeyArgs: Decodable {
  let key: String
}

struct SaveArgs: Decodable {
  let key: String
  let text: String
}

struct SignArgs: Decodable {
  let msg: String
}

struct VerifyArgs: Decodable {
  let msg: String
  let sig: String
}

struct Loaded: Encodable {
  let text: String?
  let unreadable: Bool?
}

struct Signed: Encodable {
  let sig: String
}

struct Verified: Encodable {
  let ok: Bool
}

/// Answers `{"level": null}` when there is no key, the same answer Android
/// gives. The encoder Swift writes for an optional would leave the field out.
struct KeyLevel: Encodable {
  let level: String?

  enum CodingKeys: String, CodingKey {
    case level
  }

  func encode(to encoder: Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encode(level, forKey: .level)
  }
}

struct VaultError: Error {
  let message: String
}

/// A value is one Keychain item a key. A device key signs the values: a Secure
/// Enclave key when the device has one, else a software key in the Keychain.
/// Both stay on this device and are never synchronized.
final class Vault {
  private let bundleId = Bundle.main.bundleIdentifier ?? "app"
  private var service: String { bundleId + ".vault" }
  private var keyTag: Data { Data((bundleId + ".vault.key").utf8) }
  private let accessible = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
  private let algorithm = SecKeyAlgorithm.ecdsaSignatureMessageX962SHA256

  // Two calls at once would each find no key and make two keys under one tag.
  private let makeKeyLock = NSLock()

  /// The Mac's own keychain is a file the user can open and change. The data
  /// protection keychain is the iPhone's kind: only this app reaches its items,
  /// and Keychain Access never shows a key there. iOS has no other kind.
  private func scoped(_ query: [String: Any]) -> [String: Any] {
    #if os(macOS)
      return query.merging([kSecUseDataProtectionKeychain as String: true]) { _, new in new }
    #else
      return query
    #endif
  }

  private func valueQuery(_ key: String) -> [String: Any] {
    scoped([
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: key,
      kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
    ])
  }

  private var keyQuery: [String: Any] {
    scoped([
      kSecClass as String: kSecClassKey,
      kSecAttrApplicationTag as String: keyTag,
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
    ])
  }

  private func findKey() throws -> SecKey? {
    var ask = keyQuery
    ask[kSecReturnRef as String] = true
    ask[kSecMatchLimit as String] = kSecMatchLimitOne
    var found: CFTypeRef?
    let status = SecItemCopyMatching(ask as CFDictionary, &found)
    if status == errSecItemNotFound {
      return nil
    }
    guard status == errSecSuccess else {
      throw VaultError(message: "The Keychain refused the key read (\(status))")
    }
    if let found, CFGetTypeID(found) == SecKeyGetTypeID() {
      let key = unsafeDowncast(found, to: SecKey.self)
      if SecKeyIsAlgorithmSupported(key, .sign, algorithm), SecKeyCopyPublicKey(key) != nil {
        return key
      }
    }
    // A key that cannot sign counts as no key, so the next read makes a new
    // one. Rejecting would leave the app in a mirror session on every start.
    let deleted = SecItemDelete(keyQuery as CFDictionary)
    guard deleted == errSecSuccess || deleted == errSecItemNotFound else {
      throw VaultError(message: "The Keychain refused to delete an unusable key (\(deleted))")
    }
    return nil
  }

  private func level(of key: SecKey) -> String {
    let attributes = SecKeyCopyAttributes(key) as? [String: Any]
    let tokenId = attributes?[kSecAttrTokenID as String] as? String
    return tokenId == (kSecAttrTokenIDSecureEnclave as String) ? "enclave" : "software"
  }

  private func createKey(inEnclave: Bool) -> SecKey? {
    var privateAttributes: [String: Any] = [
      kSecAttrIsPermanent as String: true,
      kSecAttrApplicationTag as String: keyTag,
    ]
    var attributes: [String: Any] = [
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeySizeInBits as String: 256,
    ]
    if inEnclave {
      guard
        let access = SecAccessControlCreateWithFlags(nil, accessible, .privateKeyUsage, nil)
      else {
        return nil
      }
      privateAttributes[kSecAttrAccessControl as String] = access
      attributes[kSecAttrTokenID as String] = kSecAttrTokenIDSecureEnclave
    } else {
      privateAttributes[kSecAttrAccessible as String] = accessible
    }
    attributes[kSecPrivateKeyAttrs as String] = privateAttributes
    var error: Unmanaged<CFError>?
    let key = SecKeyCreateRandomKey(scoped(attributes) as CFDictionary, &error)
    error?.release()
    return key
  }

  func load(_ key: String) throws -> Loaded {
    var ask = valueQuery(key)
    ask[kSecReturnData as String] = true
    ask[kSecMatchLimit as String] = kSecMatchLimitOne
    var found: CFTypeRef?
    let status = SecItemCopyMatching(ask as CFDictionary, &found)
    if status == errSecItemNotFound {
      return Loaded(text: nil, unreadable: nil)
    }
    guard status == errSecSuccess, let data = found as? Data else {
      throw VaultError(message: "The Keychain refused the read (\(status))")
    }
    if let text = String(data: data, encoding: .utf8) {
      return Loaded(text: text, unreadable: nil)
    }
    return Loaded(text: nil, unreadable: true)
  }

  func save(_ key: String, _ text: String) throws {
    let fields: [String: Any] = [
      kSecValueData as String: Data(text.utf8),
      kSecAttrAccessible as String: accessible,
    ]
    var status = SecItemUpdate(valueQuery(key) as CFDictionary, fields as CFDictionary)
    if status == errSecItemNotFound {
      let add = valueQuery(key).merging(fields) { _, new in new }
      status = SecItemAdd(add as CFDictionary, nil)
    }
    guard status == errSecSuccess else {
      throw VaultError(message: "The Keychain refused the write (\(status))")
    }
  }

  func keyLevel() throws -> KeyLevel {
    KeyLevel(level: try findKey().map { level(of: $0) })
  }

  func makeKey() throws -> KeyLevel {
    makeKeyLock.lock()
    defer { makeKeyLock.unlock() }
    if let existing = try findKey() {
      return KeyLevel(level: level(of: existing))
    }
    let made = createKey(inEnclave: true) ?? createKey(inEnclave: false)
    return KeyLevel(level: made.map { level(of: $0) })
  }

  func sign(_ msg: String) throws -> Signed {
    guard let message = Data(base64Encoded: msg) else {
      throw VaultError(message: "sign needs a base64 msg")
    }
    guard let key = try findKey() else {
      throw VaultError(message: "sign found no key")
    }
    var error: Unmanaged<CFError>?
    guard let signature = SecKeyCreateSignature(key, algorithm, message as CFData, &error) as Data?
    else {
      let reason = error?.takeRetainedValue().localizedDescription ?? "unknown"
      throw VaultError(message: "The key refused to sign (\(reason))")
    }
    return Signed(sig: signature.base64EncodedString())
  }

  func verify(_ msg: String, _ sig: String) throws -> Verified {
    guard let message = Data(base64Encoded: msg) else {
      throw VaultError(message: "verify needs a base64 msg and sig")
    }
    guard let key = try findKey() else {
      throw VaultError(message: "verify found no key")
    }
    guard let publicKey = SecKeyCopyPublicKey(key) else {
      throw VaultError(message: "verify could not read the public key")
    }
    guard let signature = Data(base64Encoded: sig) else {
      return Verified(ok: false)
    }
    var error: Unmanaged<CFError>?
    let ok = SecKeyVerifySignature(
      publicKey, algorithm, message as CFData, signature as CFData, &error)
    error?.release()
    return Verified(ok: ok)
  }
}
