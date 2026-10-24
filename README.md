# DataBeam

The ultimate desktop GUI app combining the convenience of Croc with the high speeds and resilience (across the network changes) features of Sendme. Send files/data anywhere instantly across wifi/network changes, resume downloads after network drops and power failure shutdowns.

It gives you:
- the EasySendMe mode that combines,
- the convenience of Croc PAKE's small secret code
- the modern transfer model (iroh blobs, blake3, hole-punching) of Sendme


You can:
- use EasySendMe, where Sendme handles the transfer but user shares a short croc code (can even create 7 length custom code) instead of the super long sendme ticket
- use Croc directly
- there is also a sendme broadcast mode which acts as a file server.

## Features

- EasySendMe mode: Croc-style convenience with the modern improvements of Sendme
- auto-retry system when network disconnections.
- automatic safe file overwrite confilct resolution system.
- portable executables with the patched Croc engine included
- standalone `croc` mode and `sendme` modes are still available if you want to use them for specific situations
- sending text in Croc mode
- support for custom code as short as 7 characters in croc and easysendme modes

- support for transferring multiple files/folders at once
- QR code generation for transfer codes/tickets
- modern native desktop UI with no browser runtime
- visible transfer logs inside the app

## Screenshot gallery

Actual Windows v0.6.5 captures using a local build that permits multiple instances. Transfers used generated dummy files on one PC; received files were checked with SHA-256. The galleries show file/folder and text sharing, QR codes, automatic retry, manual resume, conflict protection, and two simultaneous broadcast receivers.

[Browse the interactive gallery](https://databeam.refining.online/gallery.html) · [Gallery source](docs/gallery.html)

| Multiple-file sharing | Manual Stop and Retry | Two broadcast receivers |
| --- | --- | --- |
| ![EazySendme sends three files totaling 650 MiB](docs/screenshots/v0.6.5/29-eazysendme-three-files-ready.jpg) | ![Stopped download retains its cache and offers Retry](docs/screenshots/v0.6.5/45-manual-resume-stopped.jpg) | ![Broadcast sender showing two transfers in progress](docs/screenshots/v0.6.5/38-broadcast-two-transfers-in-progress.jpg) |

Expand a section below to see all 48 screenshots. Click an image for its full-size capture.

<details>
<summary><strong>EazySendme: multiple files, custom codes, and QR</strong> (9 screenshots)</summary>

### EazySendme: three files in one batch

The 100, 250, and 300 MiB files selected together: 650 MiB total, with automatic retry enabled.

<a href="docs/screenshots/v0.6.5/29-eazysendme-three-files-ready.jpg"><img src="docs/screenshots/v0.6.5/29-eazysendme-three-files-ready.jpg" alt="EazySendme: three files in one batch" width="560"></a>

### Multiple-file download in progress

Actual hybrid download of the 650 MiB batch after the Croc ticket exchange.

<a href="docs/screenshots/v0.6.5/30-eazysendme-multiple-file-download.jpg"><img src="docs/screenshots/v0.6.5/30-eazysendme-multiple-file-download.jpg" alt="Multiple-file download in progress" width="560"></a>

### EazySendme receive options

Short code input, recent-code controls, cache choices, and automatic retry setting.

<a href="docs/screenshots/v0.6.5/22-eazysendme-receive-options.jpg"><img src="docs/screenshots/v0.6.5/22-eazysendme-receive-options.jpg" alt="EazySendme receive options" width="560"></a>

### Custom code and 250 MiB payload

Hybrid transfer uses a custom short code to exchange a native Sendme ticket through Croc.

<a href="docs/screenshots/v0.6.5/23-eazysendme-custom-code-ready.jpg"><img src="docs/screenshots/v0.6.5/23-eazysendme-custom-code-ready.jpg" alt="Custom code and 250 MiB payload" width="560"></a>

### Hybrid download progress

Actual 250 MiB transfer after the Croc ticket handoff.

<a href="docs/screenshots/v0.6.5/24-eazysendme-receive-progress.jpg"><img src="docs/screenshots/v0.6.5/24-eazysendme-receive-progress.jpg" alt="Hybrid download progress" width="560"></a>

### Hybrid receive complete

The received 250 MiB file matches the original by SHA-256.

<a href="docs/screenshots/v0.6.5/25-eazysendme-receive-complete.jpg"><img src="docs/screenshots/v0.6.5/25-eazysendme-receive-complete.jpg" alt="Hybrid receive complete" width="560"></a>

### Croc to Sendme handoff log

Ticket received via Croc, followed by Sendme download completion. Cache status and retry controls are visible.

<a href="docs/screenshots/v0.6.5/26-eazysendme-ticket-handoff-log.jpg"><img src="docs/screenshots/v0.6.5/26-eazysendme-ticket-handoff-log.jpg" alt="Croc to Sendme handoff log" width="560"></a>

### EazySendme QR sharing

QR code for the short sharing code.

<a href="docs/screenshots/v0.6.5/27-eazysendme-qr.jpg"><img src="docs/screenshots/v0.6.5/27-eazysendme-qr.jpg" alt="EazySendme QR sharing" width="560"></a>

### Hybrid sender complete

The sender reports the full 250 MiB uploaded successfully.

<a href="docs/screenshots/v0.6.5/28-eazysendme-send-complete.jpg"><img src="docs/screenshots/v0.6.5/28-eazysendme-send-complete.jpg" alt="Hybrid sender complete" width="560"></a>

</details>

<details>
<summary><strong>Manual Stop → Retry → resume</strong> (7 screenshots)</summary>

### Manual resume: automatic retry disabled

Both transfer windows had auto-retry off. A fresh output/cache folder was used for this test.

<a href="docs/screenshots/v0.6.5/43-manual-resume-auto-retry-off.jpg"><img src="docs/screenshots/v0.6.5/43-manual-resume-auto-retry-off.jpg" alt="Manual resume: automatic retry disabled" width="560"></a>

### Download before pressing Stop

Actual download of the existing twelve-file 3,600 MiB dummy batch, before the deliberate interruption.

<a href="docs/screenshots/v0.6.5/44-manual-resume-download-before-stop.jpg"><img src="docs/screenshots/v0.6.5/44-manual-resume-download-before-stop.jpg" alt="Download before pressing Stop" width="560"></a>

### Stopped download with Retry available

Stop cancelled the receiver at roughly 52%. About 1.97 GB of partial cache remained on disk. Auto-retry stayed off; the app waited for manual input.

<a href="docs/screenshots/v0.6.5/45-manual-resume-stopped.jpg"><img src="docs/screenshots/v0.6.5/45-manual-resume-stopped.jpg" alt="Stopped download with Retry available" width="560"></a>

### Sender reconnects with manual Retry

EazySendme is one-shot, so stopping the receiver also ended the sender session. Retry was pressed on the sender to share the unchanged payload again.

<a href="docs/screenshots/v0.6.5/46-manual-resume-sender-retry.jpg"><img src="docs/screenshots/v0.6.5/46-manual-resume-sender-retry.jpg" alt="Sender reconnects with manual Retry" width="560"></a>

### Receiver Retry pressed manually

Retry was pressed on the stopped receiver. It reconnected through the short code, keeping the partial blob data on disk.

<a href="docs/screenshots/v0.6.5/47-manual-resume-retry-cache-check.jpg"><img src="docs/screenshots/v0.6.5/47-manual-resume-retry-cache-check.jpg" alt="Receiver Retry pressed manually" width="560"></a>

### Download continued after Retry

The resumed download progressed beyond the stopped position, with auto-retry still off. This is the Stop + Retry workflow rather than a dedicated Pause button.

<a href="docs/screenshots/v0.6.5/48-manual-resume-download-continued.jpg"><img src="docs/screenshots/v0.6.5/48-manual-resume-download-continued.jpg" alt="Download continued after Retry" width="560"></a>

### Resumed download completed and verified

All twelve exported files matched their source SHA-256 hashes after the interrupted transfer and manual retries.

<a href="docs/screenshots/v0.6.5/49-manual-resume-completed.jpg"><img src="docs/screenshots/v0.6.5/49-manual-resume-completed.jpg" alt="Resumed download completed and verified" width="560"></a>

</details>

<details>
<summary><strong>Automatic retry and overwrite protection</strong> (5 screenshots)</summary>

### Automatic retry after an induced failure

A test fixture temporarily blocked the output path. The app scheduled Auto-retrying 1/3 in 36s. The path was restored during this wait; Retry was not clicked.

<a href="docs/screenshots/v0.6.5/31-eazysendme-auto-retry-countdown.jpg"><img src="docs/screenshots/v0.6.5/31-eazysendme-auto-retry-countdown.jpg" alt="Automatic retry after an induced failure" width="560"></a>

### Automatic recovery completed

The timer retried automatically and exported the cached batch. All three files were verified. A display bug reports only 300 MiB in this cached-export completion summary, although the exported total is 650 MiB.

<a href="docs/screenshots/v0.6.5/32-eazysendme-auto-retry-recovered.jpg"><img src="docs/screenshots/v0.6.5/32-eazysendme-auto-retry-recovered.jpg" alt="Automatic recovery completed" width="560"></a>

### Recovery used complete local blobs

The real app log shows cached blob verification and local export, without another sender ticket exchange.

<a href="docs/screenshots/v0.6.5/33-eazysendme-cache-recovery-log.jpg"><img src="docs/screenshots/v0.6.5/33-eazysendme-cache-recovery-log.jpg" alt="Recovery used complete local blobs" width="560"></a>

### Overwrite protection: both folders retained

The existing destination folder remains intact. DataBeam automatically created 1_DataBeam-test-300MB._and_more_databeam for the incoming batch.

<a href="docs/screenshots/v0.6.5/34-eazysendme-conflict-folder-preserved.jpg"><img src="docs/screenshots/v0.6.5/34-eazysendme-conflict-folder-preserved.jpg" alt="Overwrite protection: both folders retained" width="560"></a>

### All three files exported into the new folder

The renamed incoming folder contains the 100, 250, and 300 MiB files. All hashes match, and the existing sentinel file is unchanged.

<a href="docs/screenshots/v0.6.5/35-eazysendme-all-three-exported.jpg"><img src="docs/screenshots/v0.6.5/35-eazysendme-all-three-exported.jpg" alt="All three files exported into the new folder" width="560"></a>

</details>

<details>
<summary><strong>Broadcast: one sender and two receivers</strong> (7 screenshots)</summary>

### One sender, two receivers active

Live sender capture explicitly showing 2 transfers in progress. Both receivers downloaded the same 3,600 MiB batch, containing twelve distinct 300 MiB files.

<a href="docs/screenshots/v0.6.5/38-broadcast-two-transfers-in-progress.jpg"><img src="docs/screenshots/v0.6.5/38-broadcast-two-transfers-in-progress.jpg" alt="One sender, two receivers active" width="560"></a>

### Receiver one: separate output and cache

First receiver prepared with its own destination/cache. This setup capture shows the initial 650 MiB session; the longer session used the same settings and a new shared ticket.

<a href="docs/screenshots/v0.6.5/36-broadcast-receiver-one-ready.jpg"><img src="docs/screenshots/v0.6.5/36-broadcast-receiver-one-ready.jpg" alt="Receiver one: separate output and cache" width="560"></a>

### Receiver two: separate output and cache

Second receiver prepared with its own destination/cache. Both later downloaded the longer batch concurrently.

<a href="docs/screenshots/v0.6.5/37-broadcast-receiver-two-ready.jpg"><img src="docs/screenshots/v0.6.5/37-broadcast-receiver-two-ready.jpg" alt="Receiver two: separate output and cache" width="560"></a>

### Receiver one downloading concurrently

One of the two real receiver windows during the 3,600 MiB broadcast.

<a href="docs/screenshots/v0.6.5/39-broadcast-receiver-one-progress.jpg"><img src="docs/screenshots/v0.6.5/39-broadcast-receiver-one-progress.jpg" alt="Receiver one downloading concurrently" width="560"></a>

### Receiver two downloading concurrently

The other receiver window during the same shared broadcast.

<a href="docs/screenshots/v0.6.5/40-broadcast-receiver-two-progress.jpg"><img src="docs/screenshots/v0.6.5/40-broadcast-receiver-two-progress.jpg" alt="Receiver two downloading concurrently" width="560"></a>

### Receiver one completed

All twelve 300 MiB files were received and checked against their original SHA-256 hashes.

<a href="docs/screenshots/v0.6.5/41-broadcast-receiver-one-complete.jpg"><img src="docs/screenshots/v0.6.5/41-broadcast-receiver-one-complete.jpg" alt="Receiver one completed" width="560"></a>

### Receiver two completed

The second copy of the full twelve-file batch also passed SHA-256 verification.

<a href="docs/screenshots/v0.6.5/42-broadcast-receiver-two-complete.jpg"><img src="docs/screenshots/v0.6.5/42-broadcast-receiver-two-complete.jpg" alt="Receiver two completed" width="560"></a>

</details>

<details>
<summary><strong>Croc: files, direct transfers, and QR</strong> (7 screenshots)</summary>

### Three test files ready

100, 250, and 300 MiB of random binary data; 650 MiB total. The app labels these sizes MB.

<a href="docs/screenshots/v0.6.5/03-croc-files-ready.jpg"><img src="docs/screenshots/v0.6.5/03-croc-files-ready.jpg" alt="Three test files ready" width="560"></a>

### Random three-word code

Fresh CLI-style code with Copy, Show QR, and transfer log controls.

<a href="docs/screenshots/v0.6.5/04-croc-sharing-code.jpg"><img src="docs/screenshots/v0.6.5/04-croc-sharing-code.jpg" alt="Random three-word code" width="560"></a>

### Croc QR sharing

A QR code for the current transfer code.

<a href="docs/screenshots/v0.6.5/05-croc-qr-code.jpg"><img src="docs/screenshots/v0.6.5/05-croc-qr-code.jpg" alt="Croc QR sharing" width="560"></a>

### Receive code and destination

Second app instance configured to receive into this demo folder.

<a href="docs/screenshots/v0.6.5/06-croc-receive-ready.jpg"><img src="docs/screenshots/v0.6.5/06-croc-receive-ready.jpg" alt="Receive code and destination" width="560"></a>

### Direct transfer in progress

Actual 650 MiB transfer between two app instances on this PC. Displayed rates reflect this same-PC test.

<a href="docs/screenshots/v0.6.5/07-croc-transfer-progress.jpg"><img src="docs/screenshots/v0.6.5/07-croc-transfer-progress.jpg" alt="Direct transfer in progress" width="560"></a>

### 650 MiB received successfully

All three received files match their originals by SHA-256. Route: direct peer link.

<a href="docs/screenshots/v0.6.5/08-croc-receive-complete.jpg"><img src="docs/screenshots/v0.6.5/08-croc-receive-complete.jpg" alt="650 MiB received successfully" width="560"></a>

### Sender completion and log

Completed batch with upload summary and direct-route log.

<a href="docs/screenshots/v0.6.5/09-croc-send-complete.jpg"><img src="docs/screenshots/v0.6.5/09-croc-send-complete.jpg" alt="Sender completion and log" width="560"></a>

</details>

<details>
<summary><strong>Text sharing</strong> (3 screenshots)</summary>

### Text composition

Sample text entered through the text payload mode.

<a href="docs/screenshots/v0.6.5/10-croc-text-compose.jpg"><img src="docs/screenshots/v0.6.5/10-croc-text-compose.jpg" alt="Text composition" width="560"></a>

### Received text popup

Exact demo message received in the second instance, with Copy Text.

<a href="docs/screenshots/v0.6.5/11-croc-text-received-popup.jpg"><img src="docs/screenshots/v0.6.5/11-croc-text-received-popup.jpg" alt="Received text popup" width="560"></a>

### Inline received text

The receive screen retains the message and supports reopening the popup.

<a href="docs/screenshots/v0.6.5/12-croc-text-received.jpg"><img src="docs/screenshots/v0.6.5/12-croc-text-received.jpg" alt="Inline received text" width="560"></a>

</details>

<details>
<summary><strong>Standalone Sendme and broadcast controls</strong> (7 screenshots)</summary>

### Sendme receive and cache options

Ticket input, output folder, and OS temp / downloads / custom blob-cache choices.

<a href="docs/screenshots/v0.6.5/15-sendme-receive-settings.jpg"><img src="docs/screenshots/v0.6.5/15-sendme-receive-settings.jpg" alt="Sendme receive and cache options" width="560"></a>

### Folder sharing and broadcast mode

A 650 MiB folder selected; Stop after single transfer is unchecked.

<a href="docs/screenshots/v0.6.5/16-sendme-broadcast-setup.jpg"><img src="docs/screenshots/v0.6.5/16-sendme-broadcast-setup.jpg" alt="Folder sharing and broadcast mode" width="560"></a>

### Live broadcast ticket

Native Sendme session ready for receivers, with ticket sharing and log.

<a href="docs/screenshots/v0.6.5/17-sendme-broadcast-ticket.jpg"><img src="docs/screenshots/v0.6.5/17-sendme-broadcast-ticket.jpg" alt="Live broadcast ticket" width="560"></a>

### Sendme ticket QR

QR sharing for a native Sendme ticket.

<a href="docs/screenshots/v0.6.5/18-sendme-ticket-qr.jpg"><img src="docs/screenshots/v0.6.5/18-sendme-ticket-qr.jpg" alt="Sendme ticket QR" width="560"></a>

### Sendme folder download progress

Actual second-instance folder transfer, with rate and ETA.

<a href="docs/screenshots/v0.6.5/19-sendme-folder-receive.jpg"><img src="docs/screenshots/v0.6.5/19-sendme-folder-receive.jpg" alt="Sendme folder download progress" width="560"></a>

### Broadcast remains online

After one receiver finished, the sender continued waiting for more receivers. The demo broadcast was then stopped.

<a href="docs/screenshots/v0.6.5/20-sendme-broadcast-stays-online.jpg"><img src="docs/screenshots/v0.6.5/20-sendme-broadcast-stays-online.jpg" alt="Broadcast remains online" width="560"></a>

### Sendme folder complete

All three exported files match their originals by SHA-256.

<a href="docs/screenshots/v0.6.5/21-sendme-receive-complete.jpg"><img src="docs/screenshots/v0.6.5/21-sendme-receive-complete.jpg" alt="Sendme folder complete" width="560"></a>

</details>

<details>
<summary><strong>Home, engines, and Windows settings</strong> (3 screenshots)</summary>

### Home and quick actions

EazySendme default, file sending, receiving, broadcast, and text sharing.

<a href="docs/screenshots/v0.6.5/01-home.jpg"><img src="docs/screenshots/v0.6.5/01-home.jpg" alt="Home and quick actions" width="560"></a>

### Three transfer engines

Installed Sendme and patched managed Croc versions, plus EazySendme.

<a href="docs/screenshots/v0.6.5/02-engines.jpg"><img src="docs/screenshots/v0.6.5/02-engines.jpg" alt="Three transfer engines" width="560"></a>

### Windows release settings

Live in system tray option. Pinned Croc updater and release tray debugging controls are hidden.

<a href="docs/screenshots/v0.6.5/13-windows-settings.jpg"><img src="docs/screenshots/v0.6.5/13-windows-settings.jpg" alt="Windows release settings" width="560"></a>

</details>

The resume demo uses **Stop + Retry** with automatic retry off. EazySendme ends its one-shot sender session after a disconnect, so Retry was pressed on both peers. Existing receiver content was preserved during the conflict test; the incoming batch was exported to a new `1_`-prefixed folder.

Known display issue shown in the captures: cached EazySendme export reports 300 MiB in the completion summary for a three-file 650 MiB batch, although all three exported files pass verification. Same-PC transfer rates are not Wi-Fi or Internet benchmarks.

[Initial transfer verification](docs/gallery-data/verification.json) · [Retry, conflict, and broadcast verification](docs/gallery-data/extended/verification.json) · [Manual-resume verification](docs/gallery-data/extended/manual-resume-verification.json)

DataBeam bundles **Croc 11.2.5-databeam-keepalive-route**, built from verified upstream source with receiver keepalive and route-display fixes. It uses a separate versioned cache and restores the bundled engine if that copy is damaged. Automatic engine upgrades are held until a verified official release replaces the patch, and the update button is hidden while pinned. Public relay names are resolved through the operating system; LAN discovery and direct transfers remain enabled. Custom `CROC_RELAY` settings remain supported. See the [keepalive investigation](investigations/croc-relay-keepalive.md) and [build instructions](CONTRIBUTING.md#patched-croc-engine).

System tray support is Windows-only. On macOS and Linux, closing the window exits the app, and automatic Croc update checks are disabled. Saved tray settings cannot enable tray behavior on those platforms.

