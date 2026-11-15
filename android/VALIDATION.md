# Android and desktop validation — 2026-11-14–15

## Current builds

- Native ARM64 Rust JNI and pinned Croc builds passed. Android Croc uses `croc_no_tailcat`; classic LAN/public-relay transfers remain available. The optional Tailcat transport was excluded after its Linux netlink probe panicked inside the Android app sandbox.
- Android debug/instrumentation APK builds passed; 22 JVM tests passed. Lint: zero errors, 45 warnings (including localization, SDK currency, ARM64-only packaging, synchronous persistence and test API use).
- Shared Sendme engine: 32 tests passed, one manual test ignored. Includes real loopback transfer, cache export, directory metadata validation, cached ranges and sender acknowledgement checks.
- Desktop: 138 tests passed, eight manual/fixture checks ignored. Release build passed. Updated executable: `../dist/DataBeam-0.8.0-windows-x64.exe`.
- Python test-driver syntax and tracked diff whitespace checks passed.
- Prior alpha checks passed APK signature v2, zip/16 KiB ELF alignment, JNI symbols and license/native engine packaging. This is a debug alpha, not a signed distribution release.

APK: `app/build/outputs/apk/debug/app-debug.apk`, 31,550,942 bytes.
SHA-256: `db8e21257cfd5deb3d6a0ee6d62a965f30f0a362a445426311b14d977fa2bc3d`.
Version 0.8.0 is installed. Desktop and Android retain Croc 11.5.5 with the same local GUI progress reporting; the evaluated desktop 11.6.5 upgrade was deferred at the user's request. No Iroh GSO override is shipped.

Desktop release revision (2026-11-15): the 11.6.5 release run was cancelled before publication. Desktop now reuses the validated 11.5.5 telemetry patch, with its normal desktop transport build. The EasySendMe ticket receiver owns a private temporary directory under the user's DataBeam cache until Croc exits; normal Croc receives use the app directory by default, matching the displayed save destination. The previous end-to-end handoff test supplied its own folder and missed this GUI path; it now calls the actual ticket receiver.

Final revised desktop checks: 138 tests passed, eight fixture/manual checks ignored by the ordinary suite. Three real-transfer regressions also passed with `TEMP` and `TMP` set to `C:\Windows\Temp`: EasySendMe ticket/payload round trip, Croc text/file round trips, and Croc conflict/overwrite protection. A separate Croc 11.5.5 probe reproduced the original `open receive root: ... Access is denied` when run directly in that system Temp root. Android's shipped engine/APK is unchanged by this desktop correction.

## Android UI revision — 2026-11-15

- Orange arrow launcher/header branding, matching monochrome notification icon, OLED-black surfaces and version footer. Visuals were checked on the phone.
- Device regression passed: named selections and individual removal, adding a folder without replacing existing selections, exclusive Croc Files/Text inputs and matching send label, live configuration/settings locking, exact shared EFF three-word codes, ongoing notification with a progress bar, stopped-sender restart for text and files. The file restart passed after the original generated MediaStore URI was deleted, using the already staged copy.
- Share-sheet regression passed again, including selected Unicode text, files/multiple files, caption protection, activity recreation with Text mode retained, and the bottom troubleshoot redirect.
- Sender/receiver state messages and recovery actions are covered by JVM tests. Content mismatch and invalid-ticket errors name the appropriate clear/new-receive action rather than always suggesting Resume.
- The first live stop test exposed an uncaught `InterruptedIOException` in the Croc stderr reader. Reader failures now return to the transfer worker; expected closures during Stop do not crash the app, and readers are joined before a restart. Four subsequent stop/restart sessions passed.
- That initial crash prevented restoration of the prior stopped sender card. The user was informed; saved files and other settings were preserved, the normal relay setting was restored, and the generated session was cleared. Repeat testing used an independent host backup as well as instrumentation cleanup.
- No direct Android URI/descriptor sender adapter was added. The current staging-copy behavior and DashBeam comparison are documented in `../investigations/android-source-references.md`.

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
- Historical evaluation (not shipped): desktop Croc was upgraded from official v11.5.5 to verified v11.6.5. Its upstream/telemetry and bundle/progress regressions passed. Both default and one-shot progress use exact engine totals rather than legacy rounded terminal bars.
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

## Saved receive data choices — 2026-11-15

`android/scripts/receive_cache_e2e.py` passed all ten physical-device checks on SM-M336BU / Android 16. Instrumentation clicked the actual app dialog; no production auto-continue or retry was added.

- Sendme and EasySendMe each received a generated 256 MiB file, paused, displayed a partial-data prompt and continued. The final installed build's first resumed payload reports were 31,965,300 and 18,432,120 bytes respectively. The prompt reports payload bytes without collection metadata, so its count can be slightly lower than the engine's first progress sample.
- Both completed caches were rediscovered from a new receive session and exported with the PC sender stopped. These offline checks use Sendme tickets; a fresh EasySendMe short code still needs its sender for ticket exchange.
- Both modes displayed a complete-data prompt for a separate 1 MiB transfer. Delete and start over caused a zero-byte initial download sample and a valid fresh download.
- An unrelated 256 MiB complete cache was still usable offline after each deletion.
- All ten automatic Downloads outputs matched their source SHA-256. The 256 MiB fixture hash was `0c5ebcf5cb0e4611da9ecf2adf97f248001e00ed6c0bd75a3a44a33ff26d3612`; the 1 MiB fixture hash was `631b84027d6b9e52b539c4e8373622d23032dfadc64d60af87339c9037e4f769`.
- The host driver restored the prior settings/transfer preferences and deleted only provenance-recorded test outputs and session directories.

Native cache inspection's regression passed for absent, partial and complete stores without network access or file export. The Android APK/test APK build, 22 JVM tests and lint passed. Receiver caches match the ticket's collection hash; deletion is restricted to that hash beneath private receive stores.

## Sender restart and Croc reconnect — 2026-11-15

`android/scripts/restart_e2e.py` completed ten distinct passing sender scenarios across the initial and corrected regression runs:

| Scenario | Croc | Sendme | EasySendMe |
| --- | --- | --- | --- |
| Stop, then actual Restart sharing button; three files, including zero-byte and Unicode names | Passed | Passed | Passed |
| Force-stop app while waiting, reopen and Restart sharing; nested folder, empty directory and extra file | Passed | Passed | Passed |
| Interrupt payload, restart sender, reuse desktop receiver cache, verify final SHA-256 | Passed | Passed | Passed |
| Stop/restart Unicode text; verify exact text without a leaked file | Passed | N/A | N/A |

Flat-file tests removed the original Android document URIs before restarting: the existing staged snapshot was reused. Native restarts issued different tickets with identical collection hashes. EasySendMe checks obtained the new ticket through an actual Croc short-code exchange. App-close tests also verified the old Croc child did not survive force-stop.

First desktop receiver samples after payload interruption/restart were 12,910,592 bytes for Croc's 128 MiB fixture, 278,828 for Sendme and 262,444 for EasySendMe (native fixtures 4 MiB plus two small files). Every final file size, layout and SHA-256 matched, including empty directories in folder cases. These are functional restart checks, not throughput benchmarks.

The first Croc reconnect attempt exposed a separate CLI consent step: `--yes` does not answer the overwrite/resume question, so the noninteractive CLI skipped its partial file and exited successfully. The Android fix accepts resume only inside the session's private receive directory and binds that directory to the original Croc code; public-file publication keeps its existing overwrite protections. The host regression explicitly consents only for its own generated partial destination. No Croc engine upgrade was required.

`python android/scripts/receive_cache_e2e.py --croc-only` then force-stopped the Android receiver during a 64 MiB receive, restarted the PC sender and resumed the same phone session. The first payload sample was 18,087,936 bytes; automatic Downloads SHA-256 matched `5c8a41a9b8d7fc418ba77b0312efc461de86740ef476f4b53adab9313c4d1562`. The final ten native receive-cache tests were repeated after this fix and all passed.

An earlier 8 MiB EasySendMe phone upload exceeded the 190-second test deadline while its Direct/Relay paths switched and the relay was slow. This remains part of the documented upstream Iroh routing/performance limitation; no networking workaround or general app retry was added. The passing smaller restart checks do not establish large-upload throughput.

Installed APK: 31,586,204 bytes, SHA-256 `574ab76c86ca2d469289eabd1701031db4ee0b0d32ba6fd0ca73b7ef9334db2a`. Croc remains 11.5.5. Remote sender restart remains deferred, and the user-excluded overnight/network-switching/additional-device checks remain excluded.

Final device storage and UI-controls regressions passed again, including existing-file protection, idempotent publication, selection controls, configuration locking and ongoing notification progress. APK v2 signature and 16 KiB alignment passed; the installed package hash matched the local APK. The instrumentation package was removed, previous settings/transfer preferences matched their host backup byte-for-byte, no ADB reverse mappings remained, and DataBeam was reopened without a generated active transfer.

## Elapsed time and completion average — 2026-11-15

- Transfer cards and ongoing notifications show elapsed active time. Preparation/waiting/saving count toward elapsed time; pauses and app downtime do not. Completed/stopped durations freeze. Only accumulated durations are persisted, so reboot/process downtime cannot be mistaken for active work.
- Completion average uses reported newly transferred payload bytes over active payload-through-saving time. Cached starting offsets are excluded. Fully cached exports have no download average. A native sender's latest completed average stays frozen while it waits for another receiver; a new payload starts a new average. Aggregate averages are hidden for concurrent native receiver connections because current progress follows the latest receiver.
- Six deterministic timing tests cover preparation, pause/resume, process recreation, cached exports, sender idle periods, repeated final counters and duration formatting. The full JVM suite passed 29 tests with no failures; APK/test APK builds and lint passed.
- Ten on-device Sendme/EasySendMe receive-cache checks passed with assertions for the displayed elapsed time/average and original saved-file SHA-256. Fully cached offline saves correctly counted zero transferred bytes and displayed no download average.
- Croc, Sendme and EasySendMe sender stop/restart transfers passed displayed elapsed/average upload assertions and desktop receiver SHA-256 checks. Android Croc app-close/reconnect also passed displayed completion timing and automatic Downloads SHA-256.
- The device UI-controls test verified elapsed text in the ongoing notification. The final full suite also exposed and fixed a pre-existing flaky test: the EFF dictionary word `yo-yo` is one word, so counting hyphen separators cannot validate a three-word generated phrase. The production dictionary and code generator were unchanged.

Timing APK SHA-256: `7547550a550376191b1404f04b1a459a62ed2ec32cf972805bd1a3928360d744` (31,586,204 bytes). Earlier APK hashes above identify their respective historical builds.

## Android source references and cleanup — 2026-11-15

This section supersedes earlier always-copy sender behavior described above. Changes apply to Android only. Croc remains 11.5.5; desktop filesystem handling and transport defaults are unchanged.

- Accessible seekable, whole-file Content URI descriptors use named aliases and remain open until the engines stop. Android-only adapters preserve original names and treat the aliases as file bytes, including Croc hashing/source validation. Pipes, bounded assets, provider-private inodes that forbid reopening and descriptor-limit overflow use bounded, cancellable copy fallbacks.
- Local sender restart reopens referenced original URIs. Originals/access must remain available. An intact all-copy fallback snapshot can still restart when original access is gone. Aliases are removed before descriptor numbers can be reused, and native sender stores shut down before their working cache is removed.
- Completed receives verify destination lengths and SHA-256 before removing private received working files. Matching publication receipts are checked before reuse; changed public outputs receive a fresh protected name. Legacy migration requires matching filename, relative folder and bytes. Unknown zero-byte files, sparse files and empty-folder trees are not silently discarded merely because allocated payload bytes are zero.
- Settings offers Keep/Delete for unused private transfer data. Known inactive caches expire after seven days when checked. Current resume/restart data and selected private sources are protected; public saved documents are never cleanup deletion targets. Unknown legacy caches require a deletion choice.

| Check | Result |
| --- | --- |
| APK/test APK builds, JVM tests and Android lint | Passed: 31 JVM tests, zero failures; lint zero errors (45 warnings) |
| Shared Sendme library regression suite | Passed: 33 tests, one manual test ignored |
| Direct-source stop/restart; original filenames, zero-byte file, desktop SHA-256 | Passed in all three modes; three references, zero copies; no full payload duplicate in the sender blob store |
| Referenced-source app/process death and restart | Passed in all three modes; original URIs reopened and desktop SHA-256 matched |
| Folder-path app/process death and restart | Passed in all three modes; nested files/Unicode names/empty directories matched |
| Provider-private, pipe and bounded-slice fallback folder; original access removed before restart | Passed in all three modes; exact bytes/layout/empty directory matched |
| Mid-payload sender restart with desktop cache reuse | Passed in all three modes; Croc 128 MiB, native modes 1 MiB bounded repeats |
| Croc Unicode text restart | Passed; exact text, no leaked file |
| PC-to-phone Sendme/EasySendMe receive-cache suite | All ten checks passed: 256 MiB pause/continue, offline complete export, delete/redownload and unrelated-cache preservation; saved checksums and working-copy removal verified |
| PC-to-phone native route evidence | Both 256 MiB checks transitioned Relay to Direct and completed Direct; this does not guarantee every future path |
| Croc PC-to-phone app-close reconnect | Passed: 64 MiB SHA-256 matched, resumed from 16,809,984 bytes; saved working duplicate removed |
| Final device source access, child descriptor read, cleanup, storage, share-sheet and UI-controls checks | All six passed; ongoing notification progress and configuration/storage-setting locking verified |
| APK verification | v2 signature and 16 KiB alignment passed; installed package SHA-256 matched local APK |

Sixteen distinct successful sender scenarios were verified across the generated-fixture runs. The first restricted host attempt could not open relay sockets; it was rerun with approved network access. A Croc text attempt returned EOF and passed on repeat. A 4 MiB Sendme upload exceeded 190 seconds while selected paths switched between Direct and Relay; the smaller repeat passed with 131,372 cached bytes reused. An initial EasySendMe test let the old receiver finish its cache before sender restart, so its new sender correctly saw no new receiver; the test now terminates the initial native receiver at the partial threshold. Both corrected native interruption cases passed with 131,372 bytes reused. These functional checks do not establish reliable large-upload throughput, and no transport workaround or general app retry was added.

Drivers/artifacts (ignored): `direct-source-e2e-final`, `direct-source-network-repeat`, `source-fallback-restart-final`, `source-reference-app-death-final`, receive-cache run `e03e79bd-3f4d-49c7-a38a-09cf8c61653e`, Croc reconnect run `730f6bc6-d36a-4827-9c6d-52fe366642cf`, and `source-cleanup-final-device` under `android/.tools/smoke/`. Checked-in drivers are `restart_e2e.py`, `receive_cache_e2e.py` and the instrumentation regressions. Host drivers support absent preferences after a fresh reset and remove their own transfer receipts.

At the user's request, DataBeam app data was reset before testing (3,045,110 KiB to 4 KiB). Final generated transfer data and screenshots were removed; the private directory measured 44 KiB after reopening. All thirteen pre-existing public Download files remained in the original inventory. Old pre-reset preferences were not restored. The instrumentation package, temporary local test relay and ADB reverse mappings were removed. DataBeam was reopened without a generated transfer card. DashBeam's own app data was not reset.

Installed APK: 31,587,675 bytes, SHA-256 `49e1f5a85a356c1563c96fdb58af9384cc9c37f80f17786282c1d8f976b0f4a8`.

Measured DashBeam 0.7.1 storage and source-inspected FlCroc/croc-app/iOS comparisons are in [the source-access investigation](../investigations/android-source-references.md). Iroh issue #4555 and proposed fix #4645 were rechecked on November 15: both remain open, the PR is unmerged, and the latest published Iroh is still 1.4.1. The existing documentation-only decision stands. Overnight Doze, network switching and additional devices remain explicitly excluded.

## Android completed sender presentation — 2026-11-15

The user's phone-to-PC EasySendMe transfer had completed, but Android still displayed Waiting for a receiver, an increasing elapsed value and no selected files after activity recreation. The native endpoint was deliberately still sharing. This Android-only change separates acknowledged payload completion from session availability; no desktop code or native transport behavior changed.

- An acknowledged idle native sender now displays Sent, completed progress, a frozen elapsed/average and its last actual connection path. Its foreground notification also says Sent while the share remains open. A new receiver replaces the old path and begins a fresh timing summary when payload starts.
- Ending sharing after completion preserves Sent and offers Share again. Recovery after app closure, including closure during stopping, preserves acknowledged completion; a transfer at 100% without acknowledgement remains interrupted.
- Fresh activity creation restores the current sender's selected URIs/names or cached paths. This also repaired the user's existing two-item completed card without clearing app data.
- Older completed cards lack a recorded finish-time snapshot and may lack the removed connection path. The UI omits unavailable values rather than presenting idle session time as elapsed transfer time or inventing a route.
- [TRANSFER_RULES.md](TRANSFER_RULES.md) is now the maintained Android transfer-state tree/table, with links by name to the enforcing regression checks.

Validation: 33 JVM tests passed; APK/test APK builds and lint passed (zero errors, 47 warnings). `SenderClosureRegression` passed isolated recovery checks for running/stopping/stopped/interrupted acknowledged senders and rejection of unacknowledged 100%. Croc stop/restart still passed its real PC-receiver checksum check. Sendme and EasySendMe real stop/restart transfers passed desktop SHA-256 verification plus assertions for the Sent heading, frozen elapsed after an idle delay, retained Direct/Relay route, ongoing Sent notification, exact restored selection count after closing/reopening the activity, and completed state after Stop sharing. The first heading assertion incorrectly compared Android's styled CharSequence directly with a String; it was corrected to compare text values, and both native cases passed on repeat.

Artifacts are under `android/.tools/smoke/completed-sender-ui/`; final native cases are in `verified-e2e`. The user's real completed card and settings matched their host backups byte-for-byte after generated-fixture testing. Its two original selections and 700,692 completed bytes were retained. The instrumentation package was removed and the phone reopened on the repaired card. No new app-data reset was performed.

Installed APK: 31,587,675 bytes, SHA-256 `ae24cc85513d36945023466fe78035ce6fd64d6ad26e39586ca42334fefbb96a`. Installed hash, v2 signature and 16 KiB alignment were verified.

## Android normal completion, Broadcast, quick actions and news — 2026-11-15

This supersedes the earlier Android default of keeping all native senders open after success. Normal Sendme and EasySendMe now close their engines after the existing engine completion acknowledgement, remove the ongoing foreground notification, release source descriptors/wake locks, keep the Sent summary and unlock configuration. A disconnect/failure or 100% byte count alone does not trigger successful shutdown. Explicit Sendme Broadcast (off by default) remains open for additional ticket receivers until Stop; EasySendMe's single short-code exchange is not offered as Broadcast. The session retains its Broadcast choice when restarted.

Top Quick actions now offers Send text (Croc Text), Broadcast (Sendme with the checkbox enabled), and Send to browser (Croc Files with the getcroc.com receiver instruction). Actions configure the form without starting a send and are disabled during active sessions. Browser guidance survives draft recreation/session restart. The footer ? button shows a dot for unread troubleshooting guide/release-news content, using the same normalized SHA-256 revision/read rule as desktop. The bundled guide is initially read; remote checks are bounded HTTPS background work with 30-minute refresh/five-minute failure retry. Failed checks do not clear an unread badge. Successful guide opening marks the latest known revision read.

At the user's updated request, Android sender and receiver endpoints temporarily disable UDP GSO using Android-only compilation guards. Desktop retains its previous transport defaults, and Croc is unchanged. Source comments, Android README and `../investigations/sendme-android-direct-relay.md` require reassessing this workaround against the released upstream fix whenever updating Iroh/Sendme. No CPU/battery/max-throughput benchmark or repeated large-file test was requested or performed for this update.

Validation was deliberately small: native/APK builds passed, all 36 JVM tests passed and lint reported zero errors (53 warnings). Normal Sendme and EasySendMe phone-to-PC checks used generated 1 MiB payloads plus two tiny files: both reported Direct, matched desktop SHA-256, closed automatically, removed notifications and unlocked configuration. Explicit Sendme Broadcast served two independent desktop destinations/caches with the same ticket, both Direct and checksum matched; the engine/notification remained available until Stop. One earlier Broadcast attempt delivered both copies but did not receive sender-side completion acknowledgement for one receiver; it was not treated as success. The small repeat passed the strict acknowledgement checks. No acknowledgement condition was weakened.

Device UI-controls regression passed all three actual popup-menu actions, browser guidance, configuration locking and existing selection/notification checks. GuideBadgeRegression passed unread-dot display, successful guide-open interception and read acknowledgement. GuideNewsTest covers read/unread persistence and CRLF/LF normalization. Generated fixtures, temporary relay and ADB reverse mappings were removed. The final test APK was uninstalled and DataBeam reopened.

An ADB disconnect exposed a test-driver bug: a reused empty snapshot was treated as missing preferences and cleared the saved card/settings entries. Original files were not touched. The user's intact pre-test `normal/settings.xml` and `normal/transfer.xml` backups were restored and verified byte-for-byte, retaining the completed 700,692-byte two-item card. Both checked-in test drivers now distinguish missing files from ADB/read failures and abort on the latter; restart tests take a fresh snapshot rather than blindly reusing an earlier empty backup. A disconnected-serial regression verified that an existing backup remains untouched. The user was informed of the restoration.

Artifacts: `android/.tools/smoke/android-actions/normal`, `broadcast-repeat`, `ui` and `harness-disconnect-check`. Final installed APK: 31,594,425 bytes, SHA-256 `52c8960967bb7fc46f19b9810ef1572fc68a680c29f810cd19b871b15bd4be96`; installed hash, v2 signature and 16 KiB alignment passed. Desktop UI and non-Android transport behavior were not changed.

## Android transfer polish and public beta - 2026-11-15

Current behavior supersedes the earlier Restart/Share-again and elapsed-time UI records above.

- Latest selected Sendme/EasySendMe route replaces the initial route; unknown/closed teardown events retain the last known route. JNI coalesces progress while preserving the baseline, first advancing sample, latest sample, route changes and completion; native event timestamps avoid polling-time inflation.
- Elapsed starts at advancing payload bytes, excluding preparation, initial waiting, paused/resume connection waits and app downtime. Fully cached export alone does not start transfer time.
- Success clears sender file/text inputs and saved payload references, with names/success/elapsed/path retained. Failed file sends retain names, reason and progress, clear their stale selection and require fresh selection of the exact originals. Croc retains its code for receiver Resume; failed text stays editable. File Restart/Share again is removed. Broadcast retains its sources until sharing ends.
- Spacing/warning position, populated Clear all, Clear text, receiver Clear, reachable-green/unreachable-yellow results and saved-folder Open are implemented. Android 13+ requests notifications once when permission is missing; the existing grant was preserved (no production-data reset to simulate a first install).
- Croc text is piped instead of placed in argv. Large service configs use a private consumed/deleted handoff file, and foreground status precedes config processing. Successful stdout preserves exact text, while failed stdout contributes the terminal error to relay recovery. Bare relay-connection failures now qualify; auth/file/payload failures do not.

Validation:

- 39 JVM tests and the native queue regression passed. Debug and signed release builds passed. Lint: zero errors, 54 warnings.
- Device controls/news-badge regressions passed, including populated Clear all, exclusive Files/Text controls, text retry, fresh URI re-selection without a staging copy, configuration lock and ongoing notification progress.
- Croc sender -> PC: valid Sendme ticket (222 bytes), multiline/CRLF/Unicode/leading-dash text (74 bytes), and 534,890-byte Unicode text matched exactly. PC -> Android: both multiline and 534,890-byte text matched exactly. Completion cleared send payloads and removed sender Restart/received-text presentation.
- PC -> Android Sendme: generated 1 MiB file SHA-256 matched; automatic publication and private-working cleanup passed. Open folder actually opened the DataBeam directory in Android DocumentsUI.
- Android -> PC Sendme: Stop, fresh selection and new ticket passed for 1,048,618 bytes across three files, including empty/Unicode fixtures; collection hash/content matched, completion froze timing/path, removed the notification, unlocked controls and cleared selections.
- Tests exposed an oversized service-handoff foreground deadline, fixed by file handoff/early foreground status. The local Croc test initially selected an unavailable LAN transfer port; text checks pin their test-only USB relay route (`--ip` for the PC receiver, `--no-local` for the PC sender). The final full text run had no new crash. These relay-only text checks do not assert LAN Direct speed.
- Generated data/receipts, test package and ADB reverse mappings were removed. User settings and the completed card matched the host backup byte-for-byte; no app-data reset. The final development APK was installed with matching APK hash, preserving its certificate and data.
- Public beta is a separately signed, non-debuggable release build, version 0.8.0-beta.1, min API 26, ARM64 only. APK v2 signature and 16 KiB zip alignment passed. It uses the same tested application/native source; the existing phone retains the development-signed build to avoid uninstalling its data. Keep the ignored release signing key/properties backed up for future public updates.
- Website download tests passed for native Android beta selection, desktop-API failure fallback and unchanged stable Windows selection. Android beta news and direct download links were added to the site and Troubleshoot.

Artifacts: `mobile-polish/f8fbb43e-4051-4ce0-90b1-a936e2bd3530` (exact text), `mobile-polish/04700d37-b738-426c-9f4d-ee5a6423c001` (receive/Open), `mobile-polish-native-send` (fresh-selection sender), `android-actions/ui` (controls/badge), and final build/signature logs under `android/.tools/smoke/`.

Final DataBeam_android-arm64_beta.apk: 20751353 bytes; SHA-256 `e263df6211582467703493abab16d714d6a6adea00c9aeb825c3211d4fbf659b`.

Final DataBeam_android-arm64_dev-update.apk: 31604938 bytes; SHA-256 `7c31dcd06eb7ca7a73082660094846a7fdce6d1abdaedd0e8a74fa8d889a08a2`.
