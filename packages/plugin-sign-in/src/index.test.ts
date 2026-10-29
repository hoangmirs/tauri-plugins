import assert from "node:assert/strict";
import { test } from "node:test";
import { mockIPC } from "@tauri-apps/api/mocks";
import { apple, available, google } from "./index.ts";

const WEB = "1-web.apps.googleusercontent.com";

const inTauri = async (handler: Parameters<typeof mockIPC>[0], run: () => Promise<void>) => {
  const g = globalThis as Record<string, unknown>;
  g.window = globalThis;
  g.isTauri = true;
  mockIPC(handler);
  try {
    await run();
  } finally {
    delete g.isTauri;
    delete g.window;
  }
};

test("outside Tauri no sheet is available and each sign-in rejects with unavailable", async () => {
  assert.deepEqual(await available(), { google: false, apple: false });
  await assert.rejects(google({ clientId: WEB }), { code: "unavailable" });
  await assert.rejects(apple({ nonce: "n1" }), { code: "unavailable" });
});

test("google sends only the options given and answers the token and the nonce it carries", async () => {
  const seen: unknown[] = [];
  await inTauri(
    (cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      const a = args as { nonce?: string };
      return a.nonce ? { idToken: "jwt", nonce: a.nonce } : { idToken: "jwt" };
    },
    async () => {
      assert.deepEqual(await google({ clientId: WEB, iosClientId: "i", nonce: "n1", silent: true }), {
        idToken: "jwt",
        nonce: "n1",
      });
      assert.deepEqual(seen[0], { cmd: "plugin:sign-in|google", clientId: WEB, iosClientId: "i", nonce: "n1", silent: true });
      assert.deepEqual(await google({ clientId: WEB }), { idToken: "jwt" });
      assert.deepEqual(seen[1], { cmd: "plugin:sign-in|google", clientId: WEB });
    },
  );
});

test("apple answers the token and the code, and an answer missing either rejects with failed", async () => {
  let answer: unknown = { idToken: "jwt", authorizationCode: "code" };
  await inTauri(
    () => answer,
    async () => {
      assert.deepEqual(await apple({ nonce: "n1" }), { idToken: "jwt", authorizationCode: "code" });
      answer = { idToken: "jwt" };
      await assert.rejects(apple({ nonce: "n1" }), { code: "failed" });
      answer = null;
      await assert.rejects(apple({ nonce: "n1" }), { code: "failed" });
    },
  );
});

test("a rejection keeps its code and message, and anything else reads as failed", async () => {
  let thrown: unknown = { code: "cancelled", message: "closed" };
  await inTauri(
    () => {
      throw thrown;
    },
    async () => {
      await assert.rejects(google({ clientId: WEB }), { code: "cancelled", message: "closed" });
      thrown = { code: "no-account", message: "none" };
      await assert.rejects(google({ clientId: WEB, silent: true }), { code: "no-account" });
      thrown = { code: "1001", message: "odd" };
      await assert.rejects(apple({ nonce: "n1" }), { code: "failed", message: "odd" });
      thrown = "plain text";
      await assert.rejects(apple({ nonce: "n1" }), { code: "failed", message: "plain text" });
      assert.deepEqual(await available(), { google: false, apple: false }, "available never rejects");
    },
  );
});

test("available reads the platform's answer", async () => {
  await inTauri(
    () => ({ google: true, apple: false }),
    async () => {
      assert.deepEqual(await available(), { google: true, apple: false });
    },
  );
});
