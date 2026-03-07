# DataBeam

DataBeam is a compact desktop GUI for `croc` and `sendme`.

It gives you:
- the convenience of Croc
- the modern transfer model of Sendme
- an EasySendMe mode that combines both ideas

## What It Does

DataBeam is built for people who want fast file transfer without living in the terminal.

You can:
- use Croc directly
- use Sendme directly
- use EasySendMe, where Sendme handles the transfer but user shares a short croc code (can even create 7 length custom code) instead of the super long sendme ticket

## Features

- EasySendMe mode: Croc-style convenience with the modern improvements of Sendme
- small executables, typically in the single-digit MB range
- standalone `croc` and `sendme` are still available if you want to use them for specific situations
- sending text in Croc mode
- support for custom code as short as 7 characters in croc and easysendme modes
- support for transferring multiple files/folders at once
- QR code generation for transfer codes/tickets
- modern native desktop UI with no browser runtime
- visible transfer logs inside the app

## Modes

### Croc

Use Croc when you want:
- simple code-based sharing
- text sending
- a familiar relay-based workflow

### Sendme

Use Sendme when you want:
- modern peer-to-peer transfer behavior
- native in-app Sendme support
- reusable server-style sending when one-shot mode is disabled

### EasySendMe

EasySendMe is the convenience mode.

It is meant to feel closer to Croc from a user-flow perspective while still using Sendme for the actual file transfer.

In short:
- Croc helps share the code more conveniently
- Sendme does the transfer work

## Build

```bash
cargo build --release
```

Release binary:
- `target/release/databeam`
- on Windows: `target/release/databeam.exe`

## Why DataBeam

DataBeam is for users who want:
- one app for both Croc and Sendme
- a desktop-first workflow
- lightweight binaries
- logs and transfer state visible in the UI
- QR-based handoff available when sharing codes or tickets

## Issues And Future Improvements(not ordered by priority)

- a website to download this app
- proper support for multi broadcasting in Sendme server mode
- refining UX/UI after gathering user feedback
- adding helpful pointers
- locking app launch to a single instance
- fixing drag/drop in Linux
- fixing progress bar state on repeated transfers in Sendme server mode
- more target os/arch binary releases

## Notes

DataBeam is a GUI wrapper and workflow layer around Croc and Sendme for people not comfortable with cli apps.
