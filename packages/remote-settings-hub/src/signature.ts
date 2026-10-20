/** Equal strings, in a time that does not tell how much of them matched. */
function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** GrowthBook's event webhooks send `X-GrowthBook-Signature`: the HMAC-SHA256 of the body, in hex, keyed with the secret's text. */
export async function verifyEventSignature(body: string, header: string | null, secret: string): Promise<boolean> {
  const key = secret.trim();
  if (!header || !key) return false;
  const hmac = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", hmac, new TextEncoder().encode(body));
  const want = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return same(header.trim().toLowerCase(), want);
}
