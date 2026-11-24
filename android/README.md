# DataBeam for Android

Native Android app. Open this directory in Android Studio.

Transfer behavior and required regression coverage are maintained in [TRANSFER_RULES.md](TRANSFER_RULES.md).

## Build on this PC

Android Studio: `X:/as`, SDK: `X:/asproj`. `local.properties` is local and ignored.

```powershell
.\scripts\build-apk.ps1
```

Requires Rust with `aarch64-linux-android`, Go (the recipe selects its toolchain), SDK API 36, and NDK 28.2.13676358. This initial APK targets ARM64 phones. Android Croc is independently pinned in `croc-engine.json` to the previously validated 11.5.5 base plus local GUI telemetry, source adapters and an Android-only local-data-port admission check. Desktop 0.8.0 also retains Croc 11.5.5 and reuses the telemetry patch; its route selection is unchanged. The evaluated 11.6.5 bundle publication required hard links that failed in the Android app sandbox; that upgrade is deferred. The build refuses to package missing native engines. Native libraries and tool downloads are ignored by Git. Gradle and Android tooling may require accepting their licenses in SDK Manager.

## Implemented

- Native Kotlin Android UI; no WebView.
- Top Quick actions dropdown for Croc text, Sendme Broadcast and browser receiving with getcroc.com instructions. Actions prepare the form without starting a transfer.
- The footer troubleshoot button shows an unread guide/release-news badge, matching desktop. Opening the guide clears it; offline failures retain unread state.
- Orange arrow branding and an OLED-black theme. The footer shows the installed version and a troubleshoot shortcut.
- Named file/folder selections with individual removal. Croc has separate Files and Text choices; only the selected input is shown.
- Settings can skip byte-identical files selected individually when sending. Selected folders keep their contents.
- Transfer configuration is locked while starting/running/stopping. Progress, copy/QR, pause and stop remain available as applicable.
- Automatic Croc codes and relay-recovery replacements use three random words from the exact EFF word list shared with desktop.
- Interrupted receives offer Resume. Stopped or failed sends retain filenames, progress and the reason. Resend starts a new session when all original files are accessible, or asks the sender to pick any unavailable items again. Croc text can also be resent.
- On app restart, Resend reopens the original references without keeping staging copies. Sendme issues a fresh ticket for the same collection when its contents are unchanged, allowing the receiver to reuse its cache. Successful sends clear the selection. Completion cards show names, elapsed time and the last Direct/Relay path. Receive completion offers Open folder.
- Errors appear directly in transfer status. No visible log pane; received Croc text has a separate selectable view.
- Manual transfer Resume/Retry only. No general automatic transfer retries or tray UI. Remote restart of stopped senders is deferred; public-relay recovery remains enabled.
- Croc, Sendme and EasySendMe can read seekable, whole-file Android sources using the descriptor granted by the picker, including files whose paths they cannot reopen. This also applies to Share-sheet and folder selections. Pipes, bounded assets and selections beyond the descriptor limit still use private copies. Folder selections preserve their names and empty directories. See [the source-reference investigation](../investigations/android-source-references.md).
- Embedded Rust Sendme engine from the same vendored source as desktop via JNI.
- The pinned, SHA-256-verified, patched Croc CLI built as an Android PIE executable, packaged under jniLibs and executed from Android's installed native library directory (never extracted into a writable directory).
- Android Croc uses upstream's `croc_no_tailcat` build tag: standard LAN Direct and public-relay transfers are available; the optional Tailcat/DERP transport is excluded because its Linux netlink-based probe crashes inside Android's app sandbox.
- Croc files/text, Sendme sharing/receiving, and EasySendMe ticket handoff.
- Fresh CameraX + bundled ML Kit receiver scanner (full portrait camera frames, rotation/lifecycle handling, tap focus, pinch zoom and torch). Sender QR generation for codes/tickets, with stale QR dialogs closed on relay recovery. Receiver camera scanning is embedded in the app and fills the code field without automatically starting a transfer; camera permission is requested when scanning.
- EasySendMe is in Quick actions. Shared codes append `esm` without a space so pasting or scanning the code selects EasySendMe automatically. Croc receive history also offers Scan QR when entering a replacement Croc code and shows completion time, elapsed time and connection type when recorded.
- Native system file and folder pickers, “Send with DataBeam” in Android’s share sheet and selected-text menu, background source preparation, and automatic publication to Downloads/DataBeam after receipt. Sharing files keeps attachments rather than treating their captions as transfer text. Croc file receives preserve the original files and directory tree inside a Sendme-style `*_databeam` folder.
- Ongoing foreground transfer notification with phase, progress and live rate, Stop and tap-to-return. It is removed after completion/stop. Preparation and payload progress are separate; all modes show selection-wide byte totals and ETA when known. Croc includes completed-file counts and explicit verification reporting. Cooperative Sendme cancellation retains partial caches; the wake lock is bounded to six hours.
- Elapsed time appears in the transfer card and ongoing notification. It starts at the first advancing payload-byte report, excludes preparation/initial waiting, pauses, reconnection waits and app downtime, and freezes when stopped/completed. A cached baseline alone does not start the timer. Native event timestamps prevent Android polling delay from being counted. Completion average uses newly transferred payload bytes and active payload-through-saving time; cached bytes do not inflate it. Fully cached exports show no download average. A completed sender keeps its Sent heading, filenames, elapsed time and latest known connection path. The average remains available in telemetry/notifications; the completed card uses the requested concise summary. Normal sends close automatically; explicit Sendme Broadcast keeps sharing open. A new receiver starts a fresh timing summary; concurrent-receiver aggregate averages are omitted because the existing counters follow the latest receiver.
- Public Croc relay recovery: bounded background diagnostics, visible auto-troubleshooting status, sender replacement codes with a warning to reshare, and a Sendme fallback when no public relay is reachable. Receiver codes and explicit custom-relay settings are preserved; payloads are not automatically replayed.
- Croc is the default engine. Sendme includes the EasySendMe short-code option when both engines are enabled. Active transfers keep a progress card.
- Settings cog for advanced Croc relay configuration, a persistent download-folder picker, and manual Croc/Sendme public-relay checks. Croc checks TCP reachability of all four public endpoints; Sendme establishes an Iroh relay connection. These are checks from this device, not global service-health guarantees.
- Battery-optimization and notification status with links to Android controls. Home shows guidance only when a setting needs attention; the idle transfer card is hidden.
- Actual payload-path indicator (Direct/Relay). Native Sendme/EasySendMe paths update as Iroh changes the selected path; multiple sender connections are tracked separately. Croc uses the engine's direction evidence, independently of relay configuration. EasySendMe's Croc ticket handoff does not determine its payload-path label.
- Persistent custom sharing codes, a Clear field action, and five recent successful short codes per engine/direction with tap-to-fill and separate history clearing.
- Saved transfer state with Pause/Resume and replacement-code/ticket controls. A replacement ticket must have the original collection hash. Croc file receives appear in Croc receive history. Multiple partial downloads can be resumed independently. A new Croc code works when the sender offers the same paths and full SHA-256 digests.
- With the default Downloads destination on Android 10 and newer, Croc can write directly to pending Downloads files and publish them without a second copy. Custom folders, older devices, and storage providers that cannot supply the needed access use verified export. For completed default Downloads transfers, Move files out moves the saved files through MediaStore and keeps conflicting names separate.
- Sendme/EasySendMe check private receive stores by content hash before downloading and automatically reuse matching partial or complete data without a confirmation prompt. This also finds matching data after the old card has been cleared. Native verification and destination overwrite protection remain in force.
- Transfer storage cleanup verifies saved output bytes before removing private received working copies. Auto-clear defaults to one week and runs while the app is idle. Settings offers manual cleanup and lets you choose Off or one month instead. Active transfers, verified data awaiting export and saved files are protected. Discarding a partial direct Croc receive removes only its app-owned pending download rows.
- Normal Sendme and EasySendMe senders close automatically after confirmed success and remove their ongoing notification. Sendme offers an explicit Broadcast checkbox (off by default) to remain available for multiple receivers until Stop. Desktop non-broadcast sharing also ends on transfer failure, including a receiver pause; continuing then requires a new sharing session. EasySendMe performs one Croc ticket handoff per session; a replacement exchange requires a new share.
- Settings can hide either Sendme or Croc, keeping at least one enabled. EasySendMe needs both. The mode selector, Quick actions and related main-screen features follow the enabled engines; settings retains both toggles and advanced controls. Changes are blocked during active/retained transfers. Hidden transfers/cached data are not deleted.

## Resume behavior

Sendme/EasySendMe Pause retains and flushes partial blob data. On Resume, choose New ticket / New code after the desktop's normal sender has ended or restarted sharing; ask it to share the exact same files again. Sender still sharing reuses the original ticket only while that original session remains available, such as Broadcast. No cache confirmation is shown. Croc pauses its live child process and resumes the retained connection; after app/process termination it instead requires a normal reconnect. Sendme blob caches and staged Croc receives use private storage. Default Croc receives keep their payload in app-owned pending Downloads files. Do not uninstall or clear app data before resuming. Completed files publish automatically to Downloads/DataBeam or the folder selected in Settings. Each transfer retains its destination through pause/resume. Publication records individual output URIs so retries do not duplicate files; conflicts receive distinct names. Empty directories and zero-byte files are preserved. A user file named databeam_hash.txt is retained. Selected-folder publication checks existing document URIs before writing and never opens an existing file for replacement.

For Sendme/EasySendMe, Receive and Resume automatically reuse verified cached ranges and payload progress begins at the cached amount. A complete cache can be verified/exported without downloading again. Sendme accepts the original ticket for this offline export; EasySendMe needs its previously resolved matching ticket, or a live sender to resolve a short code first. Card Resume uses the saved transfer independently of the main input. Its New code option fetches the restarted EasySendMe sender's current ticket, even when the short code stayed the same. A successful lookup runs once. The files must have the same collection hash. For Sendme, New ticket accepts the replacement ticket for those files. No receiver action remotely restarts a stopped sender.

Both folder Open buttons request reuse of the existing file-manager task/activity. File managers retain control of their own window behavior.

Use Croc receive history to restart a Croc receive after the app closes. Enter or scan the sender’s current Croc code. For a changed batch with full SHA-256 manifests, DataBeam shows the files to replace, add and remove. Applying the update preserves matching partial bytes and resets changed files. Removed files are discarded from the unfinished receive, and previously saved files keep their destinations. A verified complete transfer drops its resume manifest. A sender that provides only Croc's legacy full hash can resume the original file list with the original code.

Android controls the lifetime of background transfers. Unsupported source providers use a local copy for the active send. Receive caches remain available for resume or offline export until eligible cleanup. Verified published working duplicates are removed automatically. Android backup is disabled.

## Empty directories in Sendme modes

Sendme collections contain files, so DataBeam adds a bounded `.databeam-directories-v1.json` collection blob only when empty directories exist. Updated DataBeam receivers validate its paths, restore the directories and omit the metadata from the received folder. It is never written into the sender’s original folder. Private transfer caches can retain its blob along with other transfer data. Older/third-party Sendme clients receive the normal files plus this small metadata file, and do not reconstruct the empty directories. Croc uses its own directory protocol without this extension.

## Verification

Build results and six passing on-device engine-transfer checks are recorded in VALIDATION.md. An APK build alone does not establish desktop interoperability or recovery under Android power management. Test both directions against desktop in all three modes, interruption and sender restart, changed-ticket same-hash recovery, different-content rejection, background/screen-off transfers, and file export on a physical device before distribution.

## Licenses

DataBeam and the vendored Sendme engine are AGPL-3.0. Croc is MIT. See the repository root LICENSE, THIRD_PARTY_NOTICES.md and LICENSES directory. The APK includes these notices as assets.

## Known Iroh routing limitation

The phone can lose a Direct UDP path and continue through an Iroh relay. Upstream [iroh#4555](https://github.com/n0-computer/iroh/issues/4555) includes a matching Samsung Android 16/Iroh 1.4.1 report; its fix PR [#4645](https://github.com/n0-computer/iroh/pull/4645) remains open as of 2026-11-14. Per the user’s decision, no local UDP-offload workaround is shipped. Normal relay fallback is retained. Android application-context initialization and platform network callbacks are integrated as required by Iroh’s Android API.


## Temporary Android UDP GSO workaround

Android Sendme/EasySendMe sender and receiver endpoints temporarily disable UDP segmentation offload to avoid the documented direct-path timeout. Desktop defaults and Croc are unchanged. When upgrading Iroh or Sendme, check whether the released dependency contains the upstream fix, then reassess/remove this workaround using small Direct-transfer checksum checks. See [the upgrade review note](../investigations/sendme-android-direct-relay.md#required-review-when-updating-iroh-or-sendme).

- Quick actions has extra space below it; the battery/notification warning appears above it. File payloads have Clear all, text has Clear text, and the receiver code has Clear. Relay checks use green for reachable and yellow for unreachable.
- Android 13+ requests notification permission once on first launch if needed; denial is respected, and Settings retains the notification link.
- Croc text uses stdin with the engine text marker rather than putting the payload in command arguments. Received stdout is preserved exactly, including CRLF, leading/trailing newlines and Unicode. An explicit 8 Mi-character receive limit fails visibly instead of silently truncating oversized text; larger content can be sent as a file.

- Large text is handed to the foreground service through a private temporary configuration file, deleted when consumed, to stay below Android Binder limits. Foreground notification is established before loading it. Failed Croc exits read terminal errors from stdout for relay recovery; successful stdout remains exact received text.

## Public Android builds

`assembleRelease` produces the stable v0.8.8 APK. Release signing uses the ignored `.tools/signing/databeam-release.jks` and `.tools/signing/release.properties` (properties: `storePassword`, `keyAlias`, `keyPassword`). Keep a private backup of both files because future public APK updates must use the same key.

Development APKs installed during USB testing use the development certificate. Install development updates over that installation to preserve its settings and partial data. The public APK uses the same dedicated release certificate as the public beta.

Guide badge refresh now runs on every foreground resume, bypassing the ordinary cooldown and HTTP cache; concurrent checks are coalesced. The read revision persists across APK updates. The dot draws above the native button rather than relying on child insertion order.
