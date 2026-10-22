import { addPluginListener, invoke, isTauri } from "@tauri-apps/api/core";

/** A product's price, as the store writes it in the player's currency. */
export type Price = { id: string; price: string };

/** What the store's purchase sheet ended with. `proof` is for a server to
 * check with the store: the signed transaction on iOS, the purchase token on
 * Android. */
export type Bought = { state: "bought"; proof: string } | { state: "cancelled" | "pending" | "failed" };

/** What `buy` ends with for a consumable. `tx` is the transaction id on iOS
 * and the purchase token on Android. The purchase stays open until `finish`. */
export type BoughtConsumable =
  | { state: "bought"; proof: string; tx: string }
  | { state: "cancelled" | "pending" | "failed" };

/** A consumable the store holds open: bought, and not yet finished. */
export type Open = { id: string; tx: string };

/** A product this store account owns now, with its proof. */
export type Held = { id: string; proof: string };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** Each item of `list` that has the two string fields, rebuilt. */
const pairs = <K extends string>(list: unknown, a: "id", b: K): ({ id: string } & Record<K, string>)[] =>
  Array.isArray(list)
    ? list.flatMap((x) =>
        isObj(x) && typeof x[a] === "string" && typeof x[b] === "string" && x[b] !== ""
          ? [{ id: x[a], [b]: x[b] } as { id: string } & Record<K, string>]
          : [],
      )
    : [];

const call = async (cmd: string, args: Record<string, unknown>): Promise<unknown> => {
  if (!isTauri()) return null;
  try {
    return await invoke(`plugin:purchase|${cmd}`, args);
  } catch {
    return null;
  }
};

/** The prices of the products the store knows. Outside Tauri, on a desktop,
 * or when the store cannot be reached, it is empty. */
export async function products(ids: string[]): Promise<Price[]> {
  const v = await call("products", { ids });
  return isObj(v) ? pairs(v.products, "id", "price") : [];
}

/** Opens the store's purchase sheet. Anything but a purchase with its proof
 * reads as cancelled, pending or failed. A consumable also answers `tx`, and
 * stays open until `finish(tx)`: claim it first, then finish it. A consumable
 * bought without a `tx` reads as failed, because it could not be finished. */
export async function buy(id: string, options: { consumable: true }): Promise<BoughtConsumable>;
export async function buy(id: string, options?: { consumable?: false }): Promise<Bought>;
export async function buy(id: string, options?: { consumable?: boolean }): Promise<Bought | BoughtConsumable> {
  const consumable = options?.consumable === true;
  const v = await call("buy", consumable ? { id, consumable } : { id });
  if (!isObj(v)) return { state: "failed" };
  if (v.state === "bought" && typeof v.proof === "string" && v.proof) {
    if (!consumable) return { state: "bought", proof: v.proof };
    if (typeof v.tx === "string" && v.tx) return { state: "bought", proof: v.proof, tx: v.tx };
    return { state: "failed" };
  }
  if (v.state === "cancelled" || v.state === "pending") return { state: v.state };
  return { state: "failed" };
}

/** Finishes a consumable the app has claimed: `finish` on iOS, consume on
 * Android. True when the store took it, false when it did not, so call it
 * again later. Finishing one twice is fine. */
export async function finish(tx: string): Promise<boolean> {
  return isObj(await call("finish", { tx }));
}

/** The consumables among `ids` that are bought and not yet finished, or null
 * when the store cannot be asked. Claim each, then `finish` it. */
export async function unfinished(ids: string[]): Promise<Open[] | null> {
  const v = await call("unfinished", { ids });
  return isObj(v) ? pairs(v.open, "id", "tx").filter((o) => ids.includes(o.id)) : null;
}

/** Calls `fn` when a consumable completes later: a pending Android payment
 * that went through, or an iOS purchase the app closed on. Claim it, then
 * `finish` it. Returns a function that stops listening. Outside Tauri it
 * never calls `fn`. */
export async function onArrived(fn: (open: Open) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const listener = await addPluginListener<unknown>("purchase", "arrived", (p) => {
      const [open] = pairs([p], "id", "tx");
      if (open) fn(open);
    });
    return () => void listener.unregister();
  } catch {
    return () => {};
  }
}

/** `{ owned: [{ id, proof }] }`, kept to `ids`. Null for no answer. */
const heldIn = (v: unknown, ids: string[]): Held[] | null =>
  isObj(v) ? pairs(v.owned, "id", "proof").filter((h) => ids.includes(h.id)) : null;

/** What this store account owns now, among `ids`. A refunded purchase is not
 * in it. Null when the store cannot be asked: outside Tauri, on a desktop, or
 * when the store does not answer. Keep the last answer then, because reading
 * null as "owns nothing" locks what a player paid for. */
export async function owned(ids: string[]): Promise<Held[] | null> {
  return heldIn(await call("owned", { ids }), ids);
}

/** The same as `owned`, after asking the store to bring back past purchases.
 * Call it from a button: on iOS it can ask the player to sign in. */
export async function restore(ids: string[]): Promise<Held[] | null> {
  return heldIn(await call("restore", { ids }), ids);
}
