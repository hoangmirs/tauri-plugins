# TypeScript facade

The framework-neutral frontend API for the posthog Tauri plugin. Call
`init()` once on the web (a no-op under Tauri, where Rust already holds the
project key), then `track()`, `flush()`, `setOptOut()` and `isOptedOut()`
however you like. Under Tauri these call the plugin's commands; on the plain
web they queue events in `localStorage` and send them with `fetch`, in
exactly the same shape either way. Every function resolves and never
rejects — analytics never breaks the app.
