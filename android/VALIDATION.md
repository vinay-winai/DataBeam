# Android and desktop validation — 2026-11-14–15

## Current builds

- Native ARM64 Rust JNI and pinned Croc builds passed. Android Croc uses `croc_no_tailcat`; classic LAN/public-relay transfers remain available. The optional Tailcat transport was excluded after its Linux netlink probe panicked inside the Android app sandbox.
- Android debug/instrumentation APK builds passed; 17 JVM tests passed. Lint: zero errors, 50 warnings (including localization, SDK currency, ARM64-only packaging, synchronous persistence and test API use).
- Shared Sendme engine: 32 tests passed, one manual test ignored. Includes real loopback transfer, cache export, directory metadata validation, cached ranges and sender acknowledgement checks.
- Desktop: 138 tests passed, eight manual/fixture checks ignored. Release build passed. Updated executable: `../dist/DataBeam-0.8.0-windows-x64.exe`.
- Python test-driver syntax and tracked diff whitespace checks passed.
- Prior alpha checks passed APK signature v2, zip/16 KiB ELF alignment, JNI symbols and license/native engine packaging. This is a debug alpha, not a signed distribution release.

APK: `app/build/outputs/apk/debug/app-debug.apk`, 31,550,621 bytes.
SHA-256: `f8251f737cc77480b4ecefcfb5704faf85fdd6a22a60c2be8d0dd63327b8b854`.
Version 0.8.0 is installed. Desktop uses Croc 11.6.5; Android is independently pinned to its validated 11.5.5 base with the same local GUI progress reporting. No Iroh GSO override is shipped.

## Phone and configuration

Samsung SM-M336BU, ARM64, Android 16. SDK `X:/asproj`, Studio/JBR `X:/as`, NDK 28.2.13676358. Notifications enabled and user-selected Unrestricted battery access retained. USB stay-awake remains enabled at the user's request. Forced idle/battery and Wi-Fi/mobile-data overrides used by tests were restored afterward.

Tests run serially on the phone and use generated files only. Logs/fixtures/verified JSON records live in ignored `.tools/smoke/`. Passing small-file checks do not establish every Android lifecycle or networking condition.

## Workflow and interoperability

| Check | Result |
| --- | --- |
| Croc, Sendme, EasySendMe, desktop → Android and Android → desktop, 1 MiB | All six passed again on the progress/share update; SHA-256 verified |
| Sendme and EasySendMe 256 MiB pause/resume | Passed again; nonzero cached progress reused; automatic Downloads checksum matched |
| Croc 256 MiB live pause/resume | Passed again; retained process/connection continued from nonzero progress |
| Sender restarted with a different same-content Sendme ticket | Passed again; cached content reused and saved checksum matched |
| Nested automatic save, existing-file sentinel, idempotent repeat save | Passed on device |
| Zero-byte files, empty directories, legitimate `databeam_hash.txt` | Storage regression passed; legitimate user files are not filtered as engine markers |
| Custom destination folder | Prior on-device folder-picker/persisted-access and nested/conflict/idempotence checks passed; latest empty-directory storage run used default Downloads |
| Android share sheet and selected-text menu | Passed discovery/label, SEND, SEND_MULTIPLE, PROCESS_TEXT, attachment captions, duplicate URI handling, draft recreation and activity reuse; opening a share does not start a transfer |

The 1 MiB SHA-256 was `32980d4957d54f433b42e1f229853be85982e4e91462bc719c10186c8f21a10c`.
The 256 MiB pause fixture SHA-256 was `0c5ebcf5cb0e4611da9ecf2adf97f248001e00ed6c0bd75a3a44a33ff26d3612`.
Drivers: `device_smoke.py`, `pause_resume_smoke.py`, `croc_pause_smoke.py`, `changed_ticket_smoke.py` and storage/share instrumentation.

Croc's live pause uses SIGSTOP/SIGCONT. After its process/connection is lost, normal reconnect requires a sharing sender; this is not remote sender restart. Native modes persist their partial blob caches. No general automatic transfer retry was added; explicit public-relay code recovery remains enabled.

## QR and relay checks

- The final Android scanner is a fresh CameraX + bundled ML Kit implementation: full portrait camera frames, lifecycle/rotation handling, tap focus, zoom and torch. Earlier experimental ZXing capture/cropping paths were removed. ZXing remains only for sender image generation/tests.
- Desktop QR rendering uses black modules on white, a four-module quiet zone and integer-pixel nearest-neighbour sizing; the long Sendme ticket dialog is larger.
- Actual desktop-renderer fixtures passed decoder checks, and the user confirmed live camera scanning for all three modes after the desktop QR/scanner update. Earlier generated-code checks alone had not established desktop camera interoperability.
- The user confirmed relay checks work. Manual device/desktop checks found public Croc endpoints 1, 2 and 4 reachable, endpoint 3 timing out, and established an Iroh relay connection for Sendme. These are device-local checks at that time, not a global health guarantee.
- Sender-side public Croc relay recovery and receiver guidance were tested. All-unreachable fallback guidance is covered deterministically; global failure was not induced. Explicit custom-relay settings and receiver codes are preserved.

## Routes, notification and progress

- Foreground notification includes phase, progress and live rate, Stop and tap-to-return; it is removed on completion/stop. Its live progress record was captured during the physical USB-unplug test.
- Preparation counters are separate from payload counters. Native receive/send rates use their scaled byte values correctly; byte progress and ETA appear when reliable totals exist. Croc uses structured preparation/rate/selection-wide transfer telemetry. Completed/resumed bytes affect progress while only transferred payload contributes to rate. Explicit verification markers distinguish hashing from payload transfer.
- Both Croc and native modes now use selection-wide payload totals. Croc additionally reports completed-file counts. Indeterminate stages cannot provide a truthful ETA.
- Actual Direct/Relay evidence comes from engine route events, not the configured relay. Iroh watches selected paths and supports multiple sender connections; Croc uses its direction line. Native uploads have shown Direct → Relay changes in earlier tests.
- The previous large phone → PC Croc timeout did not recur: normal public-relay rendezvous, without ADB relay mappings, passed 512 MiB Direct foreground and 512 MiB Direct locked-screen sends with matching SHA-256. This does not prove the historical timeout's unique cause.

## Background and recovery

| Scenario | Result |
| --- | --- |
| Earlier Home + screen lock, all three engines, both directions | Passed checksums; native receives 512 MiB, native sends 1 MiB, Croc both 512 MiB |
| Real USB unplug, locked, Croc PC → Android, 512 MiB | Passed Direct transfer and automatic Downloads SHA-256; sender was gated until physical USB disconnection |
| Forced deep idle after payload began, Croc/Sendme/EasySendMe receives, 512 MiB each | All three passed saved-file SHA-256, with Unrestricted retained |
| Five-second Wi-Fi loss, Sendme receive, cellular data temporarily disabled | Passed; original engine connection recovered after Wi-Fi returned; no app retry loop added |
| Force-stop native receiver mid-transfer, then manual resume | Passed; first resumed report reused 32,833,654 cached bytes and final published checksum matched |

512 MiB fixture SHA-256: `474a99dd2796b7fd2547f6ca3aef4335bc7d6f5a21c12ebd4b76c8818a2e9919`.

The physical battery run's original driver timed out waiting for USB reconnection; the PC reached 100% while unplugged and the subsequent reconnected phone state was Complete. The public file was then independently hashed successfully (`battery-d93b624a-verified.json`). This was a verification-wait timeout, not a failed payload transfer.

The earlier six plugged background checks used ordinary Iroh networking and a local Croc relay over temporary USB mappings (removed afterward). New forced-idle and real-unplug Croc checks used public rendezvous. Samsung AOD may report Dozing; screen-off assertions use `PowerManager.isInteractive`.

## Large folders and metadata

Folder checks use a generated 128 MiB payload, a zero-byte file, a nested empty directory with a space in its name, and a user-provided `databeam_hash.txt`. Android sends also select an additional separate file. All six engine/direction checks passed with matching content hashes, empty-directory checks, and automatic public-save hashes for receives. Drivers: `folder_smoke.py` and the generated-folder instrumentation fixture.

One EasySendMe upload selected a slow relay route and exceeded the 310-second harness deadline (about 13 MiB had transferred); its isolated 128 MiB rerun passed. This is a remaining variable-route/performance limitation, not evidence that every large upload will be fast or Direct. The first native folder harness attempt used an outdated peer binary built at a different output path; after rebuilding the exact peer path, the empty-directory transfer checks passed.

Sendme's file-only collection gains an optional bounded `.databeam-directories-v1.json` blob only when needed. Updated desktop/Android receivers validate relative paths, create the directories and never export this metadata file. Source folders are untouched. A private cached blob can remain. Older/third-party receivers retain the metadata file and do not recreate empty directories. Croc uses its native directory protocol and receives no added wrapper or metadata.

## Known limits and explicitly skipped checks

- Overnight/naturally entered Doze, additional devices and OEM variants were explicitly skipped by the user. Android still enforces its platform data-sync foreground-service time limits.
- Wi-Fi ↔ cellular switching was explicitly skipped. Arbitrary NAT/WAN conditions and every kill/restart timing are not claimed by this validation.
- Long-lived/large native uploads over slow relay paths. Earlier 512 MiB attempts exceeded the harness deadline before the background trigger and do not establish a background failure.
- Remote restart of a stopped sender remains intentionally deferred. Android cannot promise availability after force-stop. No tray or visible log pane is required for Android.

Selected files are copied into private staging because CLI/native engines need readable paths. Received files can occupy private work/cache storage as well as their public destination. This alpha is not a claim of complete desktop settings/history/update parity.

## Final handoff

The final APK is installed. Storage and share-entry device regressions passed again on it. The test-only APK was removed, generated transfer cards/codes and 109 private test paths were cleared, and 33 provenance-recorded test Downloads were verified absent. Empty test directories were pruned using directory-only removal. User destination/relay settings and unrestricted-battery/notification access were retained; USB stay-awake is 2, cellular-data preference is restored, forced idle is off and no ADB reverse mappings remain.

## Version 0.8.0 final pass

- Desktop header DataBeam text removed and the extra Receive/Troubleshoot gap removed. The user launched the release build and confirmed the header UI is correct.
- Android’s bottom ? button opens the same troubleshooting URL. The on-device share-entry test intercepted and verified the exact ACTION_VIEW target without launching a browser; share/text selection/draft checks also passed.
- Desktop Croc upgraded from official v11.5.5 to verified v11.6.5. Its upstream/telemetry and bundle/progress regressions passed. Both default and one-shot progress use exact engine totals rather than legacy rounded terminal bars.
- A real Android receive with v11.6.5 failed in its new bundle capability probe (`linkat: permission denied`). That engine upgrade is not shipped on Android. The Android recipe uses the previously validated v11.5.5 base plus selection-wide progress and explicit verification markers. No Croc output wrapper is added.
- The Iroh direct-path timeout closely matches upstream #4555, including a Samsung Android 16/Iroh 1.4.1 report. PR #4645 remained open when checked. The user explicitly chose documentation and normal relay fallback over a local workaround. Experimental no-GSO test results are diagnostic only; the override was removed from the installed/shipping build. See investigations/sendme-android-direct-relay.md.
- Correct Android DNS application-context initialization and platform network callbacks remain integrated; these are the documented platform API integration.

Final shipping-build device checks passed:

- Croc: 128 MiB folder in both directions, 16 additional small files, nested empty directories, zero-byte file and a user file named databeam_hash.txt. Exact selection-wide byte totals and 19/20-file completion counts were asserted.
- Croc: 256 MiB live pause/resume and automatic Downloads SHA-256 passed again.
- Croc: normal public-rendezvous 1 MiB upload completed via Relay with matching SHA-256. The 512 MiB public run had selected Relay and exceeded the 120-second instrumentation deadline; it is not recorded as a passing Direct transfer. Earlier Direct checks are historical results from their stated builds.
- Sendme and EasySendMe: final upstream-default 1 MiB uploads both passed SHA-256 through normal Direct-to-Relay fallback (about 24 s and 17 s). Native receive checks in both modes passed SHA-256 again. These final results use no GSO workaround.
- APK signature v2 and 16 KiB zip alignment verified.

Overnight Doze, Wi-Fi/cellular switching and additional-device testing remain explicitly excluded at the user’s request.
