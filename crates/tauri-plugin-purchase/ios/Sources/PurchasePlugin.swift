import Tauri
import WebKit

// The iOS side of each command. `Store.swift` does the StoreKit work, which
// the Mac shares.
class PurchasePlugin: Plugin {
  private var updates: Task<Void, Never>?

  public override func load(webview: WKWebView) {
    updates = Store.listen { [weak self] open in
      self?.trigger("arrived", data: ["id": open.id, "tx": open.tx])
    }
  }

  @objc public func products(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("products needs ids")
      return
    }
    Task {
      do {
        invoke.resolve(try await Store.prices(args.ids))
      } catch {
        invoke.reject(error.localizedDescription)
      }
    }
  }

  @objc public func buy(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(BuyArgs.self) else {
      invoke.reject("buy needs an id")
      return
    }
    Task {
      if let bought = await Store.buy(args.id, consumable: args.consumable == true) {
        invoke.resolve(bought)
      } else {
        invoke.reject("The store has no product \(args.id)")
      }
    }
  }

  @objc public func finish(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(FinishArgs.self), let id = UInt64(args.tx) else {
      invoke.reject("finish needs a tx")
      return
    }
    Task {
      await Store.finish(id)
      invoke.resolve([:])
    }
  }

  @objc public func unfinished(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("unfinished needs ids")
      return
    }
    Task {
      invoke.resolve(await Store.unfinished(args.ids))
    }
  }

  @objc public func owned(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("owned needs ids")
      return
    }
    Task {
      invoke.resolve(await Store.owned(args.ids))
    }
  }

  @objc public func restore(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("restore needs ids")
      return
    }
    Task {
      invoke.resolve(await Store.restore(args.ids))
    }
  }
}

@_cdecl("init_plugin_purchase")
func initPlugin() -> Plugin {
  return PurchasePlugin()
}
