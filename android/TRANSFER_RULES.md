# Android transfer rules

Android-only rules. Desktop behavior is the reference and is not modified here.
Update this table and its regression checks whenever transfer-state behavior changes.

## Sender state tree

```text
Select files/folder or Croc text
  -> prepare sources (reference accessible originals; copy unsupported providers)
  -> wait for a receiver
  -> send payload
  -> wait for engine completion acknowledgement
       Croc -> Sent; session ends; clear selected payload; retain concise completion card
       Sendme / EasySendMe normal mode -> close engines; Sent; clear selected payload; retain concise completion card
       Sendme Broadcast enabled -> Sent; sharing session remains open
           next receiver -> Connecting / Sending; new completion summary
           Stop sharing -> Sent; session ends; clear selected payload; retain concise completion card
           app closes -> Sent; session ends; clear selected payload; retain concise completion card
  stopped/failed before acknowledgement -> Sharing stopped; retain reason/progress
       Resend with accessible original sources or text -> start a new sharing session
       Resend with missing or inaccessible sources -> restore available items and ask to reselect missing items
```

| Evidence/state | UI and behavior | Regression coverage |
| --- | --- | --- |
| Preparation or first receiver not yet connected | Preparing / Waiting for a receiver; no success claim | `TransferLabelsTest`, sender restart E2E |
| Reported bytes reach total without engine completion | Do not infer Sent from 100% | `TransferLabelsTest` |
| Native completion acknowledged in normal mode | Close engines automatically; Sent, 100%, frozen elapsed/average/path; remove ongoing notification and unlock configuration | `TransferLabelsTest`, `TransferTimingTest`, sender restart E2E |
| Completed receiver disconnects | Retain its last actual Direct/Relay path as Connection used | `TransferRouteTest`, sender restart E2E |
| Another receiver connects | Use the new connection's path; remove the idle Sent presentation; start a new summary when payload begins | `TransferRouteTest`, `TransferTimingTest`, `TransferLabelsTest` |
| Explicit Sendme Broadcast remains open after completion | Keep the foreground notification and configuration lock; notification says Sent; serve additional receivers until Stop | sender restart E2E, UI controls regression |
| Stop or app closure after acknowledged completion | Keep Sent; clear payload and reselect files for a new session | sender restart E2E, `SenderClosureRegression` |
| Stop/failure/app closure before completion | Sharing stopped. Preserve filenames, reason and progress. Keep the selection during the app session. On Resend, reopen saved source URIs or restore text and start only if the whole payload is available. | `TransferLabelsTest`, `UiControlsRegression`, sender restart E2E |
| Activity reopened while sender service remains alive | Restore selected items and their names from the session | sender restart E2E |
| Older completed sender lacks a completion-time snapshot | Omit an unreliable elapsed value; do not invent its finish time | `TransferTimingTest` |

Broadcast is off by default and is available only for Sendme ticket sharing, as on desktop. Its setting is locked during a session and retained when restarting that session. EasySendMe performs one short-code exchange and ends after success. Before success, both native modes remain available for interrupted receivers to reconnect; failure, disconnection or a reported 100% byte count alone must never trigger successful shutdown.

## Receiver and storage rules

| Branch | Rule | Coverage |
| --- | --- | --- |
| Matching partial/full native cache | Automatically reuse matching verified data on Receive and Resume; no confirmation prompt | receive-cache E2E, EasySendMe UI resume E2E |
| Receiver paused or interrupted | Preserve current data; manual Resume requires a sharing sender | receive-cache E2E, Croc reconnect E2E |
| Source/reference restart | Resend validates original URI access. If a URI is unavailable, ask the sender to select that item again. A new native ticket for unchanged sources keeps the same collection hash. | source/restart E2E, `UiControlsRegression` |
| Saving completed files | Protect existing destination files, verify publication, then remove private working duplicates | storage/cleanup regressions, receive-cache E2E |
| Idle storage cleanup | Preserve current resume/restart data; ask before deleting unknown orphan data; expire eligible known inactive data after seven days when checked | `CleanupRegression`, `TransferCleanupTest` |

No general transfer auto-retry or remote restart of a stopped sender. Croc public-relay recovery remains enabled. Android temporarily disables UDP GSO in Sendme send/receive endpoints at the user's request; desktop defaults are unchanged. Reassess this workaround when upgrading Iroh/Sendme, following `../investigations/sendme-android-direct-relay.md`.

Native card Resume belongs to its saved transfer and ignores unrelated text in the main receive input. It shows a sender-session choice, not a cache confirmation. Pausing the desktop non-broadcast sender can end its one-shot session. The primary New code (EasySendMe) / New ticket (Sendme) action explains that the sender must share the exact same files again. EasySendMe can resolve a replacement ticket even when the short code stayed unchanged. The replacement must have the original content hash. Sender still sharing is the alternative for a live original session, such as Broadcast; it reuses the resolved ticket without repeating the one-shot Croc handoff. Successful lookup consumes the refresh flag and binds the saved ticket to its code. Main Receive uses its entered code/ticket; matching native cached data is automatically discovered even for a new transfer card. Complete local data can be exported without a live sender when a resolved matching ticket is available.


## Quick actions and guidance

- Top Quick actions: Send text selects Croc Text; Broadcast selects Sendme with Broadcast enabled; Send to browser selects Croc Files and shows the getcroc.com receiver link. These change configuration only; they never start sending automatically and are locked while a session is active.
- The ? button shows an unread badge when the remote troubleshooting guide/news revision differs from the last read revision. Opening the guide successfully acknowledges the latest known revision. Checks run off the UI thread; each foreground resume requests a fresh check, coalescing any in-flight request. Ordinary checks use a 30-minute cooldown (five-minute retry on failure). Offline failures preserve unread state. The bundled guide is initially considered read, matching desktop. GuideNewsTest, GuideBadgeRegression and UiControlsRegression cover these rules.

## Timing, route and payload presentation

- Initial zero/cached byte samples, preparation and receiver wait do not start elapsed time. The first advancing payload sample starts it. Pauses, process downtime and reconnect waits are excluded. Native samples carry their emission timestamp.
- Latest known selected Direct/Relay replaces the initial path for both directions; teardown unknown/closed events do not erase it. Route and completion events survive telemetry bursts in the JNI queue.
- Successful sends clear files and text from the draft. Completion retains filenames, success, elapsed time and the latest path. Failed sends retain names, progress and cause. Receive completion opens its original saved directory, including empty-folder-only transfers.
- Croc stdin carries text independently of argument limits. stdout remains exact text and is never parsed as telemetry; stderr supplies live status events, and stdout is inspected for terminal errors only after a failed exit. Oversize capture fails explicitly rather than reporting truncated text as success.
- A bare public-relay connection failure enables bounded relay troubleshooting before payload begins; authentication, file and payload errors do not generate replacement codes. No general auto-retry is enabled.

- Large text configurations stay off Binder and are consumed/deleted from a private handoff file. Foreground status is established before loading the configuration. Android 13+ requests notifications once on first launch; denial is respected.

## Automatic private storage lifecycle

- Success ends a sender session and removes its outgoing fallback copies after descriptors are closed. Failure retains those copies only until the next app foreground entry, when inactive outgoing snapshots and sender blobs are deleted without a Keep/Delete prompt. Broadcast remains active until explicitly ended. Original provider files and public downloads are never sender-cleanup targets.
- Sender blobs are removed when the native session closes. Verified receive export removes its working directory and receive blob store immediately; failed or paused receive caches remain available for Resume.
- File Clear all is a compact right-aligned control visible only with a non-empty selection. Received text has Copy text. A newly shown transfer card scrolls into view at the bottom after layout. Relay results display decoded URL strings, not JSON-escaped strings.

Android Sendme/EasySendMe senders derive the same wrapper as desktop: first 20 Unicode code points of the largest selected item by total file bytes, `_and_more` for multiple selections, then `_databeam`. Equal sizes retain selection order. Folder trees keep their original children. This changes newly created sender tickets; existing ticket/cache contents are not renamed. Croc has no added wrapper.

A valid Sendme ticket in received text exposes an explicit Receive with Sendme action. It switches the receive engine, passes the validated ticket and starts a normal receive; other text does not show the action.

## Guide badge refresh and rendering

Each Activity resume requests a fresh guide check, bypassing the ordinary cooldown while coalescing an existing in-flight request. Requests disable HTTP caches and use a unique query. The read baseline is saved once and survives APK bundle updates; legacy installs use their last checked revision when no seen revision exists. The badge is explicitly above the button, whose elevation animator is disabled. Device regression checks both Z ordering and orange screenshot pixels before testing read acknowledgement.

## Engine visibility and folder navigation

Settings stores at most one disabled engine. Disabling Sendme leaves Croc; disabling Croc leaves Sendme; EasySendMe requires both. Hiding an engine filters the main mode selector, Quick actions, related controls, native-ticket actions and its old transfer card. QR/paste/share entry points cannot silently select a disabled engine. The settings cog retains both toggles and advanced options. At least one engine must remain enabled, and changes are blocked during active/retained transfers. Hidden transfer data is retained, not deleted.

Settings Open and transfer-card Open folder share the same folder intent. It requests an existing file-manager task/activity with NEW_TASK, CLEAR_TOP and SINGLE_TOP, forwards read access only for user-granted tree folders, and applies the same reuse flags to the Downloads fallback. Actual reuse is subject to the chosen file manager's activity handling.

## Croc local route admission

Android receivers check a private/local control endpoint's advertised data ports before joining the sender's real room. The check authenticates a separate disposable room, then probes the advertised ports concurrently within a 1.5-second overall deadline. Reachable local routes remain eligible; an unusable local candidate is rejected before replacing the public connection. This applies to initial local discovery and the later encrypted local-IP upgrade. It does not restart the transfer or change its code. Public relay endpoints and desktop engines are unchanged.

A terminal `could not connect transfer ports` error naming a private/local address is not treated as a public relay outage. Genuine public-relay transport failures still use the existing bounded pre-payload recovery. Preserve the engine's terminal cause in diagnostics before recovery messaging replaces it.
