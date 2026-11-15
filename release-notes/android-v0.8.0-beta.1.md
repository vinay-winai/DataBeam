# DataBeam Android v0.8.0-beta.1

First native Android beta for Android 8.0+ ARM64 phones and tablets.

APK refresh — build 802:

- Fix EasySendMe Resume repeating a consumed ticket handoff or using an unrelated main-input code. Matching partial data now continues automatically on Receive and Resume.
- Resume explains when a normal desktop sender has ended and provides New code / New ticket for the exact same files. A still-sharing session, such as Broadcast, can reuse its current ticket.
- Settings can hide either Sendme or Croc and its related main-screen options. At least one engine must remain enabled; EasySendMe requires both.
- Check local Croc data ports before switching away from a usable public connection. Local-port failures no longer produce a misleading public-relay outage warning. Croc remains pinned to 11.5.5.
- Refresh troubleshooting news on foreground entry and keep its unread badge visible above the button.
- Request file-manager window reuse when opening the save folder. File managers that force new document windows, including Google Files on the tested phone, retain control of that behavior.

Retained from build 801:

- Match desktop Sendme/EasySendMe folder naming; Croc retains original names.
- Automatically remove successful sender copies, clear failed sender copies on the next app opening, and remove receive blobs after verified export while retaining partial receives.
- Compact Clear all at the right, received-text Copy and Receive with Sendme actions, automatic transfer-card scrolling, and Open beside the download location in Settings.
- Display relay check URLs without JSON slash escaping.

- Croc, Sendme and EasySendMe file transfers, Croc text, QR sharing/scanning and Android share-sheet integration.
- Background transfer notification with progress, phases, rate and elapsed time beginning with payload bytes.
- Latest selected Direct/Relay path, public relay diagnostics and Croc public-relay recovery.
- Automatic saving with overwrite protection, saved-folder access and cleanup of verified working files.
- Completed sends clear the selected payload and retain a concise success card. Failed file sends ask you to reselect the exact originals; Croc retains its code for receiver resume. Failed text remains editable.
- Large text avoids Android's service Intent size limit and preserves Unicode and original line endings.
- Android 13+ asks for notification permission once; background battery/notification guidance is available in Settings.

Install `DataBeam_android-arm64_beta.apk`. This is a beta; the desktop stable release remains v0.8.0. A previous development APK uses a different signing certificate; Android requires removing that development installation before installing this public beta. Removing it clears its private settings/cache, so keep the development build if you need its saved resume data.

Android temporarily disables UDP GSO for Sendme/Iroh while tracking the upstream Android transport issue. Desktop transport settings are unchanged. Cloud providers and restricted document providers can require a temporary source copy; accessible seekable files are read directly.
