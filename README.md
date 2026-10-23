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

DataBeam bundles **Croc 11.2.5-databeam-keepalive**, built from verified upstream source with a receiver keepalive fix. It uses a separate versioned cache and restores the bundled engine if that copy is damaged. Automatic engine upgrades are held until a verified official release replaces the patch. Public relay names are resolved through the operating system; custom `CROC_RELAY` settings remain supported. See the [keepalive investigation](investigations/croc-relay-keepalive.md) and [build instructions](CONTRIBUTING.md#patched-croc-engine).

System tray support is Windows-only. On macOS and Linux, closing the window exits the app, and automatic Croc update checks are disabled. Saved tray settings cannot enable tray behavior on those platforms.

