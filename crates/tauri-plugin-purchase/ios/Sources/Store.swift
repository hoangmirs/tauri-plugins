import StoreKit

// StoreKit 2 for both platforms: `PurchasePlugin.swift` calls it on iOS, and
// `macos/Bridge.swift` on the Mac. Nothing here imports Tauri, so the Mac's
// build compiles this file as it is.

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

enum Store {
  // A purchase can complete outside the purchase sheet: one a parent approves
  // later, or one cut off when the app closed. StoreKit delivers those here,
  // and keeps delivering them on each launch until they are finished.
  // A consumable is not finished here: the app claims it into its wallet
  // first, and finishing it before then would lose what the player paid for.
  static func listen(_ arrived: @escaping (Open) -> Void) -> Task<Void, Never> {
    Task.detached {
      for await result in Transaction.updates {
        guard case .verified(let transaction) = result else { continue }
        if transaction.productType == .consumable {
          arrived(Open(id: transaction.productID, tx: String(transaction.id)))
        } else {
          await transaction.finish()
        }
      }
    }
  }

  static func prices(_ ids: [String]) async throws -> Prices {
    let found = try await Product.products(for: ids)
    return Prices(products: found.map { Price(id: $0.id, price: $0.displayPrice) })
  }

  /// Nil when the store has no product with this id.
  static func buy(_ id: String, consumable: Bool) async -> Bought? {
    do {
      guard let product = try await Product.products(for: [id]).first else { return nil }
      switch try await product.purchase() {
      case .success(let result):
        // An unverified transaction failed StoreKit's own signature check,
        // so it is reported as failed and never finished.
        guard case .verified(let transaction) = result else {
          return Bought(state: "failed")
        }
        if consumable || transaction.productType == .consumable {
          return Bought(
            state: "bought", proof: result.jwsRepresentation, tx: String(transaction.id))
        }
        await transaction.finish()
        return Bought(state: "bought", proof: result.jwsRepresentation)
      case .userCancelled:
        return Bought(state: "cancelled")
      case .pending:
        return Bought(state: "pending")
      @unknown default:
        return Bought(state: "failed")
      }
    } catch {
      return Bought(state: "failed")
    }
  }

  // A transaction that is already finished, or that StoreKit does not know,
  // ends here too: finishing twice must not fail a claim that went through.
  static func finish(_ id: UInt64) async {
    for await result in Transaction.unfinished {
      if case .verified(let transaction) = result, transaction.id == id {
        await transaction.finish()
        break
      }
    }
  }

  static func unfinished(_ ids: [String]) async -> Unfinished {
    let ids = Set(ids)
    var open: [Open] = []
    for await result in Transaction.unfinished {
      guard case .verified(let transaction) = result, ids.contains(transaction.productID)
      else { continue }
      open.append(Open(id: transaction.productID, tx: String(transaction.id)))
    }
    return Unfinished(open: open)
  }

  // currentEntitlements leaves out refunded and revoked purchases, and works
  // from the device's own copy when it is offline.
  static func owned(_ ids: [String]) async -> Owned {
    let ids = Set(ids)
    var out: [Held] = []
    for await result in Transaction.currentEntitlements {
      guard case .verified(let transaction) = result, ids.contains(transaction.productID),
        transaction.revocationDate == nil
      else { continue }
      out.append(Held(id: transaction.productID, proof: result.jwsRepresentation))
    }
    return Owned(owned: out)
  }

  // Apple asks apps to sync only when the player asks for it: it can show a
  // sign-in sheet. A cancelled or failed sync still answers what is held.
  static func restore(_ ids: [String]) async -> Owned {
    try? await AppStore.sync()
    return await owned(ids)
  }
}
