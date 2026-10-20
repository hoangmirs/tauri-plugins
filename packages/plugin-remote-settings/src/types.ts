export type Mode = "hidden" | "rest" | "now";
export type SourceName = "growthbook" | "firebase";
/** Where one applied value came from. `saved` is a value from an earlier launch. */
export type From = "global" | "growthbook" | "firebase" | "saved" | "default";
/** Where this launch's applied values came from, as a whole. */
export type Used = "growthbook" | "firebase" | "saved" | "default";

/** One setting the app reads. `read` gives `undefined` for text that does not read. */
export type Key<T> = { fallback: T; read: (text: string) => T | undefined; global?: boolean };
export type Keys = Record<string, Key<unknown>>;
export type Values<K extends Keys> = { [N in keyof K]: K[N] extends Key<infer T> ? T : never };

/** Key to text, the way Remote Config gives values. */
export type Part = Record<string, string>;
export type Saved = { global: Part; source: Part; from: SourceName; country: string | null };
export type Files = { global: string | null; rules: string | null; country: string | null };
export type Assignment = { key: string; experiment: string; variant: string };
export type Store = { get(name: string): Promise<string | null>; set(name: string, text: string): Promise<void> };
