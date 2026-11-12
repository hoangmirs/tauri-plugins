import Foundation

// The Mac side of each command, over the C ABI. `src/macos.rs` is the only
// caller. `build.rs` compiles this file with `ios/Sources/Vault.swift`.

public typealias Done = @convention(c) (UnsafeMutableRawPointer?, Bool, UnsafePointer<CChar>) -> Void

private let vault = Vault()

private func json<T: Encodable>(_ value: T) -> String {
  guard let data = try? JSONEncoder().encode(value) else { return "{}" }
  return String(decoding: data, as: UTF8.self)
}

private func parse<T: Decodable>(_ type: T.Type, _ args: String) -> T? {
  try? JSONDecoder().decode(type, from: Data(args.utf8))
}

private struct Empty: Encodable {}

/// The Keychain answers at once, so `done` runs once before this returns. The
/// Rust side reads its answer from `ctx` as soon as the call comes back.
@_cdecl("tauri_plugin_vault_call")
public func call(
  _ method: UnsafePointer<CChar>, _ args: UnsafePointer<CChar>,
  _ ctx: UnsafeMutableRawPointer?, _ done: Done
) {
  let method = String(cString: method)
  let args = String(cString: args)
  let (ok, text) = run(method, args)
  text.withCString { done(ctx, ok, $0) }
}

private func run(_ method: String, _ args: String) -> (Bool, String) {
  do {
    switch method {
    case "load":
      guard let a = parse(KeyArgs.self, args) else { return (false, "load needs a key") }
      return (true, json(try vault.load(a.key)))
    case "save":
      guard let a = parse(SaveArgs.self, args) else { return (false, "save needs a key and a text") }
      try vault.save(a.key, a.text)
      return (true, json(Empty()))
    case "keyLevel":
      return (true, json(try vault.keyLevel()))
    case "makeKey":
      return (true, json(try vault.makeKey()))
    case "sign":
      guard let a = parse(SignArgs.self, args) else { return (false, "sign needs a base64 msg") }
      return (true, json(try vault.sign(a.msg)))
    case "verify":
      guard let a = parse(VerifyArgs.self, args) else {
        return (false, "verify needs a base64 msg and sig")
      }
      return (true, json(try vault.verify(a.msg, a.sig)))
    default:
      return (false, "No command \(method)")
    }
  } catch let error as VaultError {
    return (false, error.message)
  } catch {
    return (false, "\(method) failed")
  }
}
