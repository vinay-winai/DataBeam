# News

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
- Drag/ Drop is currently not working on linux. So use file / folder buttons instead.
- If app width is small , UI persentation may not be desirable. In that case , please resize the app window.

## Suggestions
- Use Standalone Sendme mode for sensitive data transfers.
