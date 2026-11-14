# DataBeam for Android

Native Android alpha, isolated from the desktop application. Open this directory in Android Studio.

## Build on this PC

Android Studio: `X:/as`, SDK: `X:/asproj`. `local.properties` is local and ignored.

```powershell
.\scripts\build-apk.ps1
```

Requires Rust with `aarch64-linux-android`, Go (the recipe selects its toolchain), SDK API 36, and NDK 28.2.13676358. This initial APK targets ARM64 phones. Android Croc is independently pinned in `croc-engine.json` to the previously validated 11.5.5 base plus local GUI telemetry. Desktop 0.8.0 uses Croc 11.6.5; its new bundle publication requires hard links that failed in this Android app sandbox, so that upgrade is not shipped on the phone. The build refuses to package missing native engines. Native libraries and tool downloads are ignored by Git. Gradle and Android tooling may require accepting their licenses in SDK Manager.

## Implemented

- Native Kotlin Android UI; no WebView.
- Errors appear directly in transfer status. No visible log pane; received Croc text has a separate selectable view.
- Manual transfer Resume/Retry only. No general automatic transfer retries or tray UI. Remote restart of stopped senders is deferred; public-relay recovery remains enabled.
- Embedded Rust Sendme engine from the same vendored source as desktop via JNI.
- The pinned, SHA-256-verified, patched Croc CLI built as an Android PIE executable, packaged under jniLibs and executed from Android's installed native library directory (never extracted into a writable directory).
- Android Croc uses upstream's `croc_no_tailcat` build tag: standard LAN Direct and public-relay transfers are available; the optional Tailcat/DERP transport is excluded because its Linux netlink-based probe crashes inside Android's app sandbox.
- Croc files/text, Sendme sharing/receiving, and EasySendMe ticket handoff.
- Fresh CameraX + bundled ML Kit receiver scanner (full portrait camera frames, rotation/lifecycle handling, tap focus, pinch zoom and torch). Sender QR generation for codes/tickets, with stale QR dialogs closed on relay recovery. Receiver camera scanning is embedded in the app and fills the code field without automatically starting a transfer; camera permission is requested when scanning.
- Native system file and folder pickers, “Send with DataBeam” in Android’s share sheet and selected-text menu, background staging into private persistent storage, and automatic publication to Downloads/DataBeam after receipt. Sharing files keeps attachments rather than treating their captions as transfer text. Croc keeps its existing file layout; no extra DataBeam wrapper is added.
- Ongoing foreground transfer notification with phase, progress and live rate, Stop and tap-to-return. It is removed after completion/stop. Preparation and payload progress are separate; all modes show selection-wide byte totals and ETA when known. Croc includes completed-file counts and explicit verification reporting. Cooperative Sendme cancellation retains partial caches; the wake lock is bounded to six hours.
- Public Croc relay recovery: bounded background diagnostics, visible auto-troubleshooting status, sender replacement codes with a warning to reshare, and a Sendme fallback when no public relay is reachable. Receiver codes and explicit custom-relay settings are preserved; payloads are not automatically replayed.
- Separate Send and Receive screens, engine descriptions, contextual sharing controls, and a single progress card.
- Settings cog for advanced Croc relay configuration, a persistent download-folder picker, and manual Croc/Sendme public-relay checks. Croc checks TCP reachability of all four public endpoints; Sendme establishes an Iroh relay connection. These are checks from this device, not global service-health guarantees.
- Battery-optimization and notification status with links to Android controls. Home shows guidance only when a setting needs attention; the idle transfer card is hidden.
- Actual payload-path indicator (Direct/Relay). Native Sendme/EasySendMe paths update as Iroh changes the selected path; multiple sender connections are tracked separately. Croc uses the engine's direction evidence, independently of relay configuration. EasySendMe's Croc ticket handoff does not determine its payload-path label.
- Persistent custom sharing codes, a Clear field action, and five recent successful short codes per engine/direction with tap-to-fill and separate history clearing.
- Saved transfer state with Pause/Resume and replacement-code/ticket controls. A replacement ticket must have the original collection hash. One interrupted session is retained; starting a new transfer replaces its card.
- Sender stays available until Stop, including after a receiver disconnects. EasySendMe performs one Croc ticket handoff per share session; a replacement short-code exchange requires Share again on the sender.

## Resume behavior

Use Pause and Resume while the original sender is still sharing. Sendme and EasySendMe retain and flush the partial blob cache, then reopen the same session on Resume. Croc pauses its live child process and resumes the retained connection; after app/process termination it instead requires a normal reconnect. Receiver cache is under private files storage, isolated by session. Do not uninstall or clear app data before resuming. Completed files publish automatically to Downloads/DataBeam or the folder selected in Settings. Each transfer retains its destination through pause/resume. Publication records individual output URIs so retries do not duplicate files; conflicts receive distinct names. Empty directories and zero-byte files are preserved. A user file named databeam_hash.txt is retained. Selected-folder publication checks existing document URIs before writing and never opens an existing file for replacement.

This alpha does not yet implement remote restart of a stopped sender, persistent authenticated control channels, multiple transfer history cards, in-app updates, or full desktop setting parity. Android may terminate background work; force-stopped apps cannot promise remote availability. Sources are staged, so sending requires additional local disk space. Completed receives temporarily occupy both private working storage and public Downloads storage. Croc logs can contain received text and sharing secrets; Android backup is disabled.

## Empty directories in Sendme modes

Sendme collections contain files, so DataBeam adds a bounded `.databeam-directories-v1.json` collection blob only when empty directories exist. Updated DataBeam receivers validate its paths, restore the directories and omit the metadata from the received folder. It is never written into the sender’s original folder. Private transfer caches can retain its blob along with other transfer data. Older/third-party Sendme clients receive the normal files plus this small metadata file, and do not reconstruct the empty directories. Croc uses its own directory protocol without this extension.

## Verification

Build results and six passing on-device engine-transfer checks are recorded in VALIDATION.md. An APK build alone does not establish desktop interoperability or recovery under Android power management. Test both directions against desktop in all three modes, interruption and sender restart, changed-ticket same-hash recovery, different-content rejection, background/screen-off transfers, and file export on a physical device before distribution.

## Licenses

DataBeam and the vendored Sendme engine are AGPL-3.0. Croc is MIT. See the repository root LICENSE, THIRD_PARTY_NOTICES.md and LICENSES directory. The APK includes these notices as assets.

## Known Iroh routing limitation

The phone can lose a Direct UDP path and continue through an Iroh relay. Upstream [iroh#4555](https://github.com/n0-computer/iroh/issues/4555) includes a matching Samsung Android 16/Iroh 1.4.1 report; its fix PR [#4645](https://github.com/n0-computer/iroh/pull/4645) remains open as of 2026-11-14. Per the user’s decision, no local UDP-offload workaround is shipped. Normal relay fallback is retained. Android application-context initialization and platform network callbacks are integrated as required by Iroh’s Android API.
