import Foundation

// The Mac side of each command, over the C ABI. `src/macos.rs` is the only
// caller. `build.rs` compiles this file with `ios/Sources/Store.swift`.

public typealias Done = @convention(c) (UnsafeMutableRawPointer?, Bool, UnsafePointer<CChar>) -> Void
public typealias Arrived = @convention(c) (UnsafePointer<CChar>) -> Void

/// The Rust side frees its waiting future when `done` runs, so each call must
/// end in exactly one `ok` or `fail`. A second answer would free it twice.
final class Reply: @unchecked Sendable {
  private let ctx: UnsafeMutableRawPointer?
  private let done: Done

  init(_ ctx: UnsafeMutableRawPointer?, _ done: Done) {
    self.ctx = ctx
    self.done = done
  }

  func ok<T: Encodable>(_ value: T) {
    answer(true, json(value))
  }

  func fail(_ message: String) {
    answer(false, message)
  }

  private func answer(_ ok: Bool, _ text: String) {
    text.withCString { done(ctx, ok, $0) }
  }
}

struct Empty: Encodable {}

func json<T: Encodable>(_ value: T) -> String {
  guard let data = try? JSONEncoder().encode(value) else { return "{}" }
  return String(decoding: data, as: UTF8.self)
}

func parse<T: Decodable>(_ type: T.Type, _ args: String) -> T? {
  try? JSONDecoder().decode(type, from: Data(args.utf8))
}

/// Holds the listener so it lives as long as the app.
var updates: Task<Void, Never>?

@_cdecl("tauri_plugin_purchase_listen")
public func listen(_ arrived: Arrived) {
  updates = Store.listen { open in
    json(open).withCString { arrived($0) }
  }
}

@_cdecl("tauri_plugin_purchase_call")
public func call(
  _ method: UnsafePointer<CChar>, _ args: UnsafePointer<CChar>,
  _ ctx: UnsafeMutableRawPointer?, _ done: Done
) {
  // Both strings belong to the caller only until this function returns.
  let method = String(cString: method)
  let args = String(cString: args)
  let reply = Reply(ctx, done)
  Task {
    switch method {
    case "products":
      guard let a = parse(IdsArgs.self, args) else { return reply.fail("products needs ids") }
      do {
        reply.ok(try await Store.prices(a.ids))
      } catch {
        reply.fail(error.localizedDescription)
      }
    case "buy":
      guard let a = parse(BuyArgs.self, args) else { return reply.fail("buy needs an id") }
      if let bought = await Store.buy(a.id, consumable: a.consumable == true) {
        reply.ok(bought)
      } else {
        reply.fail("The store has no product \(a.id)")
      }
    case "finish":
      guard let a = parse(FinishArgs.self, args), let id = UInt64(a.tx) else {
        return reply.fail("finish needs a tx")
      }
      await Store.finish(id)
      reply.ok(Empty())
    case "unfinished":
      guard let a = parse(IdsArgs.self, args) else { return reply.fail("unfinished needs ids") }
      reply.ok(await Store.unfinished(a.ids))
    case "owned":
      guard let a = parse(IdsArgs.self, args) else { return reply.fail("owned needs ids") }
      reply.ok(await Store.owned(a.ids))
    case "restore":
      guard let a = parse(IdsArgs.self, args) else { return reply.fail("restore needs ids") }
      reply.ok(await Store.restore(a.ids))
    default:
      reply.fail("No command \(method)")
    }
  }
}
