/** A GrowthBook SDK payload, as its CDN serves it. */
export type Payload = {
  features: Record<string, unknown>;
  experiments?: unknown[];
  savedGroups?: Record<string, unknown>;
  [k: string]: unknown;
};

/** A payload with its rules encrypted in the format the GrowthBook SDK decrypts. */
export type EncryptedPayload = {
  encryptedFeatures: string;
  encryptedExperiments?: string;
  encryptedSavedGroups?: string;
  [k: string]: unknown;
};

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

const toBase64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));
const fromBase64 = (text: string) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

/** The payload, or null when it is not one: a body with the wrong shape must never replace a good file. */
export function readPayload(raw: unknown): Payload | null {
  if (!isRecord(raw) || !isRecord(raw.features)) return null;
  if (raw.experiments !== undefined && !Array.isArray(raw.experiments)) return null;
  if (raw.savedGroups !== undefined && !isRecord(raw.savedGroups)) return null;
  return raw as Payload;
}

async function encrypt(text: string, key: CryptoKey): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(16));
  const out = await crypto.subtle.encrypt({ name: "AES-CBC", iv }, key, new TextEncoder().encode(text));
  return `${toBase64(iv)}.${toBase64(new Uint8Array(out))}`;
}

/** AES-CBC with a 128-bit key, `<iv>.<ciphertext>` in base64, as `decrypt` in the SDK's `util.ts` reads it. */
export async function encryptPayload(p: Payload, keyB64: string): Promise<EncryptedPayload> {
  // `wrangler secret put` keeps a pasted trailing newline.
  const key = await crypto.subtle.importKey("raw", fromBase64(keyB64.trim()), { name: "AES-CBC" }, false, ["encrypt"]);
  const { features, experiments, savedGroups, ...rest } = p;
  const out: EncryptedPayload = { ...rest, encryptedFeatures: await encrypt(JSON.stringify(features), key) };
  if (experiments !== undefined) out.encryptedExperiments = await encrypt(JSON.stringify(experiments), key);
  if (savedGroups !== undefined) out.encryptedSavedGroups = await encrypt(JSON.stringify(savedGroups), key);
  return out;
}

export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
