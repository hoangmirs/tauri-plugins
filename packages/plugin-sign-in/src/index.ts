import { invoke, isTauri } from "@tauri-apps/api/core";

export type Provider = "google" | "apple";
export type SignInError = "cancelled" | "unavailable" | "no-account" | "failed";

/** What `google` and `apple` reject with. */
export type SignInFailure = { code: SignInError; message: string };

const CODES: readonly SignInError[] = ["cancelled", "unavailable", "no-account", "failed"];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

const failure = (code: SignInError, message: string): SignInFailure => ({ code, message });

/** Any rejection as `{ code, message }`, with a code outside the four read as
 * `failed`. */
const failureOf = (e: unknown): SignInFailure => {
  if (isObj(e)) {
    const code = CODES.find((c) => c === e.code) ?? "failed";
    return failure(code, typeof e.message === "string" && e.message ? e.message : "The sign-in failed");
  }
  return failure("failed", typeof e === "string" && e ? e : "The sign-in failed");
};

const call = async (cmd: string, args: Record<string, unknown>): Promise<Record<string, unknown>> => {
  if (!isTauri()) throw failure("unavailable", "Sign-in needs the Tauri app on iOS or Android");
  let v: unknown;
  try {
    v = await invoke(`plugin:sign-in|${cmd}`, args);
  } catch (e) {
    throw failureOf(e);
  }
  if (!isObj(v)) throw failure("failed", `${cmd} answered nothing`);
  return v;
};

const text = (v: Record<string, unknown>, field: string): string => {
  const s = v[field];
  if (typeof s !== "string" || !s) throw failure("failed", `The sign-in answered no ${field}`);
  return s;
};

/** Which providers this device can show a native sheet for. Android: google
 * only. iOS: both. Desktop and web: none. Never rejects: no answer reads as
 * none. */
export async function available(): Promise<{ google: boolean; apple: boolean }> {
  try {
    const v = await call("available", {});
    return { google: v.google === true, apple: v.apple === true };
  } catch {
    return { google: false, apple: false };
  }
}

/** Google's id token. `clientId` is the web (server) client id, and the
 * token's `aud`. iOS also needs `iosClientId`. `silent` signs in with no
 * sheet when the player chose an account before, and rejects with
 * "no-account" otherwise. `nonce` goes into the token, and the answer's
 * `nonce` is the one the token carries: a silent sign-in on iOS refreshes a
 * token, which carries none. Send the service the answer's `nonce`. */
export async function google(o: {
  clientId: string;
  iosClientId?: string;
  nonce?: string;
  silent?: boolean;
}): Promise<{ idToken: string; nonce?: string }> {
  const args: Record<string, unknown> = { clientId: o.clientId };
  if (o.iosClientId !== undefined) args.iosClientId = o.iosClientId;
  if (o.nonce !== undefined) args.nonce = o.nonce;
  if (o.silent) args.silent = true;
  const v = await call("google", args);
  const idToken = text(v, "idToken");
  return typeof v.nonce === "string" && v.nonce ? { idToken, nonce: v.nonce } : { idToken };
}

/** Apple's id token, iOS only. `nonce` is the raw nonce: the plugin sends its
 * SHA-256 to Apple, so the token carries the hash. `authorizationCode` is
 * Apple's one-time code, which the service swaps for the refresh token it
 * revokes when the account is deleted. Apple has no silent sign-in: the
 * sheet always shows. */
export async function apple(o: { nonce: string }): Promise<{ idToken: string; authorizationCode: string }> {
  const v = await call("apple", { nonce: o.nonce });
  return { idToken: text(v, "idToken"), authorizationCode: text(v, "authorizationCode") };
}
