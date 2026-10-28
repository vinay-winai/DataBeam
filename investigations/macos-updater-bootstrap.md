# DataBeam macOS updater test

These are temporary **v0.6.6 test apps containing the updater**. They check only the published v0.6.7 preview so that the complete update flow can be tested. They are Actions artifacts and do not replace the stable release or public preview downloads.

1. Extract this artifact ZIP. Choose the `macos-aarch64.dmg` for Apple Silicon, or `macos-universal.dmg` for Intel (universal also supports Apple Silicon).
2. Close other DataBeam apps. Copy the test `DataBeam.app` into a separate writable folder such as `~/Applications/DataBeamUpdaterTest/`. Keep your normal app in its existing location. Do not run the test directly from the mounted DMG.
3. Open the copied app. It should report v0.6.6 and show an Update badge for v0.6.7 once its background check completes.
4. Click Update. The app downloads the real v0.6.7 preview, verifies its checksum and bundle, closes, replaces this test app, and restarts as v0.6.7.
5. Confirm the new version opens, your settings survive, the badge clears, and no `.databeam-update-*` staging folders remain beside the app. Note any error message if it fails.

The test app uses normal DataBeam preferences, so it can verify that existing settings survive. It replaces only the copy you launched from the test folder. Once updated, the app is the genuine v0.6.7 preview and follows the normal stable-update channel.

`SHA256SUMS.txt` checks the two test DMG downloads. These test packages are ad-hoc signed in the same way as the project's current Mac release packages.
