import { invoke, isTauri } from "@tauri-apps/api/core";

/** A product's price, as the store writes it in the player's currency. */
export type Price = { id: string; price: string };

/** What the store's purchase sheet ended with. `proof` is for a server to
 * check with the store: the signed transaction on iOS, the purchase token on
 * Android. */
export type Bought = { state: "bought"; proof: string } | { state: "cancelled" | "pending" | "failed" };

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
 * reads as cancelled, pending or failed. */
export async function buy(id: string): Promise<Bought> {
  const v = await call("buy", { id });
  if (!isObj(v)) return { state: "failed" };
  if (v.state === "bought" && typeof v.proof === "string" && v.proof) return { state: "bought", proof: v.proof };
  if (v.state === "cancelled" || v.state === "pending") return { state: v.state };
  return { state: "failed" };
}

/** What this store account owns now, among `ids`. A refunded purchase is not
 * in it. Empty when it cannot be read. */
export async function owned(ids: string[]): Promise<Held[]> {
  const v = await call("owned", { ids });
  return isObj(v) ? pairs(v.owned, "id", "proof").filter((h) => ids.includes(h.id)) : [];
}

/** The same as `owned`, after asking the store to bring back past purchases.
 * Call it from a button: on iOS it can ask the player to sign in. */
export async function restore(ids: string[]): Promise<Held[]> {
  const v = await call("restore", { ids });
  return isObj(v) ? pairs(v.owned, "id", "proof").filter((h) => ids.includes(h.id)) : [];
}
