# tauri-plugin-firebase-api

The TypeScript side of `tauri-plugin-firebase`. The same functions run on a
phone, where they call the plugin, and on the web or a desktop, where they use
the Firebase web SDK. The crash functions act on a phone only and do nothing
elsewhere. The push functions (`pushToken`, `pushTokenAnswer`, `deletePushToken`, `onPushToken`,
`takePushData`, `onPushData`, `onPushArrived`) act on iOS and Android only: elsewhere they
answer `null`, `[]`, the problem `unsupported` or a stop that does nothing. No function ever rejects. See the repository README for
setup.
