import { addPluginListener, invoke, isTauri } from "@tauri-apps/api/core";

/** A kind of reading a device can send. */
export type Kind = "motion" | "heading";

export type Vec3 = [number, number, number];

/** Gravity `g` and user acceleration `a` in g, rotation rate `r` in rad/s,
 * all in the device's natural frame with CoreMotion's signs: a phone lying
 * face up reads `g` of about `[0, 0, -1]` on both systems. `at` is in
 * milliseconds on a clock that only counts up, for comparing readings. */
export type MotionReading = { kind: "motion"; g: Vec3; r: Vec3; a: Vec3; at: number };

/** Degrees from magnetic north, 0 to 360, that the top of the device points. */
export type HeadingReading = { kind: "heading"; deg: number; at: number };

export type Reading = MotionReading | HeadingReading;

const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);

const isVec3 = (v: unknown): v is Vec3 => Array.isArray(v) && v.length === 3 && v.every(isNumber);

/** A reading from the other side of the bridge, checked. Anything that is not
 * one of the two shapes is `null`. */
export function readingOf(raw: unknown): Reading | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (!isNumber(r.at)) return null;
  if (r.kind === "motion" && isVec3(r.g) && isVec3(r.r) && isVec3(r.a))
    return { kind: "motion", g: r.g, r: r.r, a: r.a, at: r.at };
  if (r.kind === "heading" && isNumber(r.deg)) return { kind: "heading", deg: r.deg, at: r.at };
  return null;
}

/** The kinds this device can send. Outside Tauri, on a desktop, or when the
 * command cannot be reached, none; this promise never rejects. */
export async function available(): Promise<Kind[]> {
  if (!isTauri()) return [];
  try {
    const answer = await invoke<{ kinds?: unknown }>("plugin:sensors|available");
    const kinds = Array.isArray(answer?.kinds) ? answer.kinds : [];
    return kinds.filter((k): k is Kind => k === "motion" || k === "heading");
  } catch {
    return [];
  }
}

/** Starts one kind at `hz` readings a second, held to 1…60. Each reading
 * reaches `onReading`. Starting a kind that runs changes its rate. */
export async function start(kind: Kind, hz: number): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:sensors|start", { kind, hz });
  } catch {
    /* no readings come */
  }
}

/** Stops one kind. Stopping a kind that does not run does nothing. */
export async function stop(kind: Kind): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:sensors|stop", { kind });
  } catch {
    /* the kind was not running */
  }
}

/** Holds the screen at the orientation it has now, until `unlockRotation`. */
export async function lockRotation(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:sensors|lock_rotation");
  } catch {
    /* the screen keeps turning */
  }
}

/** Lets the screen turn as it did before `lockRotation`. */
export async function unlockRotation(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:sensors|unlock_rotation");
  } catch {
    /* the screen was not held */
  }
}

/** Calls `onReading` with each reading of every started kind, until the
 * returned function is called. Outside Tauri it never calls, and the function
 * does nothing; this promise never rejects. */
export async function onReading(onReading: (reading: Reading) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const listener = await addPluginListener("sensors", "reading", (raw: unknown) => {
      const reading = readingOf(raw);
      if (reading) onReading(reading);
    });
    return () => {
      listener.unregister().catch(() => {});
    };
  } catch {
    return () => {};
  }
}
