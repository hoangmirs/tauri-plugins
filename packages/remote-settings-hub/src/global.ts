/** Builds an app's global file for its push script, and refuses it when one
 *  value would not read on the device. One bad value stops the push, so a
 *  typo never reaches users. */

const PLUGIN_KEYS: Record<string, readonly string[]> = {
  source: ["growthbook", "firebase"],
  apply: ["hidden", "rest", "now"],
};

const asText = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));

export function buildGlobal(
  file: Record<string, unknown>,
  keys: Record<string, { read: (text: string) => unknown; global?: boolean }>,
): { ok: true; body: string } | { ok: false; bad: string[] } {
  const bad: string[] = [];
  for (const [k, v] of Object.entries(file)) {
    const words = PLUGIN_KEYS[k];
    if (words) {
      if (!words.includes(asText(v))) bad.push(k);
      continue;
    }
    const key = keys[k];
    if (!key?.global || key.read(asText(v)) === undefined) bad.push(k);
  }
  return bad.length > 0 ? { ok: false, bad } : { ok: true, body: `${JSON.stringify(file)}\n` };
}
