# News

## DataBeam for Android

The native Android app supports **Android 8.0+ on ARM64** phones and tablets.

[Android APK downloads](https://github.com/vinay-winai/DataBeam/releases)

Android shares the desktop transfer engines and adds Share Sheet support, camera QR scanning, and background transfer notifications.

- Send and receive with Croc, Sendme and EasySendMe; generate and scan transfer QR codes.
- Share files or text to DataBeam from other Android apps, with background transfer notifications.
- Follow the latest Direct/Relay path, payload progress and elapsed time; open the saved folder after receiving.
- Public relay checks, Croc relay recovery and a Resend action to restart an interrupted send when its original files are still accessible.

The Linux AArch64 AppImage also runs through DroidDeck on Android; it is separate from the native APK.

[Download DataBeam](https://wisesloth.pages.dev/DataBeam/)

DataBeam 0.8.8
- Stable Android release for Android 8.0 and newer on ARM64. The APK uses the same signing certificate as the public beta.
- Croc is the default engine. Sendme offers EasySendMe short codes and Broadcast. DataBeam recognizes the esm suffix when receiving an EasySendMe code.
- Receive history resumes interrupted Croc, Sendme, and EasySendMe batches and individual transfers. If the sender changes the batch, a preview lists replacements, additions and removals. Unchanged files keep their download progress.
- On desktop and Android, Resend starts a new session from an interrupted transfer when its full payload is accessible. It prompts for missing files before starting.
- Android can send supported picker and Share Sheet sources directly from their file descriptors. Settings remember expanded sections and can automatically unwrap completed Croc, Sendme, and EasySendMe receives.
- Desktop includes a custom Croc relay setting. Both platforms can skip identical files selected individually and optionally expire old receive history and partial data.
- Startup prompts on desktop offer Continue transfer for remembered Sendme and EasySendMe partial downloads. EasySendMe and one-shot Sendme senders keep sharing active after a receiver disconnects so downloads can resume seamlessly.

# Troubleshooting DataBeam

## Updating older versions

**v0.6.8 and earlier:** [download the latest DataBeam from the website](https://wisesloth.pages.dev/DataBeam/) and install it manually once. The v0.6.7/v0.6.8 updater cannot find the new version-free download filenames. **v0.7.0 and newer** can update through the app's Update button.

**Linux DEB or Flatpak installations:** switch manually to the AppImage from the website; these packages are discontinued.

If an older Linux AppImage returns to the old version after updating, download and install v0.7.3 manually once. The updater restart fix then applies to future updates.

## Sendme / EasySendMe shows Relay on the same Wi-Fi

The connection can start as Direct and later fall back to Relay if its UDP path stops working. The relay is a valid transfer path, so the transfer can still finish, although speed may be lower. Croc public relay 3 availability is a separate issue.

[Iroh issue #4555](https://github.com/n0-computer/iroh/issues/4555) documents failed UDP segmentation offload batches causing established connections to time out. Its discussion includes Samsung Android 16 with Iroh 1.4.1. [Upstream fix #4645](https://github.com/n0-computer/iroh/pull/4645) was still open when checked on 2026-11-14. Sendme also has an older [offload failure report #45](https://github.com/n0-computer/sendme/issues/45). This is a strong match for the phone-to-PC behavior reproduced during DataBeam testing.

Desktop DataBeam retains upstream's normal transport settings and automatic Direct/Relay selection. The Android app temporarily disables UDP GSO for Sendme and EasySendMe while tracking the upstream fix. Check the transfer's actual route indicator. Let a relayed transfer continue if its speed is acceptable, or use Croc for a different transfer engine.

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
