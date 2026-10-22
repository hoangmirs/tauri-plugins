import StoreKit
import Tauri
import WebKit

struct IdsArgs: Decodable {
  let ids: [String]
}

struct BuyArgs: Decodable {
  let id: String
  let consumable: Bool?
}

struct FinishArgs: Decodable {
  let tx: String
}

struct Price: Encodable {
  let id: String
  let price: String
}

struct Prices: Encodable {
  let products: [Price]
}

struct Bought: Encodable {
  let state: String
  let proof: String?
  let tx: String?

  init(state: String, proof: String? = nil, tx: String? = nil) {
    self.state = state
    self.proof = proof
    self.tx = tx
  }
}

struct Open: Encodable {
  let id: String
  let tx: String
}

struct Unfinished: Encodable {
  let open: [Open]
}

struct Held: Encodable {
  let id: String
  let proof: String
}

struct Owned: Encodable {
  let owned: [Held]
}

class PurchasePlugin: Plugin {
  private var updates: Task<Void, Never>?

  // A purchase can complete outside the purchase sheet: one a parent approves
  // later, or one cut off when the app closed. StoreKit delivers those here,
  // and keeps delivering them on each launch until they are finished.
  // A consumable is not finished here: the app claims it into its wallet
  // first, and finishing it before then would lose what the player paid for.
  public override func load(webview: WKWebView) {
    updates = Task.detached { [weak self] in
      for await result in Transaction.updates {
        guard case .verified(let transaction) = result else { continue }
        if transaction.productType == .consumable {
          self?.trigger(
            "arrived", data: ["id": transaction.productID, "tx": String(transaction.id)])
        } else {
          await transaction.finish()
        }
      }
    }
  }

  @objc public func products(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("products needs ids")
      return
    }
    Task {
      do {
        let found = try await Product.products(for: args.ids)
        invoke.resolve(Prices(products: found.map { Price(id: $0.id, price: $0.displayPrice) }))
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
      do {
        guard let product = try await Product.products(for: [args.id]).first else {
          invoke.reject("The store has no product \(args.id)")
          return
        }
        switch try await product.purchase() {
        case .success(let result):
          // An unverified transaction failed StoreKit's own signature check,
          // so it is reported as failed and never finished.
          guard case .verified(let transaction) = result else {
            invoke.resolve(Bought(state: "failed"))
            return
          }
          if args.consumable == true || transaction.productType == .consumable {
            invoke.resolve(
              Bought(
                state: "bought", proof: result.jwsRepresentation, tx: String(transaction.id)))
            return
          }
          await transaction.finish()
          invoke.resolve(Bought(state: "bought", proof: result.jwsRepresentation))
        case .userCancelled:
          invoke.resolve(Bought(state: "cancelled"))
        case .pending:
          invoke.resolve(Bought(state: "pending"))
        @unknown default:
          invoke.resolve(Bought(state: "failed"))
        }
      } catch {
        invoke.resolve(Bought(state: "failed"))
      }
    }
  }

  // A transaction that is already finished, or that StoreKit does not know,
  // answers {} too: finishing twice must not fail a claim that went through.
  @objc public func finish(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(FinishArgs.self), let id = UInt64(args.tx) else {
      invoke.reject("finish needs a tx")
      return
    }
    Task {
      for await result in Transaction.unfinished {
        if case .verified(let transaction) = result, transaction.id == id {
          await transaction.finish()
          break
        }
      }
      invoke.resolve([:])
    }
  }

  @objc public func unfinished(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("unfinished needs ids")
      return
    }
    Task {
      let ids = Set(args.ids)
      var open: [Open] = []
      for await result in Transaction.unfinished {
        guard case .verified(let transaction) = result, ids.contains(transaction.productID)
        else { continue }
        open.append(Open(id: transaction.productID, tx: String(transaction.id)))
      }
      invoke.resolve(Unfinished(open: open))
    }
  }

  @objc public func owned(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("owned needs ids")
      return
    }
    Task {
      invoke.resolve(Owned(owned: await held(Set(args.ids))))
    }
  }

  // Apple asks apps to sync only when the player asks for it: it can show a
  // sign-in sheet. A cancelled or failed sync still answers what is held.
  @objc public func restore(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(IdsArgs.self) else {
      invoke.reject("restore needs ids")
      return
    }
    Task {
      try? await AppStore.sync()
      invoke.resolve(Owned(owned: await held(Set(args.ids))))
    }
  }

  // currentEntitlements leaves out refunded and revoked purchases, and works
  // from the phone's own copy when it is offline.
  private func held(_ ids: Set<String>) async -> [Held] {
    var out: [Held] = []
    for await result in Transaction.currentEntitlements {
      guard case .verified(let transaction) = result, ids.contains(transaction.productID),
        transaction.revocationDate == nil
      else { continue }
      out.append(Held(id: transaction.productID, proof: result.jwsRepresentation))
    }
    return out
  }
}

@_cdecl("init_plugin_purchase")
func initPlugin() -> Plugin {
  return PurchasePlugin()
}
