/** An app's GrowthBook rules, evaluated on the device. The SDK gets the rules
 *  file and the key, never a host or a client key, so it never calls
 *  GrowthBook. */

import { GrowthBook } from "@growthbook/growthbook";
import type { Assignment, Keys, Part } from "./types.ts";
import { asText } from "./values.ts";

/** What a rule can test. A new kind of condition needs a new attribute, and a release. */
export type Attributes = {
  id: string | null;
  platform: "ios" | "android" | "macos" | "windows" | "linux" | "web";
  web: boolean;
  version: string;
  country: string | null;
  language: string;
  now: string;
  [extra: string]: unknown;
};

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** The source part for the app's own keys, and the test groups they fall
 *  into. Null when the file does not read or does not decrypt with this key,
 *  so the saved rules stand. */
export async function evaluate(
  rulesText: string | null,
  attrs: Attributes,
  keyB64: string,
  keys: Keys,
): Promise<{ part: Part; assignments: Assignment[] } | null> {
  if (rulesText === null) return null;
  let payload: unknown;
  try {
    payload = JSON.parse(rulesText);
  } catch {
    return null;
  }
  if (!isRecord(payload)) return null;
  const attributes = Object.fromEntries(Object.entries(attrs).filter(([, v]) => v !== null && v !== undefined));
  const assignments: Assignment[] = [];
  const gb = new GrowthBook({
    attributes,
    decryptionKey: keyB64,
    trackingCallback: (experiment, result) => {
      if (result.featureId) assignments.push({ key: result.featureId, experiment: experiment.key, variant: result.key });
    },
  });
  try {
    await gb.setPayload(payload as Parameters<GrowthBook["setPayload"]>[0]);
    // The SDK logs a failed decrypt and goes on with no features. A file for
    // another key must keep the saved rules, not empty them.
    const features = gb.getDecryptedPayload().features;
    if (!isRecord(features)) return null;
    const part: Part = {};
    for (const [name, key] of Object.entries(keys)) {
      if (key.global) continue;
      const result = gb.evalFeature(name);
      if (result.source === "unknownFeature" || result.value === null) continue;
      part[name] = asText(result.value);
    }
    return { part, assignments };
  } catch {
    return null;
  } finally {
    gb.destroy();
  }
}
