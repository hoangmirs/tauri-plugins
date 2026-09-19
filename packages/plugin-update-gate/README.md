# TypeScript facade

The framework-neutral frontend API for the update-gate Tauri plugin. Call
`checkUpdate()` to find out whether the installed copy is too old to run.
Outside Tauri — the plain web, or a build without the plugin registered —
it always resolves to the open gate, since the web always serves the newest
code.
