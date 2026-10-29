# tauri-plugin-sign-in-api

The TypeScript side of `tauri-plugin-sign-in`. `available` reads which
sign-in sheets the phone has. `google` and `apple` show the phone's own sheet
and answer the provider's id token, or reject with `{ code, message }`. See
the repository README for setup.
