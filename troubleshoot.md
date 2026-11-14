# News

DataBeam is verified running on Android via DroidDeck with the AArch64/ARM64 build.

[Download DataBeam](https://wisesloth.pages.dev/DataBeam/)

## v0.7.6 released

- Improve Home actions, saved standalone expansion, and Broadcast/text-mode controls.
- Accept pasted Sendme receive commands and name conflicting Croc items.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.7.6)

# Troubleshooting DataBeam

## Updating older versions

**v0.6.8 and earlier:** [download the latest DataBeam from the website](https://wisesloth.pages.dev/DataBeam/) and install it manually once. The v0.6.7/v0.6.8 updater cannot find the new version-free download filenames. **v0.7.0 and newer** can update through the app's Update button.

**Linux DEB or Flatpak installations:** switch manually to the AppImage from the website; these packages are discontinued.

If an older Linux AppImage returns to the old version after updating, download and install v0.7.3 manually once. The updater restart fix then applies to future updates.

## Sendme / EasySendMe shows Relay on the same Wi-Fi

The connection can start as Direct and later fall back to Relay if its UDP path stops working. The relay is a valid transfer path, so the transfer can still finish, although speed may be lower. Croc public relay 3 availability is a separate issue.

[Iroh issue #4555](https://github.com/n0-computer/iroh/issues/4555) documents failed UDP segmentation offload batches causing established connections to time out. Its discussion includes Samsung Android 16 with Iroh 1.4.1. [Upstream fix #4645](https://github.com/n0-computer/iroh/pull/4645) was still open when checked on 2026-11-14. Sendme also has an older [offload failure report #45](https://github.com/n0-computer/sendme/issues/45). This is a strong match for the phone-to-PC behavior reproduced during DataBeam testing.

DataBeam retains upstream's normal transport settings and automatic Direct/Relay selection; it does not ship a local GSO workaround. Check the transfer's actual route indicator. Let a relayed transfer continue if its speed is acceptable, or use Croc for a different transfer engine.

## Transfer issues

- If transfers are failing check if vpn or dns is causing this.

## Croc / EazySendme Transfer Issues
- If transfer does not start and logs show any errors like (including not limited to) room not found or room unavailable/room full/peer left, sender should **retry with a different Code Phrase**. Avoid using simple codes like 1234567 or aaaaaaa.

## Common UI Issues

- Linux system tray support requires a desktop AppIndicator/StatusNotifier host. Enable the relevant desktop extension if the tray setting reports it is unavailable.
- On Linux Wayland, if file drag-and-drop does not work, launch DataBeam with `env -u WAYLAND_DISPLAY databeam` to use XWayland. This workaround has been confirmed working. You can also use the file / folder buttons.
- If app width is small , UI persentation may not be desirable. In that case , please resize the app window.

## Suggestions

- EazySendme/Sendme recommended for sensitive data transfers.

# Previous releases

## v0.7.3 released

- Fix macOS close-to-tray, Dock reopening and the tray logo.
- Fix Linux tray/Dock reopening and updater restarts; include AppImage XWayland libraries.
- Use AppImage for Linux downloads; DEB and Flatpak packages are discontinued.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.7.3)

## v0.7.2 - test released

- Restore macOS/Linux system tray controls and fix Windows EXE reopening from the tray.
- Improve tray startup and Dock/taskbar restoration.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.7.2)

## v0.7.1 released

- Add Linux ARM64 downloads and automatic update support.
- Use version-free download filenames and remove the Update button dot.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.7.1)

## v0.7.0 released

- Improve preparation progress and smooth speed / ETA.
- Fix Sendme Stop/resume and update iroh to 1.4.1.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.7.0)

## v0.6.8 released

- Fix notification dots on Mac and highlight available updates in yellow.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.8)

## v0.6.7 released

- Add automatic update checks and one-click installation.

v0.6.8 and earlier users: [download and install the latest version manually](https://wisesloth.pages.dev/DataBeam/) once to use current app updates.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.7)

## v0.6.6 released

- Fix Croc relay connection issues and update security dependencies.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.6)
