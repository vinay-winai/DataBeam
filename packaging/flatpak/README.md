# DataBeam Linux Flatpak

Standalone bundles have **no automatic app updates**. Install a newer downloaded bundle with the same `flatpak install --user` command to update while retaining settings.

Native bundles are built separately for x86-64 and ARM64. DroidDeck uses the
ARM64 bundle. These bundles are distributed through GitHub, not Flathub. Stable releases use the `stable` branch; prerelease test bundles use `testing`.

Install the matching downloaded bundle from the Linux desktop terminal:

```sh
flatpak install --user ./databeam_linux-arm64_no-auto-update.flatpak
flatpak run com.vinaywinai.databeam//stable
```

For x86-64 PCs, use `databeam_linux-x64_no-auto-update.flatpak` instead. Flatpak downloads the
Freedesktop runtime from Flathub when needed. Future test bundles are installed
the same way; this standalone bundle does not create a DataBeam update repository.
The app's executable replacement updater is disabled inside Flatpak.

## Permissions

| Permission | Purpose |
| --- | --- |
| Network | Direct and relay transfers, TCP/UDP listeners, DNS and HTTPS |
| X11 and shared IPC | XWayland windowing, clipboard and drag-and-drop; avoids the current native Wayland hide/redraw limitations |
| GPU rendering (`dri`) | OpenGL rendering using the host's Flatpak graphics integration |
| Home read/write | Sending selected files/folders and saving received data |
| `/run/media`, `/media`, `/mnt` read/write | SD cards, USB drives and mounted volumes |
| `/sdcard`, `/storage` read/write | Android shared storage when DroidDeck maps those paths into Linux |
| StatusNotifierWatcher D-Bus | Registering the tray icon; does not grant unrestricted session-bus access |

The existing file picker uses the desktop portal. Flatpak provides portal access
without an unrestricted D-Bus grant. No host process spawning, root access,
system-bus access or access to all devices is requested.

DroidDeck's official [Flatpak integration](https://github.com/Droid-Deck/DroidDeck/blob/main/docs/development/flatpak.md)
uses native ARM64 apps, a bubblewrap compatibility shim and X11 under gamescope.
Its launcher controls Android GPU and guest-storage mappings. These Flatpak
permissions cannot grant Android permissions that DroidDeck itself lacks, or
access to files not mapped into the guest. Test file selection, save locations,
network transfers and the tray in DroidDeck's KDE desktop.

WSL sandbox checks do not replace a KDE desktop or physical DroidDeck test.

## Build

Build the matching native binary and patched Croc payload first, then run:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub org.freedesktop.Platform//25.08 org.freedesktop.Sdk//25.08
python3 scripts/package_flatpak.py --arch aarch64
```
