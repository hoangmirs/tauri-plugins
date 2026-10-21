# Ads plugin: the cases 0.1.0-alpha.1 does not cover

Date: 2026-10-21. Status: approved by the owner on 2026-10-21 ("yes" to the
plan). Builds on PR #17, which clips the iOS card to the visible page.

## Why

`tauri-plugin-ads` 0.1.0-alpha.1 shows one native card and one rewarded ad.
Ra Chơi found that the iOS card drew over a top bar that the page pins. A
review of the whole plugin then found the cases below. Each one loses money,
breaks a Google rule, or shows the wrong thing.

The owner wants the plugin to cover every case, so that later apps get them
without a second round of fixes.

## Terms

| Term | Meaning |
|---|---|
| Card | The native ad view the plugin draws over the box the page draws. |
| Slot | The page's box for the card, sent with `showNative`. |
| Show | A `showNative` call. |
| Hide | A `hideNative` call. |
| Visible part | The part of the page's scroll box between `clipTop` and `clipBottom`. |
| Impression | Google's `native_shown` event for the current card ad. |
| Stale | An ad loaded more than 60 minutes ago. |
| Privacy options | Google's form where a user in a consent region changes a past answer. |

## Findings and changes

Each item names what is wrong, then the change. "P0" loses money or breaks a
Google rule. "P1" shows the wrong thing. "P2" is reporting or polish.

### 1. A tap on the iOS card opens nothing (P0)

WebKit's own hit test never returns a view that WebKit did not make. A tap on
the card went to the page under it, and the ad never opened. Seen on the iOS
Simulator with 0.1.0-alpha.1 and with PR #17.

Change, done in commit `3b23897`:

1. Add a clear view over the web view, as large as the web view.
2. Give the view a hit test that returns the card's own view for a point on
   the card's visible part, and nothing for any other point.

UIKit then sends the touch to the card. The card stays inside the scroll box,
so a drag that starts on the card still scrolls the page. On the Simulator a
tap now opens Google's App Store sheet.

WebKit still sees the tap and clicks the page under the card. The box under
the card must therefore react to no tap. Ra Chơi's box does not. The README
must say so.

Android's card already sits over the web view and takes its own taps. Check
this on a device or an emulator.

### 2. No way to change consent later (P0)

Google requires an entry point to the privacy options form when
`privacyOptionsRequirementStatus` is `required`. That is the case in the EEA
and the UK. The plugin has no command for it.

Change:

1. Add `privacy_options` → `"required" | "not_required"`. It reads the status
   after `consent`. Outside a phone it answers `"not_required"`.
2. Add `show_privacy_options` → `"shown" | "failed"`. iOS calls
   `ConsentForm.presentPrivacyOptionsForm(from:)`. Android calls
   `UserMessagingPlatform.showPrivacyOptionsForm`.

The app shows a row for the form only when the answer is `"required"`.

### 3. One failure stops ads for the session (P0)

1. The native loader loads once. After `native_failed` it never tries again.
2. A consent request that fails, for example offline, answers `"failed"`.
   This is so even when `canRequestAds` is still true from an earlier session.

Change:

1. After a native load fails, load again after 30 s, 2 min, then every
   10 min. Send `native_failed` after each failure and `native_loaded` after a
   success. Stop when the app sends no show for 30 min.
2. When the consent request fails, answer `"obtained"` if `canRequestAds` is
   true. Otherwise answer `"failed"`.

### 4. The card shows one ad for the whole session (P1)

The loader never loads a second ad. An ad that stays on screen for hours can
stop counting impressions.

Change: a show after a hide loads a new ad when one of these is true:

1. The current ad recorded an impression at least 60 s ago.
2. The current ad is stale.

The current ad stays on the card until the new one arrives. Then the plugin
destroys the old ad. On Android this is `NativeAd.destroy()`. On iOS the
plugin drops its last reference.

### 5. A rewarded ad never expires (P1)

`load_rewarded` answers `"ready"` for an ad loaded hours earlier. Google tells
apps not to show a rewarded ad loaded more than an hour ago.

Change:

1. Keep the time each rewarded ad loads.
2. `load_rewarded` drops a stale ad and loads a new one.
3. `show_rewarded` with a stale ad drops it, answers `"failed"`, and loads a
   new one.

### 6. A new ad unit is ignored (P1)

The loader keeps the first `unitId` it gets. A new unit from the app's remote
settings never applies.

Change: a show or a `load_rewarded` with a different unit drops the old loader
and its ad, then loads from the new unit.

### 7. The card flashes over the screen the player leaves (P1)

The app sends a show before WebKit paints the new screen. The card appears for
a frame over the old screen. Seen on the iOS Simulator on the way back to
Home.

Change, in the app: send the first show of a slot two animation frames after
it mounts.

### 8. One card for the whole app (P1)

The plugin has one card. Two slots on screen at once would move the card
between them. Ra Chơi's Get games has two places, in two views that do not
show together. Nothing enforces this.

Change, in the app: the first slot that mounts owns the card. A second slot
takes no room and sends no show until the first one unmounts.

### 9. The SDK starts before consent (P1)

`init` starts the SDK, and the app asks for consent after that. Google's order
is consent first, then `MobileAds.start` once `canRequestAds` is true.

Change: `init` sets the request configuration only. `consent` starts the SDK
once, when its answer is `"obtained"` or `"not_required"`.

### 10. No ad revenue reaches the app (P2)

The plugin never sets Google's paid event handler. The app cannot count
revenue.

Change: set the handler on each native and rewarded ad. Send the event `paid`
on the `ads` channel:

```json
{ "format": "native", "micros": 1200, "currency": "USD", "precision": "estimated" }
```

`format` is `native` or `rewarded`. `precision` is `unknown`, `estimated`,
`publisher` or `precise`.

The app decides whether to count the event. Counting it changes what the app
collects, so the privacy policy and the store answers change with it.

### 11. Rewards are granted on the phone only (P2, not in this change)

There is no server-side verification. A changed client could take a reward
without an ad. Ra Chơi's reward is stars that buy hints, so the risk is low.
Add verification when a reward has money value.

### 12. Ad sound ignores the app's mute (P2)

Change: add `set_muted(muted: bool)`. iOS sets
`MobileAds.shared.isApplicationMuted`. Android calls `MobileAds.setAppMuted`.
The app sends its own sound setting.

### 13. The card's text keeps fixed sizes (P2, not in this change)

The card does not follow the system text size or the app's text size, and it
uses the system font. This needs a design pass. Leave it for later.

### 14. Android Activity recreation (no change)

Ra Chơi's Android Activity declares `uiMode`, `locale`, `screenSize` and the
other common changes in `configChanges`. The Activity is not recreated, so the
cached web view stays valid. An app that drops these entries must say so in
its own code.

## API after this change

| Command | Answer | New |
|---|---|---|
| `init(settings)` | nothing | changed: starts nothing |
| `consent()` | `obtained`, `not_required`, `failed` | changed: falls back to `canRequestAds`, starts the SDK |
| `privacy_options()` | `required`, `not_required` | yes |
| `show_privacy_options()` | `shown`, `failed` | yes |
| `show_native(slot)` | nothing | changed: retry, refresh, new unit |
| `hide_native()` | nothing | no |
| `load_rewarded(unitId)` | `ready`, `failed` | changed: stale ad, new unit |
| `show_rewarded()` | `earned`, `closed`, `failed` | changed: stale ad |
| `set_muted(muted)` | nothing | yes |

| Event on `ads` | Data | New |
|---|---|---|
| `native` | `{ event }` | no |
| `paid` | `{ format, micros, currency, precision }` | yes |

The release is 0.1.0-alpha.2, on crates.io and npm.

## App changes in Ra Chơi

1. Pin `tauri-plugin-ads` and `tauri-plugin-ads-api` to 0.1.0-alpha.2.
2. Me → Privacy: show an "Ad privacy" row when `privacy_options` answers
   `"required"`. The row calls `show_privacy_options`. Both languages, and no
   vendor name, as `CLAUDE.md` asks.
3. `adsNative.ts`: after a `"failed"` consent, ask again on the next show, at
   most once a minute. Let `native_loaded` after `native_failed` bring the
   slot back.
4. `useAdSlot.ts`: send the first show two frames after mount (item 7). Let
   only one slot own the card (item 8).
5. Send the sound setting with `set_muted` at start and on each change.
6. Count `paid` as `ad_paid` only after the owner approves it. Then update the
   policy in both languages, `/privacy`, `PrivacyInfo.xcprivacy`, and the
   console answers in `docs/store/listing.md`, all in the same pull request.

## How to check

1. `cargo test -p tauri-plugin-ads`, `cargo clippy`, `cargo fmt --check`.
2. `npm test` and `npm run typecheck` in `packages/plugin-ads`.
3. Build the Swift package for the iOS Simulator with `xcodebuild`.
4. Build the Android library with Gradle.
5. Run Ra Chơi staging on the iOS Simulator with the plugin patched in by
   path. Check items 1, 3, 4, 7 and 8 there, and the clip from PR #17.
6. Run Ra Chơi staging on an Android emulator or phone, if one is available.
   Check items 1 to 4.
7. Run AdMob's native validator on each card. It must report no issues.
