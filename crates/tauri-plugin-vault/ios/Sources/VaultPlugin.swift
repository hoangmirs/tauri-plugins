import Foundation
import Security
import Tauri

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

/// Answers `{"level": null}` when there is no key, so the Rust side reads
/// "no key" and not a missing field.
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
/// Enclave key when the phone has one, else a software key in the Keychain.
/// Both stay on this device and are never synchronized.
class VaultPlugin: Plugin {
  private let bundleId = Bundle.main.bundleIdentifier ?? "app"
  private var service: String { bundleId + ".vault" }
  private var keyTag: Data { Data((bundleId + ".vault.key").utf8) }
  private let accessible = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
  private let algorithm = SecKeyAlgorithm.ecdsaSignatureMessageX962SHA256

  // Two calls at once would each find no key and make two keys under one tag.
  private let makeKeyLock = NSLock()

  private func valueQuery(_ key: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: key,
      kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
    ]
  }

  private var keyQuery: [String: Any] {
    [
      kSecClass as String: kSecClassKey,
      kSecAttrApplicationTag as String: keyTag,
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
    ]
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
    guard status == errSecSuccess, let found, CFGetTypeID(found) == SecKeyGetTypeID() else {
      throw VaultError(message: "The Keychain refused the key read (\(status))")
    }
    return unsafeDowncast(found, to: SecKey.self)
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
    return SecKeyCreateRandomKey(attributes as CFDictionary, &error)
  }

  @objc public func load(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(KeyArgs.self) else {
      invoke.reject("load needs a key")
      return
    }
    var ask = valueQuery(args.key)
    ask[kSecReturnData as String] = true
    ask[kSecMatchLimit as String] = kSecMatchLimitOne
    var found: CFTypeRef?
    let status = SecItemCopyMatching(ask as CFDictionary, &found)
    if status == errSecItemNotFound {
      invoke.resolve(Loaded(text: nil, unreadable: nil))
    } else if status == errSecSuccess, let data = found as? Data {
      if let text = String(data: data, encoding: .utf8) {
        invoke.resolve(Loaded(text: text, unreadable: nil))
      } else {
        invoke.resolve(Loaded(text: nil, unreadable: true))
      }
    } else {
      invoke.reject("The Keychain refused the read (\(status))")
    }
  }

  @objc public func save(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(SaveArgs.self) else {
      invoke.reject("save needs a key and a text")
      return
    }
    let fields: [String: Any] = [
      kSecValueData as String: Data(args.text.utf8),
      kSecAttrAccessible as String: accessible,
    ]
    var status = SecItemUpdate(valueQuery(args.key) as CFDictionary, fields as CFDictionary)
    if status == errSecItemNotFound {
      let add = valueQuery(args.key).merging(fields) { _, new in new }
      status = SecItemAdd(add as CFDictionary, nil)
    }
    if status == errSecSuccess {
      invoke.resolve()
    } else {
      invoke.reject("The Keychain refused the write (\(status))")
    }
  }

  @objc public func keyLevel(_ invoke: Invoke) {
    do {
      let key = try findKey()
      invoke.resolve(KeyLevel(level: key.map { level(of: $0) }))
    } catch let error as VaultError {
      invoke.reject(error.message)
    } catch {
      invoke.reject("keyLevel failed")
    }
  }

  @objc public func makeKey(_ invoke: Invoke) {
    makeKeyLock.lock()
    defer { makeKeyLock.unlock() }
    do {
      if let existing = try findKey() {
        invoke.resolve(KeyLevel(level: level(of: existing)))
        return
      }
    } catch let error as VaultError {
      invoke.reject(error.message)
      return
    } catch {
      invoke.reject("makeKey failed")
      return
    }
    let made = createKey(inEnclave: true) ?? createKey(inEnclave: false)
    invoke.resolve(KeyLevel(level: made.map { level(of: $0) }))
  }

  @objc public func sign(_ invoke: Invoke) {
    guard
      let args = try? invoke.parseArgs(SignArgs.self),
      let message = Data(base64Encoded: args.msg)
    else {
      invoke.reject("sign needs a base64 msg")
      return
    }
    do {
      guard let key = try findKey() else {
        invoke.reject("sign found no key")
        return
      }
      var error: Unmanaged<CFError>?
      guard
        let signature = SecKeyCreateSignature(key, algorithm, message as CFData, &error) as Data?
      else {
        let reason = error?.takeRetainedValue().localizedDescription ?? "unknown"
        invoke.reject("The key refused to sign (\(reason))")
        return
      }
      invoke.resolve(Signed(sig: signature.base64EncodedString()))
    } catch let error as VaultError {
      invoke.reject(error.message)
    } catch {
      invoke.reject("sign failed")
    }
  }

  @objc public func verify(_ invoke: Invoke) {
    guard
      let args = try? invoke.parseArgs(VerifyArgs.self),
      let message = Data(base64Encoded: args.msg)
    else {
      invoke.reject("verify needs a base64 msg and sig")
      return
    }
    do {
      guard let key = try findKey() else {
        invoke.reject("verify found no key")
        return
      }
      guard let publicKey = SecKeyCopyPublicKey(key) else {
        invoke.reject("verify could not read the public key")
        return
      }
      guard let signature = Data(base64Encoded: args.sig) else {
        invoke.resolve(Verified(ok: false))
        return
      }
      var error: Unmanaged<CFError>?
      let ok = SecKeyVerifySignature(
        publicKey, algorithm, message as CFData, signature as CFData, &error)
      error?.release()
      invoke.resolve(Verified(ok: ok))
    } catch let error as VaultError {
      invoke.reject(error.message)
    } catch {
      invoke.reject("verify failed")
    }
  }
}

@_cdecl("init_plugin_vault")
func initPlugin() -> Plugin {
  return VaultPlugin()
}
