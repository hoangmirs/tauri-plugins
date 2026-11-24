# tauri-plugin-sensors-api

TypeScript facade for [`tauri-plugin-sensors`](https://github.com/hoangmirs/tauri-plugins): a phone's motion and compass heading, and a hold on the screen's rotation.

```ts
import { available, onReading, start, stop } from "tauri-plugin-sensors-api";

if ((await available()).includes("motion")) {
  const stopListening = await onReading((reading) => {
    if (reading.kind === "motion") console.log(reading.g);
  });
  await start("motion", 30);
}
```

Outside Tauri and on a desktop, `available()` answers `[]`, the other calls do nothing, and no reading comes. No call ever rejects.
