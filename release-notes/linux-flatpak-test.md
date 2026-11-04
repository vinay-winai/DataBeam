Linux-only tray and updater test. The normal latest release remains v0.7.2.

- Restore the hidden window when DataBeam is launched again from its pinned dock icon.
- Fix the inherited Linux instance lock that prevented the app from restarting after an update.
- Keep the X11/XWayland tray fix and existing Flatpak permissions. **DroidDeck: choose ARM64.**

**Flatpak:** install the matching `.flatpak` with `flatpak install --user ./databeam_linux-x64_no-auto-update.flatpak` (or the ARM64 file). This replaces the previous test bundle and keeps settings. Close to tray, then click the pinned dock launcher; the existing window should reopen. On Ubuntu/GNOME, single-clicking the tray icon opens its menu and double-clicking activates DataBeam. Bundle updates are installed manually; this is not a Flathub listing.

**AppImage updater test:** `databeam_update-test-v0.7.1_linux-*.AppImage` contains the fixed code but deliberately reports v0.7.1. Mark it executable, launch it from a writable folder, and click Update. It should upgrade to the published v0.7.2 and reopen automatically. Install system `curl` first if missing. The final v0.7.2 app contains the existing stable code, not these newer fixes; keep the fixed test AppImage separately. This does not replace the historical v0.7.1 release.

[Installation and permissions](https://github.com/vinay-winai/DataBeam/blob/main/packaging/flatpak/README.md) · [DataBeam website](https://vinay-winai.github.io/DataBeam/)
