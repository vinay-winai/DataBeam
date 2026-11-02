# Sendme 0.37.1 synchronization

## Whole-selection preparation and live rates (November 2)

Croc preparation now uses exact local engine telemetry for the full selection,
including files too small for CLI hashing bars and repeated filenames. One
aggregate bar shows completed bytes / selection bytes and overall ETA; file
transitions do not reset it. The sender's share code still waits for preparation
to finish. Hash algorithms, transfer frames, and peer compatibility are unchanged.
The opt-in `DATABEAM_CROC_PROGRESS=1` environment flag enables local stderr
messages only (`DATABEAM_PREPARE_V1`, `DATABEAM_PREPARED_V1`, and
`DATABEAM_RATE_V1`). The explicit ready marker prevents terminal redraws flushed
by telemetry from publishing the share code during hashing.

Preparation and transfer rates now use a **one-second rolling window**, sampled
about every 100 ms. Samples are time weighted, including partial intervals at
the window boundary; a short task uses the measurements available so far.
Completed bytes and percentage remain live. Croc wire-rate counters
exclude skipped/resumed bytes added to its terminal bar. Sendme likewise counts
newly transferred bytes, preserving resume baselines. The GUI applies each rate
directly and clears stale rates after a one-second gap; ETA uses that current
rate. Completed-transfer average summaries remain averages. Rate history stays
bounded, and resets preserve cached-byte baselines. The window applies to Croc,
Sendme, and EasySendMe preparation and transfer, on both sending and receiving.

The Croc telemetry tests check aggregate accounting, small files, duplicate
names, unchanged digests, cancellation, and abrupt rate changes. App regression
tests check total-selection percentages/ETA, upload isolation, and that terminal
averages cannot overwrite current rates. Source checks passed: **116 app tests**,
**30 native Sendme tests**, and the Croc Go tests. Rate tests cover burst
smoothing, expiry after one second, irregular sample intervals, slow updates,
stalls, resumed baselines, and bounded history. Logs:
`target/one-second-app-tests.log`, `target/one-second-native-tests.log`,
`target/one-second-croc-bundle.log`.
Live Croc relay round trips, EasySendMe ticket/payload handoff, and Sendme
Stop/restart resume passed (`target/aggregate-live-*.log`). The resume test
continued from 13,582,336 bytes on both peers with an initial sender rate of zero
after the one-second-window change (`target/one-second-live-*.log`).

A real 5 GiB file (5,368,709,120 bytes) produced 24 monotonic Croc preparation
samples and reached the exact total before readiness. In the isolated Windows
GUI, the aggregate bar displayed 2.0 / 5.0 GB (40.4%) and 4.0 / 5.0 GB (79.9%),
with rate and ETA, then changed to waiting and revealed the code. Report:
`target/croc-preparation-observe/croc-preparation-5gib.json`. Small real payloads
cover the subsequent network progress transition; the 5 GiB file is a local
preparation observation, not a 5 GiB relay throughput benchmark.

The earlier Windows GUI check displayed Sendme preparation at 13.5%, 30.6%, and
49.9% for the same 5 GiB file, with ETA changing from 4s to 3s to 2s. It then
changed to waiting and revealed its ticket. Both GUI tests used scratch settings,
Croc configuration, and caches under `target/croc-preparation-observe`. The
preparation panel no longer shows broadcast captions for a single transfer.
The GUI repeat for the one-second-window build was unavailable because Computer
Use app approval timed out; the 5 GiB engine run and live backend checks passed.
Local multi-instance EXE: `dist/databeam_v0.7.0_windows-x64-one-second-window.exe`.

## Stop/receive resume follow-up (November 2)

A real GUI-backend Stop/restart test exposed a gap in the previous resume
validation: the earlier test reopened a cleanly saved partial store, rather
than stopping a live DataBeam receiver. The Stop path aborted the download
task and skipped the store's graceful shutdown, which can discard recent
writes and ephemeral partial-range state. The new test failed before the fix:
the receiver restarted at zero despite a cache directory being present.

Receiver cancellation now reaches the native download through a cooperative
flag. It drops the download future, closes the endpoint, and awaits blob-store
shutdown before the backend worker exits. Cancellation shutdown failures retain
their error details. JSON settings, ticket-map schema, content hashes, store
format, and receiver cache locations are unchanged.

Verified with a 256 MiB temporary payload through the real DataBeam backend:
Stop a partially downloaded receiver, stop/restart the sender with the same
payload, and receive using the new ticket. The collection hash remains identical,
both progress bars begin above zero, cached sender bytes have zero upload rate,
and the received contents match byte for byte. The final run resumed at
1,900,544 cached payload bytes. Logs: `target/sendme-stop-resume-before.log`
(reproduced failure) and `target/sendme-stop-resume-verified.log` (fixed).
App regressions: `target/sendme-stop-resume-final-app-tests.log`;
native regressions: `target/sendme-stop-resume-native-regressions.log`.
Replacement local artifact: `dist/databeam_v0.7.0_windows-x64-resume-fix.exe`,
with SHA-256 alongside it. The earlier `windows-x64-test.exe` is retained for
comparison and does not contain the receiver cancellation fix.

This also distinguishes resume data from endpoint identity: stopping a receiver
can terminate a one-shot sender. Its old ticket cannot connect after that sender
exits; a restarted sender's new ticket can still reuse the same payload cache.

## November 2, 2026 update

Compared the previous reference `21881bd` with official Sendme main at
[`5aad32a`](https://github.com/n0-computer/sendme/commit/5aad32a1b88b5e51b98c6100c85c3a0b312a7ebe).
The only intervening commit is dependency maintenance, PR #159; upstream's
application source and latest release version (0.37.1) are unchanged.

- Both lockfiles now match upstream's iroh / iroh-base / iroh-dns / iroh-relay
  **1.4.1**, noq / noq-proto / noq-udp **1.4.0**, and Tokio **1.54.1**.
  The previous iroh 1.0.2 retention described below is superseded by this update.
- Keep iroh-blobs **0.103.1**, its upload authorization fix, and DataBeam's
  delivery acknowledgment, retry, CLI collection wrapping, and route display.
- Adopt the applicable checkout, Rust toolchain, and Rust cache action pins
  from upstream's workflow update; retain DataBeam's packaging and platform jobs.
  Upstream-only zizmor installation changes do not apply to this repository.
- DataBeam additions: cooperative cancellation during directory scanning and
  file imports, with endpoint/store shutdown and share-specific cache cleanup;
  separate preparation byte progress, hashing rate, and ETA for Sendme and
  EasySendMe. Preparation never writes network-transfer counters. An OS file
  operation already in progress finishes before its worker can stop.
- Sender resume display: derive logical completion from the receiver's existing
  missing-chunk requests, including fully cached files and holes in partial
  files. Deduplicate file progress, isolate receiver sessions, and keep cached
  bytes out of upload speed. No wire-protocol extension is required. This is a
  DataBeam fix for the symptom tracked by
  [upstream issue #2](https://github.com/n0-computer/sendme/issues/2), which is
  still open and has no comments as of this comparison.
- Treat a reset during size discovery as metadata-only, not a failed payload.
  Size probes can stop after learning the sizes; a fatal sender error at that
  point could close the connection before the real download. Aborted payload
  requests still fail. Added a deterministic provider-event regression for both.
- Croc UI: separate current-file hashing progress from uploaded bytes, remove
  inferred verification/finalization after four seconds of silence, and delay
  code display until the final preparation summary. CR redraws are not readiness
  signals; newline and Windows CRLF summaries are. Preparation shows current-file
  bytes, rate, and ETA. The Croc binary/protocol is unchanged.

Validated locally on Windows:

- DataBeam with `release-single-instance`: **112 passed**, four unrelated/manual
  tests ignored. Includes preparation-to-transfer transitions for both engines,
  one-shot and broadcast modes, cancellation, and stale-event isolation.
- Native Sendme library: **26 passed**, one public-relay test normally ignored.
  Includes cancellation after the first read, file handle / UDP port release,
  parallel preparation byte accounting, direct transfer, rejected uploads,
  CLI collection export, delayed delivery acknowledgment, and metadata-reset
  regressions. The restart test closes both engines after caching a complete
  file plus half of another, then verifies the new sender starts at 75% with
  zero upload speed and both received files match exactly.
- Ran the ignored relay test explicitly: **passed**, 262,144 payload bytes
  verified with the sender's IP transports disabled, receiving a relay route event.
- Ran the ignored real EasySendMe handoff explicitly: **passed**, 262,144
  payload bytes verified after Croc ticket exchange. Preparation reaches its
  total before the backend publishes the ticket.
- Additional real Croc text/file cases passed with delayed code publication.
  EasySendMe initially reproduced connection loss from a metadata-only reset;
  after the classification fix, the isolated test and three consecutive repeats
  passed without continuous event logging. No arbitrary shutdown delay was added.

Logs: `target/databeam-v070-final-regressions.log`,
`target/sendme-progress-probe-regressions.log`,
`target/sendme-iroh141-relay-test.log`,
`target/databeam-v070-live-progress-tests.log` (includes the initial EasySendMe
failure and successful Croc cases), and
`target/databeam-v070-probe-fixed-repeat-{1,2,3}.log`.
These tests do not establish behavior on macOS/Linux or on the user's VPN setup;
those environments have not been tested for this update. CI workflow changes
have not been run on GitHub. No release or push was performed.
After the user's go-ahead, the local multi-instance v0.7.0 test EXE was rebuilt
with these progress/code-readiness fixes. Artifact:
`dist/databeam_v0.7.0_windows-x64-test.exe` (checksum alongside it).
Release compilation succeeded, and the copied EXE's checksum and Windows
file/product version metadata were verified. This has not been pushed or released.

## October 26, 2026 update

Compared on 2026-10-26 against official
[`n0-computer/sendme` at `21881bd`](https://github.com/n0-computer/sendme/commit/21881bd),
updated from the previous reference at `8dda1e5`.

DataBeam embeds a customized alt-sendme library. This update ports applicable
upstream changes into that library; it does not replace the engine with the CLI.
The embedded engine version is now 0.37.1; this sync ships in DataBeam 0.6.6.

## Applied

- Require `iroh-blobs >= 0.103.1` and lock both builds to 0.103.1.
  [Upstream #144](https://github.com/n0-computer/sendme/pull/144) fixes a request-mode
  authorization bug: enabling download progress notifications also allowed peers
  to push arbitrary blobs into the sender's store. Push remains disabled by default.
- Update both lockfiles to `rustls 0.23.46`, `rustls-webpki 0.103.16`, and
  `webpki-roots` / `webpki-root-certs 1.0.11` (the latter are newer than upstream's
  1.0.10). These include the fixes carried by
  [#149](https://github.com/n0-computer/sendme/pull/149) and
  [#151](https://github.com/n0-computer/sendme/pull/151).
- Update Tokio to 1.53.3, matching the official lockfile, and update yanked
  `chacha20`, `der`, and `spin` versions following
  [upstream's lockfile cleanup](https://github.com/n0-computer/sendme/commit/0661eb2).
- Port [#148](https://github.com/n0-computer/sendme/pull/148): observe the selected
  path with `Connection::paths_stream()`. Sendme and EasySendMe receivers show
  **Transfer mode: Direct** or **Transfer mode: Relay**, including route changes,
  in the running transfer details, completion summary, and log. No route is inferred
  from ticket addresses or the Croc code-exchange leg. Clear it between transfers
  and when the connection has no selected path. The watcher is cancelled when
  the network download ends or is cancelled. Sender-side route display is not
  added; upstream's change also applies to receiving only.
- Give CI read-only repository permissions and disable checkout credential
  persistence, following the applicable parts of upstream's CI hardening.
- Add native loopback transfer/upload-rejection coverage to CI, alongside a GUI
  regression test for relay/direct changes and clearing route state.

## Kept specific to DataBeam

Multi-file selection, custom short codes, broadcast sharing, retry/resume,
configurable caches, and destination conflict handling remain in the customized
engine. Existing iroh 1.0.2 is retained: the route API and patched blob library
work with it. This is a targeted synchronization, not a full dependency refresh
to upstream's iroh 1.3.0 graph.

The CLI help improvement and terminal key-event handling do not apply to the
native GUI. Upstream's release workflow and contribution policy are specific to
its repository and are not copied into DataBeam.

## Verification

```powershell
cargo check --locked
cargo test --locked
cargo test --manifest-path third_party/sendme/Cargo.toml --lib --no-default-features --locked --target-dir target
```

The loopback test verifies received file bytes, a real direct-path event, and
rejection of a push into the actual sharing engine. The GUI test verifies both
route labels, reconnect/reset behavior, and isolation from Croc's route logs.
This does not replace a separate-device or public-relay connectivity test.
