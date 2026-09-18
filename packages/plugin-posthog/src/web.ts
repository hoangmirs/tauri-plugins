//! The plain-web half of the facade: no Tauri runtime underneath, so the
//! events this file queues never touch Rust. It keeps its own queue in
//! `localStorage`, its own install id, and sends batches with `fetch` —
//! same event shape as the Rust plugin's `event::batch_body` produces, so
//! `PostHog` sees identical payloads whichever side sent them.
//!
//! Every exported function is a silent no-op on any failure: a private
//! browsing `localStorage` that throws on every access, a `crypto` that is
//! missing `randomUUID`, or a `fetch` that rejects, all leave the caller
//! with a resolved promise and, at worst, an unsent queue.

/** What a caller passes to {@link init}. `host` defaults to `PostHog`'s US
 * cloud; `appVersion` — the app's own version, not this package's — is
 * omitted from events when not given. */
export type WebConfig = {
  apiKey: string;
  host?: string;
  appVersion?: string;
};

type Config = {
  apiKey: string;
  host: string;
  appVersion?: string;
};

/** One event as it sits in the queue: already fully shaped, since the web
 * path (unlike the Rust worker) has nothing left to add at flush time. */
type QueuedEvent = {
  uuid: string;
  event: string;
  timestamp: string;
  properties: Record<string, unknown>;
};

/** `PostHog`'s own name for this half of the plugin, as the Rust side spells
 * the native one (`tauri-plugin-posthog`) with `-web` appended. */
const LIB_NAME = "tauri-plugin-posthog-web";

/** Kept in sync with `package.json`'s `version` — a JSON import would need
 * `resolveJsonModule` plumbing that fights the `node --experimental-strip-types`
 * runner this package's tests run under, so a test instead asserts this
 * constant equals `package.json`'s `version`. */
const LIB_VERSION = "0.1.0-alpha.1";

const DEFAULT_HOST = "https://us.i.posthog.com";

/** Same bound as the Rust side's `event::MAX_EVENT_NAME_LEN`: a sanity
 * check on event names, not user-input validation. Counted in Unicode code
 * points, like Rust's `chars().count()`. */
const MAX_EVENT_NAME_LEN = 200;

/** Flush once this many events are queued — mirrors the Rust worker's
 * `flush_at`. */
const FLUSH_AT = 20;

/** How often the lazily-started timer flushes while the page is open. */
const FLUSH_EVERY_MS = 30_000;

/** The most events a single request carries — mirrors `send::BATCH`. */
const MAX_BATCH = 100;

/** The queue's limits — mirrors `queue::Limits::default()`: above either,
 * the oldest events are dropped first. */
const MAX_QUEUE_EVENTS = 1_000;
const MAX_QUEUE_BYTES = 1_048_576;

const QUEUE_KEY = "posthog-plugin:queue";
const ID_KEY = "posthog-plugin:id";
const OPT_OUT_KEY = "posthog-plugin:opt-out";

/** How long a retry waits after a failed send — mirrors the Rust side's
 * `send::Backoff`: 30s on the first failure, doubling each subsequent one,
 * capped at 10 minutes; any success resets it. */
const INITIAL_BACKOFF_MS = 30_000;
const MAX_BACKOFF_MS = 600_000;

/** The default request timeout — mirrors the Rust side's `UreqPost`'s
 * `REQUEST_TIMEOUT`, so a POST that never answers is bounded here the same
 * way it is there. */
const DEFAULT_REQUEST_TIMEOUT_MS = 10_000;

/** Set by {@link init}; `undefined` means `track`/`flush` are no-ops, the
 * state a page that never called `init` (or isn't running under Tauri, and
 * imported this module only transitively) is in. */
let config: Config | undefined;

/** The lazily-started periodic flush, `undefined` until the first `track`
 * after `init`. */
let timer: ReturnType<typeof setInterval> | undefined;

/** Whether the `pagehide` listener has been registered yet, so it is only
 * ever added once. */
let pagehideRegistered = false;

/** The clock the backoff reads to decide whether a retry is due yet.
 * Swappable in tests (see {@link setClockForTests}) so backoff can be
 * exercised without a real wait; real code never touches this. */
let clock: () => number = () => Date.now();

/** The wait the *next* failure will record — doubles on each one, reset to
 * {@link INITIAL_BACKOFF_MS} by a success. */
let backoffNext = INITIAL_BACKOFF_MS;

/** When the most recent failure happened, and how long it decided to
 * wait — `undefined` once a success has reset it, or before any failure
 * at all, meaning a retry is due right now. */
let backoffWaitingSince: { at: number; wait: number } | undefined;

/** The chain every flush attempt joins: whichever trigger — the 20-event
 * threshold, the 30s timer, `pagehide`, or a manual `flush()` — calls
 * {@link performFlush} next appends itself to the tail, so at most one
 * `fetch` is ever in flight and a later attempt always sees whatever an
 * earlier one left in the queue, rather than racing it. */
let flushChain: Promise<void> = Promise.resolve();

/** How long a single POST is allowed to run before it's treated as a
 * failure. Swappable in tests (see {@link setRequestTimeoutMsForTests}) so
 * a stalled-request test needs no real 10s wait; real code never touches
 * this. */
let requestTimeoutMs = DEFAULT_REQUEST_TIMEOUT_MS;

/** How much longer, in milliseconds, a retry must wait — zero once the
 * last recorded wait has elapsed, or when there has been no failure since
 * the last success. */
function backoffReadyInMs(): number {
  if (backoffWaitingSince === undefined) return 0;
  const elapsed = clock() - backoffWaitingSince.at;
  return Math.max(0, backoffWaitingSince.wait - elapsed);
}

/** Records a failure now: the next attempt must wait `backoffNext`, which
 * then doubles (capped) for the failure after that. */
function backoffFailed(): void {
  backoffWaitingSince = { at: clock(), wait: backoffNext };
  backoffNext = Math.min(backoffNext * 2, MAX_BACKOFF_MS);
}

/** Resets the backoff: the next failure waits {@link INITIAL_BACKOFF_MS}
 * again, and a retry is due at once until then. */
function backoffSucceeded(): void {
  backoffNext = INITIAL_BACKOFF_MS;
  backoffWaitingSince = undefined;
}

/**
 * Configures the web path: the project API key, optionally a non-US (or
 * self-hosted) host, and the app's own version for `$app_version`. Under
 * Tauri this is never reached — the facade routes to Rust instead, which
 * already holds its own config — so calling it there would simply be
 * config nobody reads.
 */
export function init(webConfig: WebConfig): void {
  config = {
    apiKey: webConfig.apiKey,
    host: webConfig.host ?? DEFAULT_HOST,
    ...(webConfig.appVersion !== undefined ? { appVersion: webConfig.appVersion } : {}),
  };
}

/** `navigator.language`, when there is a `navigator` with one — never
 * throws, since a hostile or stubbed `navigator` must not break `track`. */
function browserLocale(): string | undefined {
  try {
    return typeof navigator === "undefined" ? undefined : navigator.language;
  } catch {
    return undefined;
  }
}

/**
 * Adds `$locale` from `navigator.language` unless `properties` already has
 * one — used on both the Tauri and the web path, since only the JS side
 * ever knows the browser's locale. Never mutates `properties`.
 */
export function addLocale(properties?: Record<string, unknown>): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...(properties ?? {}) };
  if (merged.$locale === undefined) {
    const locale = browserLocale();
    if (locale !== undefined) merged.$locale = locale;
  }
  return merged;
}

function readQueue(): QueuedEvent[] {
  const raw = localStorage.getItem(QUEUE_KEY);
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as QueuedEvent[]) : [];
  } catch {
    return [];
  }
}

function writeQueue(queue: QueuedEvent[]): void {
  localStorage.setItem(QUEUE_KEY, JSON.stringify(queue));
}

function byteLength(queue: QueuedEvent[]): number {
  return new TextEncoder().encode(JSON.stringify(queue)).length;
}

/** Drops the oldest events once the queue is past either limit — mirrors
 * `Queue::push`'s eviction, event-count first, then by size. */
function trimQueue(queue: QueuedEvent[]): QueuedEvent[] {
  let trimmed = queue;
  if (trimmed.length > MAX_QUEUE_EVENTS) {
    trimmed = trimmed.slice(trimmed.length - MAX_QUEUE_EVENTS);
  }
  while (trimmed.length > 0 && byteLength(trimmed) > MAX_QUEUE_BYTES) {
    trimmed = trimmed.slice(1);
  }
  return trimmed;
}

/** The install's own random id, made once and persisted — never the same
 * as any `distinct_id` a caller might try to pass in. */
function installId(): string {
  const existing = localStorage.getItem(ID_KEY);
  if (existing !== null) return existing;
  const id = crypto.randomUUID();
  localStorage.setItem(ID_KEY, id);
  return id;
}

function readOptOut(): boolean {
  return localStorage.getItem(OPT_OUT_KEY) === "true";
}

function writeOptOut(out: boolean): void {
  localStorage.setItem(OPT_OUT_KEY, out ? "true" : "false");
}

/** Merges `callerProperties` onto the base properties the same way the
 * Rust side's `merged_properties` does: base first, then the caller's own
 * (which may shadow most of them), then `distinct_id` and
 * `$process_person_profile` re-asserted last so neither can be
 * overridden. */
function mergedProperties(distinctId: string, callerProperties: Record<string, unknown>): Record<string, unknown> {
  const merged: Record<string, unknown> = {
    distinct_id: distinctId,
    $lib: LIB_NAME,
    $lib_version: LIB_VERSION,
    $os: "Web",
    platform: "web",
    $process_person_profile: false,
  };
  if (config?.appVersion !== undefined) {
    merged.$app_version = config.appVersion;
  }
  for (const [key, value] of Object.entries(callerProperties)) {
    merged[key] = value;
  }
  merged.distinct_id = distinctId;
  merged.$process_person_profile = false;
  return merged;
}

/** `clearInterval`/`clearTimeout` both keep Node alive until the handle
 * fires; `unref`'d, they don't. Browsers have no `unref`, so this is a
 * no-op there. */
function unref(handle: unknown): void {
  const fn = (handle as { unref?: () => void } | undefined)?.unref;
  if (typeof fn === "function") fn.call(handle);
}

/** Runs `fetch(url, init)`, but treats it as failed once
 * {@link requestTimeoutMs} passes — mirrors the Rust side's `UreqPost`,
 * which bounds every request the same way, so a chained flush (see
 * {@link performFlush}) can never be stuck behind a POST that neither
 * resolves nor rejects. Prefers `AbortController`, which actually cancels
 * the underlying request; a host with no `AbortController` instead races
 * the same timeout against the raw `fetch` and just ignores whatever it
 * eventually does. Either way this rejects on a timeout, exactly like a
 * network failure, which `runFlush` already treats as one. */
async function fetchWithTimeout(url: string, init: RequestInit): Promise<Response> {
  if (typeof AbortController === "function") {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), requestTimeoutMs);
    unref(timer);
    try {
      return await fetch(url, { ...init, signal: controller.signal });
    } finally {
      clearTimeout(timer);
    }
  }

  // No AbortController: the request itself cannot be cancelled, so this
  // races the timeout against it instead — the loser's outcome (a late
  // response, or a rejection after this has already timed out) is simply
  // never observed by `runFlush`.
  return new Promise<Response>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("request timed out")), requestTimeoutMs);
    unref(timer);
    fetch(url, init).then(
      (response) => {
        clearTimeout(timer);
        resolve(response);
      },
      (error: unknown) => {
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      },
    );
  });
}

/** Registers the `pagehide` flush, once, only where `window.addEventListener`
 * exists (never in Node, where there is no such event to fire). */
function registerPagehide(): void {
  if (pagehideRegistered) return;
  if (typeof window === "undefined" || typeof window.addEventListener !== "function") return;
  window.addEventListener("pagehide", () => {
    void performFlush(true);
  });
  pagehideRegistered = true;
}

/** Starts the periodic flush the first time it's needed — lazily, on the
 * first `track` after `init`, not on `init` itself, so a page that only
 * calls `init` and never `track`s never starts a timer. `unref`'d where
 * the host supports it (Node) so it never keeps a process alive on its
 * own. */
function ensureTimer(): void {
  registerPagehide();
  if (timer !== undefined) return;
  timer = setInterval(() => {
    void performFlush(false);
  }, FLUSH_EVERY_MS);
  unref(timer);
}

/**
 * Queues `event` with `properties`, unless there is no config yet (no
 * `init` was called), the install has opted out, or the event's name,
 * trimmed, is empty or over 200 characters — mirrors the Rust side's
 * `Event::new`. Triggers a flush, without waiting for it, once 20 events
 * are waiting. Never rejects: a throwing `localStorage` or `crypto` leaves
 * this a no-op.
 */
export async function track(event: string, properties?: Record<string, unknown>): Promise<void> {
  try {
    if (config === undefined) return;
    if (readOptOut()) return;
    const name = event.trim();
    if (name === "" || [...name].length > MAX_EVENT_NAME_LEN) return;

    const id = installId();
    const queued: QueuedEvent = {
      uuid: crypto.randomUUID(),
      event: name,
      timestamp: new Date().toISOString(),
      properties: mergedProperties(id, addLocale(properties)),
    };

    const queue = trimQueue([...readQueue(), queued]);
    writeQueue(queue);
    ensureTimer();
    // The backoff precheck (rather than always chaining, then letting
    // `runFlush` decide) keeps a long backoff from piling up one chain
    // entry per `track` call — the queue may sit above `FLUSH_AT` for a
    // long time while a retry is owed.
    if (queue.length >= FLUSH_AT && backoffReadyInMs() === 0) {
      void performFlush(false);
    }
  } catch {
    // a throwing localStorage or crypto makes track a silent no-op
  }
}

/** The actual work of one flush attempt: only ever run one at a time, in
 * turn, via {@link performFlush}'s chain — never called directly. Honours
 * the backoff (no request at all while one is owed), sends at most
 * `MAX_BATCH` events to `{host}/batch/`, and on a 2xx response removes
 * exactly the events that were sent — by `uuid`, from a *fresh* read of
 * the queue, so events queued while this request was in flight (by a
 * `track` that ran between this attempt starting and its response
 * arriving) are never discarded. A failed or throwing `fetch`, or a
 * non-2xx response, leaves the queue untouched and starts (or extends)
 * the backoff; any success resets it. */
async function runFlush(keepalive: boolean): Promise<void> {
  try {
    if (config === undefined) return;
    if (backoffReadyInMs() > 0) return; // still waiting out a previous failure
    const waiting = readQueue();
    if (waiting.length === 0) return;
    const batch = waiting.slice(0, MAX_BATCH);

    const url = `${config.host.replace(/\/+$/, "")}/batch/`;
    const body = JSON.stringify({ api_key: config.apiKey, batch });

    let response: Response;
    try {
      response = await fetchWithTimeout(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body,
        keepalive,
      });
    } catch {
      // a throw here is a transport failure, a non-2xx `ureq`-style
      // rejection never happens on the web (fetch resolves on any status),
      // or — the case this exists for — a request that timed out
      backoffFailed();
      return;
    }

    if (!response.ok) {
      backoffFailed();
      return;
    }

    backoffSucceeded();
    const sentIds = new Set(batch.map((event) => event.uuid));
    const stillWaiting = readQueue();
    writeQueue(stillWaiting.filter((event) => !sentIds.has(event.uuid)));
  } catch {
    // a throwing localStorage (or anything else unexpected) leaves the
    // queue untouched, same as a failed network
  }
}

/** The one entry point both `flush()` and the timer/`pagehide` paths use.
 * Never overlaps another attempt: this call joins the tail of
 * {@link flushChain}, so it runs only once every earlier-requested attempt
 * has finished, and resolves only once its own turn has run. */
function performFlush(keepalive: boolean): Promise<void> {
  const attempt = flushChain.then(() => runFlush(keepalive));
  flushChain = attempt;
  return attempt;
}

/** Sends whatever is queued now. Never rejects: a failed or throwing
 * `fetch` (or `localStorage`) leaves the queue as it was. */
export async function flush(): Promise<void> {
  await performFlush(false);
}

/** Opts the install out (clearing anything queued) or back in, and
 * persists the choice across launches. Never rejects. */
export async function setOptOut(out: boolean): Promise<void> {
  try {
    writeOptOut(out);
    if (out) {
      writeQueue([]);
    }
  } catch {
    // a throwing localStorage makes this a silent no-op
  }
}

/** Whether the install has opted out. Resolves `false`, never rejects,
 * when a throwing `localStorage` makes that impossible to tell. */
export async function isOptedOut(): Promise<boolean> {
  try {
    return readOptOut();
  } catch {
    return false;
  }
}

/** Test-only: overrides the clock the backoff reads, so a test can make a
 * retry due without a real wait. Not exported from the package's public
 * entry. */
export function setClockForTests(fn: () => number): void {
  clock = fn;
}

/** Test-only: overrides how long a single POST is allowed to run before
 * it's treated as a failure, so a stalled-request test needs no real 10s
 * wait. Not exported from the package's public entry. */
export function setRequestTimeoutMsForTests(ms: number): void {
  requestTimeoutMs = ms;
}

/** Test-only: waits for every currently chained flush attempt to finish
 * (so none of it leaks into whatever the next test does), then clears the
 * timer, the `pagehide` registration flag, the in-memory config, the
 * backoff, the clock override, and the request-timeout override — the
 * state a freshly loaded page would be in. Not exported from the
 * package's public entry (`index.ts`) — tests import this module
 * directly, and must `await` this (a pending flush attempt is real async
 * work, not something a synchronous reset can discard). */
export async function resetForTests(): Promise<void> {
  await flushChain.catch(() => {
    // `runFlush` never rejects, but this drain must not throw either way
  });
  if (timer !== undefined) {
    clearInterval(timer);
    timer = undefined;
  }
  pagehideRegistered = false;
  config = undefined;
  clock = () => Date.now();
  backoffNext = INITIAL_BACKOFF_MS;
  backoffWaitingSince = undefined;
  flushChain = Promise.resolve();
  requestTimeoutMs = DEFAULT_REQUEST_TIMEOUT_MS;
}
