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
       Croc -> Sent; session ends; Share again available
       Sendme / EasySendMe -> Sent; sharing session remains open
           receiver reconnects -> Connecting / Sending; new completion summary
           Stop sharing -> Sent; session ends; Share again available
           app closes -> Sent; session ends; Share again available
  stopped/failed before acknowledgement -> Sharing stopped; Restart sharing
```

| Evidence/state | UI and behavior | Regression coverage |
| --- | --- | --- |
| Preparation or first receiver not yet connected | Preparing / Waiting for a receiver; no success claim | `TransferLabelsTest`, sender restart E2E |
| Reported bytes reach total without engine completion | Do not infer Sent from 100% | `TransferLabelsTest` |
| Native completion acknowledged; no active receiver | Sent, 100%, frozen elapsed and average; explain that sharing remains open | `TransferLabelsTest`, `TransferTimingTest`, sender restart E2E |
| Completed receiver disconnects | Retain its last actual Direct/Relay path as Connection used | `TransferRouteTest`, sender restart E2E |
| Another receiver connects | Use the new connection's path; remove the idle Sent presentation; start a new summary when payload begins | `TransferRouteTest`, `TransferTimingTest`, `TransferLabelsTest` |
| Sharing remains open after completion | Keep the foreground notification and configuration lock; notification says Sent | sender restart E2E, UI controls regression |
| Stop or app closure after acknowledged completion | Keep Sent; Share again starts a new session | sender restart E2E, `SenderClosureRegression` |
| Stop/failure/app closure before completion | Sharing stopped; local Restart sharing; do not offer sender Resume | `TransferLabelsTest`, sender restart E2E |
| Activity reopened while sender service remains alive | Restore selected items and their names from the session | sender restart E2E |
| Older completed sender lacks a completion-time snapshot | Omit an unreliable elapsed value; do not invent its finish time | `TransferTimingTest` |

Sendme's open session can serve another receiver using its ticket. EasySendMe performs one short-code ticket exchange per session: the current receiver can reconnect using its resolved ticket; a new short-code handoff needs a new share. Do not imply that the already-consumed EasySendMe code is available for a fresh exchange.

## Receiver and storage rules

| Branch | Rule | Coverage |
| --- | --- | --- |
| Matching partial/full native cache | Offer Continue / Delete and start over / Cancel; Continue reuses verified bytes | receive-cache E2E |
| Receiver paused or interrupted | Preserve current data; manual Resume requires a sharing sender | receive-cache E2E, Croc reconnect E2E |
| Source/reference restart | Reopen originals; reuse intact all-copy fallback snapshots; issue a new native ticket with the same hash for unchanged content | source/restart E2E |
| Saving completed files | Protect existing destination files, verify publication, then remove private working duplicates | storage/cleanup regressions, receive-cache E2E |
| Idle storage cleanup | Preserve current resume/restart data; ask before deleting unknown orphan data; expire eligible known inactive data after seven days when checked | `CleanupRegression`, `TransferCleanupTest` |

No general transfer auto-retry or remote restart of a stopped sender. Croc public-relay recovery remains enabled. Do not change transport defaults to hide the documented upstream Iroh routing issue.
