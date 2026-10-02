# tauri-plugin-silent-mode-api

TypeScript facade for [`tauri-plugin-silent-mode`](https://github.com/hoangmirs/tauri-plugins): whether the phone is set to silent or vibrate, and when that changes.

```ts
import { isQuiet, onSilentModeChange, silentMode } from "tauri-plugin-silent-mode-api";

let quiet = isQuiet(await silentMode());
const stop = await onSilentModeChange((mode) => (quiet = isQuiet(mode)));
```

Only Android reports anything but `"normal"`. Outside Tauri, on iOS and on a desktop, `silentMode()` answers `"normal"` and the change listener never fires. Neither call ever rejects.
