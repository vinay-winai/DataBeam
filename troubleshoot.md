# News

[Download DataBeam](https://vinay-winai.github.io/DataBeam/)

## v0.6.8 released

- Fix notification dots on Mac and highlight available updates in yellow.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.8)

## v0.6.7 released

- Add automatic update checks and one-click installation.

v0.6.6 users: install v0.6.7 or newer manually once to enable app updates.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.7)

## v0.6.6 released

- Fix Sendme VPN transfer failures and Croc relay connection issues; update security dependencies.

[Release notes](https://github.com/vinay-winai/DataBeam/releases/tag/v0.6.6)

# Troubleshooting DataBeam
- If transfers are failing check if vpn or dns is causing this.

## Croc / EazySendme Transfer Issues
- If transfer does not start and logs show any errors like (including not limited to) room not found or room unavailable/room full/peer left, sender should **retry with a different Code Phrase**. Avoid using simple codes like 1234567 or aaaaaaa.

## Common UI Issues

- File drag-and-drop currently does not work on Linux Wayland. Use the file / folder buttons instead. In the meantime, you could try launching DataBeam with `env -u WAYLAND_DISPLAY databeam`.
- If app width is small , UI persentation may not be desirable. In that case , please resize the app window.

## Suggestions
- Use Standalone Sendme mode for sensitive data transfers.
