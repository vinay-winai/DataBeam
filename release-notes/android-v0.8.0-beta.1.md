# DataBeam Android v0.8.0-beta.1

First native Android beta for Android 8.0+ ARM64 phones and tablets.

- Croc, Sendme and EasySendMe file transfers, Croc text, QR sharing/scanning and Android share-sheet integration.
- Background transfer notification with progress, phases, rate and elapsed time beginning with payload bytes.
- Latest selected Direct/Relay path, public relay diagnostics and Croc public-relay recovery.
- Automatic saving with overwrite protection, saved-folder access and cleanup of verified working files.
- Completed sends clear the selected payload and retain a concise success card. Failed file sends ask you to reselect the exact originals; Croc retains its code for receiver resume. Failed text remains editable.
- Large text avoids Android's service Intent size limit and preserves Unicode and original line endings.
- Android 13+ asks for notification permission once; background battery/notification guidance is available in Settings.

Install `DataBeam_android-arm64_beta.apk`. This is a beta; the desktop stable release remains v0.8.0. A previous development APK uses a different signing certificate; Android requires removing that development installation before installing this public beta. Removing it clears its private settings/cache, so keep the development build if you need its saved resume data.

Android temporarily disables UDP GSO for Sendme/Iroh while tracking the upstream Android transport issue. Desktop transport settings are unchanged. Cloud providers and restricted document providers can require a temporary source copy; accessible seekable files are read directly.
