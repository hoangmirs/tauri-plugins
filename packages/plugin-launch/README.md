# tauri-plugin-launch-api

The page's side of `tauri-plugin-launch`. Call `ready()` once the page has
painted its own launch frame, and the native frame over the web view goes.

```ts
import { ready } from "tauri-plugin-launch-api";

requestAnimationFrame(() => void ready());
```
