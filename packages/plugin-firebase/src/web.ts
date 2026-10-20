//! The web half of the facade, for a browser and for a Tauri desktop app,
//! which has no native Firebase. It loads the Firebase web SDK with a
//! dynamic `import()`, so an app that never takes this path never downloads
//! it.
//!
//! Every function here answers from the defaults when the SDK cannot load or
//! throws. A private window, whose IndexedDB throws, still gets them.

export type Source = "remote" | "default" | "static";
export type Entry = { value: string; source: Source };
export type Fetched = "fetched" | "throttled" | "failed";
export type Params = Record<string, string | number>;
export type Defaults = Record<string, string | number | boolean>;
export type WebConfig = { options: Record<string, string>; minimumFetchIntervalMillis: number };

type RemoteConfig = {
  settings: { minimumFetchIntervalMillis: number; fetchTimeoutMillis: number };
  defaultConfig: Defaults;
};

/** The part of the Firebase web SDK this file uses. */
export type Sdk = {
  initializeApp(options: Record<string, string>): unknown;
  getRemoteConfig(app: unknown): RemoteConfig;
  fetchConfig(rc: RemoteConfig): Promise<void>;
  activate(rc: RemoteConfig): Promise<boolean>;
  getAll(rc: RemoteConfig): Record<string, { asString(): string; getSource(): Source }>;
  isSupported(): Promise<boolean>;
  initializeAnalytics(app: unknown, options: { config: { send_page_view: boolean } }): unknown;
  logEvent(analytics: unknown, name: string, params?: Params): void;
  setAnalyticsCollectionEnabled(analytics: unknown, enabled: boolean): void;
  onConfigUpdate(rc: RemoteConfig, observer: UpdateObserver): () => void;
};

type UpdateObserver = {
  next(update: { getUpdatedKeys(): Set<string> }): void;
  error(e: unknown): void;
  complete(): void;
};

export type Loader = () => Promise<Sdk>;

const loadFirebase: Loader = async () => {
  const [app, rc, analytics] = await Promise.all([
    import("firebase/app"),
    import("firebase/remote-config"),
    import("firebase/analytics"),
  ]);
  return { ...app, ...rc, ...analytics } as unknown as Sdk;
};

type State = {
  configured: boolean;
  defaults: Defaults;
  sdk?: Sdk;
  app?: unknown;
  rc?: RemoteConfig;
  analytics?: unknown;
  collecting: boolean;
  started?: Promise<void>;
};

let state: State = { configured: false, defaults: {}, collecting: false };

export const isConfigured = (): boolean => state.configured;

/** Starts one Firebase app. A second call does nothing. */
export async function init(config: WebConfig, loader: Loader = loadFirebase): Promise<void> {
  if (state.configured) return state.started;
  state.configured = true;
  state.started = (async () => {
    try {
      const sdk = await loader();
      state.sdk = sdk;
      state.app = sdk.initializeApp(config.options);
      const rc = sdk.getRemoteConfig(state.app);
      rc.settings.minimumFetchIntervalMillis = config.minimumFetchIntervalMillis;
      rc.defaultConfig = state.defaults;
      state.rc = rc;
    } catch {
      // No SDK, no IndexedDB: every answer comes from the defaults.
    }
  })();
  return state.started;
}

export async function setDefaults(defaults: Defaults): Promise<void> {
  state.defaults = defaults;
  await state.started;
  if (state.rc) state.rc.defaultConfig = defaults;
}

export async function fetch(): Promise<Fetched> {
  await state.started;
  const { sdk, rc } = state;
  if (!sdk || !rc) return "failed";
  try {
    await sdk.fetchConfig(rc);
    return "fetched";
  } catch (e) {
    return (e as { code?: unknown })?.code === "remoteconfig/fetch-throttle" ? "throttled" : "failed";
  }
}

export async function activate(): Promise<boolean> {
  await state.started;
  const { sdk, rc } = state;
  if (!sdk || !rc) return false;
  try {
    return await sdk.activate(rc);
  } catch {
    return false;
  }
}

/** Calls `on` after each publish the SDK fetched. With no SDK, or when the
 *  SDK refuses, nothing is heard and the stop does nothing. */
export async function listen(on: (keys: string[]) => void): Promise<() => void> {
  await state.started;
  const { sdk, rc } = state;
  if (!sdk || !rc) return () => {};
  try {
    return sdk.onConfigUpdate(rc, {
      next: (update) => {
        try {
          on([...update.getUpdatedKeys()]);
        } catch {
          /* the app's handler failed; the stream goes on */
        }
      },
      error: () => {},
      complete: () => {},
    });
  } catch {
    return () => {};
  }
}

export const defaultsView = (defaults: Defaults): Record<string, Entry> =>
  Object.fromEntries(Object.entries(defaults).map(([k, v]) => [k, { value: String(v), source: "default" as const }]));

export async function getAll(): Promise<Record<string, Entry>> {
  await state.started;
  const { sdk, rc } = state;
  if (sdk && rc) {
    try {
      const all = sdk.getAll(rc);
      return Object.fromEntries(
        Object.entries(all).map(([k, v]) => [k, { value: v.asString(), source: v.getSource() }]),
      );
    } catch {
      /* the defaults below */
    }
  }
  return defaultsView(state.defaults);
}

/** Analytics starts only once collection is on, so nothing is sent first. */
export async function setCollection(enabled: boolean): Promise<void> {
  await state.started;
  state.collecting = enabled;
  const { sdk, app } = state;
  if (!sdk || !app) return;
  try {
    if (enabled && state.analytics === undefined && (await sdk.isSupported())) {
      // No automatic page view: it carries the page's address and the full
      // referring address, which an app's own counts may promise to keep out.
      state.analytics = sdk.initializeAnalytics(app, { config: { send_page_view: false } });
    }
    if (state.analytics !== undefined) sdk.setAnalyticsCollectionEnabled(state.analytics, enabled);
  } catch {
    /* Analytics stays off */
  }
}

export async function logEvent(name: string, params: Params): Promise<void> {
  await state.started;
  const { sdk, analytics } = state;
  if (!state.collecting || !sdk || analytics === undefined) return;
  try {
    sdk.logEvent(analytics, name, params);
  } catch {
    /* dropped */
  }
}

export async function resetForTests(): Promise<void> {
  await state.started;
  state = { configured: false, defaults: {}, collecting: false };
}
