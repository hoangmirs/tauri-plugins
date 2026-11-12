import Tauri
import Foundation

// The iOS side of each command. `Vault.swift` does the Keychain work, which
// the Mac shares.
class VaultPlugin: Plugin {
  private let vault = Vault()

  /// Runs `body` and answers with its result, or rejects with its message.
  private func answer<T: Encodable>(_ invoke: Invoke, _ fallback: String, _ body: () throws -> T) {
    do {
      invoke.resolve(try body())
    } catch let error as VaultError {
      invoke.reject(error.message)
    } catch {
      invoke.reject(fallback)
    }
  }

  @objc public func load(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(KeyArgs.self) else {
      invoke.reject("load needs a key")
      return
    }
    answer(invoke, "load failed") { try vault.load(args.key) }
  }

  @objc public func save(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(SaveArgs.self) else {
      invoke.reject("save needs a key and a text")
      return
    }
    do {
      try vault.save(args.key, args.text)
      invoke.resolve()
    } catch let error as VaultError {
      invoke.reject(error.message)
    } catch {
      invoke.reject("save failed")
    }
  }

  @objc public func keyLevel(_ invoke: Invoke) {
    answer(invoke, "keyLevel failed") { try vault.keyLevel() }
  }

  @objc public func makeKey(_ invoke: Invoke) {
    answer(invoke, "makeKey failed") { try vault.makeKey() }
  }

  @objc public func sign(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(SignArgs.self) else {
      invoke.reject("sign needs a base64 msg")
      return
    }
    answer(invoke, "sign failed") { try vault.sign(args.msg) }
  }

  @objc public func verify(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(VerifyArgs.self) else {
      invoke.reject("verify needs a base64 msg and sig")
      return
    }
    answer(invoke, "verify failed") { try vault.verify(args.msg, args.sig) }
  }
}

@_cdecl("init_plugin_vault")
func initPlugin() -> Plugin {
  return VaultPlugin()
}
