/** When read values replace the values the app shows, and when a test group
 *  is reported. A read that would change the screen under the user waits for
 *  the moment the app's mode allows. A test group is reported only when its
 *  value applies, so a user counts in group B from the moment B shows. */

import type { Assignment, Mode, Part, Saved, Used } from "./types.ts";

export type Snapshot = { saved: Saved; used: Used; assignments: Assignment[] };

export type Applier = {
  applied(): Snapshot | null;
  pending(): Snapshot | null;
  landed(s: Snapshot): Promise<void>;
  setVisible(visible: boolean): Promise<void>;
  setAtRest(atRest: boolean): Promise<void>;
  applyNow(keys?: string[]): Promise<void>;
  holding(): void;
  holdEnded(): Promise<void>;
};

function changedKeys(before: Saved | undefined, after: Saved): string[] {
  const out = new Set<string>();
  for (const part of ["global", "source"] as const) {
    const a: Part = before?.[part] ?? {};
    const b = after[part];
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) if (a[k] !== b[k]) out.add(k);
  }
  return [...out];
}

function withKeys(part: Part, from: Part, keys: string[]): Part {
  const out = { ...part };
  for (const k of keys) {
    const v = from[k];
    if (v === undefined) delete out[k];
    else out[k] = v;
  }
  return out;
}

export function createApplier(o: {
  start: Snapshot | null;
  mode: () => Mode;
  beforeApply?: (s: Snapshot) => Promise<Snapshot>;
  onApply: (s: Snapshot, changed: string[]) => void;
  onExperiment: (experiment: string, variant: string) => void;
}): Applier {
  let applied = o.start;
  let pending: Snapshot | null = null;
  let visible = true;
  // Null until the app first reports: an app that never does waits as in `hidden`.
  let atRest: boolean | null = null;
  let holding = false;
  const reported = new Set<string>();
  let queue: Promise<void> = Promise.resolve();

  const report = (assignments: Assignment[]) => {
    for (const a of assignments) {
      const id = `${a.experiment}\u0000${a.variant}`;
      if (reported.has(id)) continue;
      reported.add(id);
      o.onExperiment(a.experiment, a.variant);
    }
  };

  const serial = (fn: () => Promise<void> | void): Promise<void> => {
    queue = queue.then(fn, fn);
    return queue;
  };

  const applyAll = async () => {
    if (!pending) return;
    let next = pending;
    pending = null;
    if (o.beforeApply) next = await o.beforeApply(next);
    const changed = changedKeys(applied?.saved, next.saved);
    applied = next;
    report(next.assignments);
    o.onApply(next, changed);
  };

  const mayApply = () => {
    const mode = o.mode();
    return holding || !visible || mode === "now" || (mode === "rest" && atRest === true);
  };

  if (applied) report(applied.assignments);

  return {
    applied: () => applied,
    pending: () => pending,
    landed: (s) =>
      serial(async () => {
        pending = s;
        if (mayApply()) await applyAll();
      }),
    setVisible: (v) =>
      serial(async () => {
        visible = v;
        if (!v) await applyAll();
      }),
    setAtRest: (r) =>
      serial(async () => {
        atRest = r;
        if (r && o.mode() !== "hidden") await applyAll();
      }),
    applyNow: (keys) =>
      serial(async () => {
        if (!pending) return;
        if (!keys) {
          await applyAll();
          return;
        }
        const base: Snapshot = applied ?? {
          saved: { ...pending.saved, global: {}, source: {} },
          used: pending.used,
          assignments: [],
        };
        const next: Snapshot = {
          ...base,
          saved: {
            ...base.saved,
            global: withKeys(base.saved.global, pending.saved.global, keys),
            source: withKeys(base.saved.source, pending.saved.source, keys),
          },
        };
        const changed = changedKeys(applied?.saved, next.saved);
        applied = next;
        report(pending.assignments.filter((a) => keys.includes(a.key)));
        o.onApply(next, changed);
      }),
    holding: () => {
      holding = true;
    },
    holdEnded: () =>
      serial(async () => {
        holding = false;
        await applyAll();
      }),
  };
}
