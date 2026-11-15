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
  stopped/failed before acknowledgement -> Sharing stopped; retain reason/progress; reselect exact original files for a new transfer (text stays editable)
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
| Stop/failure/app closure before completion | Sharing stopped; preserve filenames/reason/progress; clear file selection and require fresh selection; keep failed text editable | `TransferLabelsTest`, sender restart E2E |
| Activity reopened while sender service remains alive | Restore selected items and their names from the session | sender restart E2E |
| Older completed sender lacks a completion-time snapshot | Omit an unreliable elapsed value; do not invent its finish time | `TransferTimingTest` |

Broadcast is off by default and is available only for Sendme ticket sharing, as on desktop. Its setting is locked during a session and retained when restarting that session. EasySendMe performs one short-code exchange and ends after success. Before success, both native modes remain available for interrupted receivers to reconnect; failure, disconnection or a reported 100% byte count alone must never trigger successful shutdown.

## Receiver and storage rules

| Branch | Rule | Coverage |
| --- | --- | --- |
| Matching partial/full native cache | Offer Continue / Delete and start over / Cancel; Continue reuses verified bytes | receive-cache E2E |
| Receiver paused or interrupted | Preserve current data; manual Resume requires a sharing sender | receive-cache E2E, Croc reconnect E2E |
| Source/reference restart | No file Restart button. Freshly reselected originals issue a new native ticket with the same hash for unchanged content; expired grants are never silently reused | source/restart E2E |
| Saving completed files | Protect existing destination files, verify publication, then remove private working duplicates | storage/cleanup regressions, receive-cache E2E |
| Idle storage cleanup | Preserve current resume/restart data; ask before deleting unknown orphan data; expire eligible known inactive data after seven days when checked | `CleanupRegression`, `TransferCleanupTest` |

No general transfer auto-retry or remote restart of a stopped sender. Croc public-relay recovery remains enabled. Android temporarily disables UDP GSO in Sendme send/receive endpoints at the user's request; desktop defaults are unchanged. Reassess this workaround when upgrading Iroh/Sendme, following `../investigations/sendme-android-direct-relay.md`.


## Quick actions and guidance

- Top Quick actions: Send text selects Croc Text; Broadcast selects Sendme with Broadcast enabled; Send to browser selects Croc Files and shows the getcroc.com receiver link. These change configuration only; they never start sending automatically and are locked while a session is active.
- The ? button shows an unread badge when the remote troubleshooting guide/news revision differs from the last read revision. Opening the guide successfully acknowledges the latest known revision. Checks run off the UI thread, at startup/resume when due, then at 30-minute intervals (five-minute retry on failure). Offline failures preserve unread state. The bundled guide is initially considered read, matching desktop. GuideNewsTest, GuideBadgeRegression and UiControlsRegression cover these rules.

## Timing, route and payload presentation

- Initial zero/cached byte samples, preparation and receiver wait do not start elapsed time. The first advancing payload sample starts it. Pauses, process downtime and reconnect waits are excluded. Native samples carry their emission timestamp.
- Latest known selected Direct/Relay replaces the initial path for both directions; teardown unknown/closed events do not erase it. Route and completion events survive telemetry bursts in the JNI queue.
- Successful sends clear files/text in the draft and saved session. Completion retains filenames, success, elapsed and the latest path. Failed files clear only the unreliable selection; the card retains names, progress and cause, and asks to reselect the exact originals. Failed text stays editable. Receive completion opens its original saved directory, including empty-folder-only transfers.
- Croc stdin carries text independently of argument limits. stdout remains exact text and is never parsed as telemetry; stderr supplies live status events, and stdout is inspected for terminal errors only after a failed exit. Oversize capture fails explicitly rather than reporting truncated text as success.
- A bare public-relay connection failure enables bounded relay troubleshooting before payload begins; authentication, file and payload errors do not generate replacement codes. No general auto-retry is enabled.

- Large text configurations stay off Binder and are consumed/deleted from a private handoff file. Foreground status is established before loading the configuration. Android 13+ requests notifications once on first launch; denial is respected.

## Automatic private storage lifecycle

- Success ends a sender session and removes its outgoing fallback copies after descriptors are closed. Failure retains those copies only until the next app foreground entry, when inactive outgoing snapshots and sender blobs are deleted without a Keep/Delete prompt. Broadcast remains active until explicitly ended. Original provider files and public downloads are never sender-cleanup targets.
- Sender blobs are removed when the native session closes. Verified receive export removes its working directory and receive blob store immediately; failed or paused receive caches remain available for Resume.
- File Clear all is a compact right-aligned control visible only with a non-empty selection. Received text has Copy text. A newly shown transfer card scrolls into view at the bottom after layout. Relay results display decoded URL strings, not JSON-escaped strings.

Android Sendme/EasySendMe senders derive the same wrapper as desktop: first 20 Unicode code points of the largest selected item by total file bytes, `_and_more` for multiple selections, then `_databeam`. Equal sizes retain selection order. Folder trees keep their original children. This changes newly created sender tickets; existing ticket/cache contents are not renamed. Croc has no added wrapper.

A valid Sendme ticket in received text exposes an explicit Receive with Sendme action. It switches the receive engine, passes the validated ticket and starts a normal receive; other text does not show the action.
