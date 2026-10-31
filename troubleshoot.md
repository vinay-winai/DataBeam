# News

## v0.6.8 released

- Draw notification badges as circles so they do not appear as missing-character squares on Mac.
- Make the update-available hint and Update button highlight yellow.
- Add a short suggested command under UI issues for Linux Wayland drag-and-drop.

[DataBeam website](https://vinay-winai.github.io/DataBeam/) · [Download v0.6.8 and read the release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.8)

## v0.6.7 released

- Add an Update button that checks for newer stable releases at startup and every three hours. A badge appears when an update is available; click it to download, verify, install, and restart DataBeam.
- Add an unread badge to the Troubleshoot button when this guide changes. Opening the guide clears the badge, and read status survives restarts.
- Fix the Mac updater architecture check that could report `lipo: unknown architecture`. The repaired update flow has now been tested on Windows and Mac.
- Correct the Troubleshoot link and keep the old URL working for existing apps.
- Serve the portable EXE directly from the website's Windows download links.

Existing v0.6.6 users need to install v0.6.7 manually once to get the updater. Users of the repaired v0.6.7 preview already have the same tested packages.

[DataBeam website](https://vinay-winai.github.io/DataBeam/) · [Download v0.6.7 and read the release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.7)

## v0.6.6 released

- Fix Sendme sender completion and delivery acknowledgment issues that could cause transfers to fail with VPN enabled or on slower relay connections. Transfers with VPN enabled have now succeeded from Windows to Mac and from Mac to Windows.
- Update Sendme security dependencies, including the iroh-blobs fix that prevents unsolicited uploads into the sender's store.
- Show live Direct/Relay connection status on Sendme and EasySendMe receivers.
- Fix receiving files from the Sendme CLI. Sends without an enclosing folder are placed inside a `_databeam` folder; existing sender folders are preserved.
- Improve Windows cache cleanup and retain more useful connection and export error details.
- Update the bundled Croc engine to 11.5.5, fix the macOS local listener startup error, and improve slower relay probe handling and Windows text reception.
- Improve the Croc received-text viewer for large messages with wrapping, scrolling, selection, copying, and an expanded view while preserving the full text.

[DataBeam website](https://vinay-winai.github.io/DataBeam/) · [Download v0.6.6 and read the release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.6)

# Troubleshooting DataBeam
- If transfers are failing check if vpn or dns is causing this.

## Croc / EazySendme Transfer Issues
- If transfer does not start and logs show any errors like (including not limited to) room not found or room unavailable/room full/peer left, sender should **retry with a different Code Phrase**. Avoid using simple codes like 1234567 or aaaaaaa.

## Common UI Issues

- File drag-and-drop currently does not work on Linux Wayland. Use the file / folder buttons instead. In the meantime, you could try launching DataBeam with `env -u WAYLAND_DISPLAY databeam`.
- If app width is small , UI persentation may not be desirable. In that case , please resize the app window.

## Suggestions
- Use Standalone Sendme mode for sensitive data transfers.
