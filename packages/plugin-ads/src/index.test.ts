import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import {
  consent,
  hideNative,
  init,
  loadRewarded,
  privacyOptions,
  setMuted,
  showNative,
  showPrivacyOptions,
  showRewarded,
  type Slot,
} from "./index.ts";

// mockIPC writes to `window`, which Node lacks, and leaves `isTauri` unset.
const g = globalThis as { window?: unknown; isTauri?: boolean };
g.window ??= globalThis;
const inTauri = (handler: Parameters<typeof mockIPC>[0]) => {
  g.isTauri = true;
  mockIPC(handler);
};
afterEach(() => {
  clearMocks();
  delete g.isTauri;
});

const SLOT: Slot = {
  unitId: "u", x: 0, y: 900, vx: 16, vy: 400, w: 343, h: 140, scrollHeight: 1200,
  ink: "rgb(0, 0, 0)", muted: "rgb(0, 0, 0)", button: "rgb(0, 0, 0)", edge: "rgb(0, 0, 0)",
};

test("outside Tauri every call answers its failure value and does not reject", async () => {
  assert.equal(await init({ childDirected: true, maxRating: "G" }), undefined);
  assert.equal(await consent(), "failed");
  assert.equal(await showNative(SLOT), undefined);
  assert.equal(await hideNative(), undefined);
  assert.equal(await loadRewarded("u"), "failed");
  assert.equal(await showRewarded(), "failed");
  assert.equal(await privacyOptions(), "not_required");
  assert.equal(await showPrivacyOptions(), "failed");
  assert.equal(await setMuted(true), undefined);
});

test("the privacy options and mute calls name their commands and arguments", async () => {
  const seen: [string, unknown][] = [];
  inTauri((cmd, args) => {
    seen.push([cmd, args]);
    return cmd.endsWith("|privacy_options") ? "required" : cmd.endsWith("show_privacy_options") ? "shown" : null;
  });
  assert.equal(await privacyOptions(), "required");
  assert.equal(await showPrivacyOptions(), "shown");
  await setMuted(true);
  assert.deepEqual(
    seen.map(([cmd]) => cmd),
    ["privacy_options", "show_privacy_options", "set_muted"].map((c) => `plugin:ads|${c}`),
  );
  assert.deepEqual(seen[2]?.[1], { muted: true });
});

test("each call names its command and arguments", async () => {
  const seen: [string, unknown][] = [];
  inTauri((cmd, args) => {
    seen.push([cmd, args]);
    return cmd.endsWith("load_rewarded") ? "ready" : cmd.endsWith("show_rewarded") ? "earned" : "obtained";
  });
  await init({ childDirected: true, maxRating: "G" });
  assert.equal(await consent(), "obtained");
  await showNative(SLOT);
  await hideNative();
  assert.equal(await loadRewarded("u"), "ready");
  assert.equal(await showRewarded(), "earned");
  assert.deepEqual(
    seen.map(([cmd]) => cmd),
    ["init", "consent", "show_native", "hide_native", "load_rewarded", "show_rewarded"].map((c) => `plugin:ads|${c}`),
  );
  assert.deepEqual(seen[0]?.[1], { settings: { childDirected: true, maxRating: "G" } });
  assert.deepEqual(seen[2]?.[1], { slot: SLOT });
  assert.deepEqual(seen[4]?.[1], { unitId: "u" });
});

test("a call that throws answers its failure value", async () => {
  inTauri(() => {
    throw new Error("no native side");
  });
  assert.equal(await consent(), "failed");
  assert.equal(await loadRewarded("u"), "failed");
  assert.equal(await showRewarded(), "failed");
  assert.equal(await showNative(SLOT), undefined);
  assert.equal(await privacyOptions(), "not_required");
  assert.equal(await showPrivacyOptions(), "failed");
});
