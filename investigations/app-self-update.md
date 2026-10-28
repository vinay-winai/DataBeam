# DataBeam application updates

The Update button checks the repository's latest stable GitHub release at launch and every three hours measured from the last check. It also permits a manual check when no update is available. Semantic version comparison prevents downgrades; drafts and previews are excluded. A newer version adds an orange badge to the button. Clicking it downloads, verifies, installs, and restarts DataBeam.

Checks and downloads run on background workers. Active transfers, pending transfer retries, and Croc engine updates disable installation. Starting a new transfer is blocked while an application update is being prepared. Failures leave the button available for retry and show the error in its status text.

## Download and installation

- Metadata comes from `https://api.github.com/repos/vinay-winai/DataBeam/releases/latest`. Asset URLs must match this repository, the selected tag, and the exact platform filename. HTTPS certificate verification remains enabled for requests and redirects.
- The package's exact size and streamed SHA-256 must match `SHA256SUMS.txt`. When GitHub exposes an asset digest, it must agree with the manifest. These checks detect download corruption; the trust source is this repository over HTTPS, without an independent publisher-signature scheme.
- A copied helper executable validates its installation plan and signals readiness before the running app exits. It bypasses UI and single-instance initialization, waits for the old process to exit, then installs. The replacement app acknowledges successful initialization. The restarted app removes staging and backup files after the helper exits.
- Portable Windows EXEs update beside the existing executable. Installation requires a writable parent directory. The helper waits for Windows to release the running executable before replacement. A failed file swap or failed startup restores the previous app.
- macOS updates use the Apple Silicon or universal DMG as appropriate. The updater mounts it read-only, checks bundle identifier, version, architecture, and code-signature validity, copies the complete app bundle, and detaches the image. The installed app must be in a writable application folder; running directly from a mounted DMG cannot update in place. Bundle replacement and restart failures restore the previous bundle.
- Linux AppImages replace the original path from `APPIMAGE`, preserving executable permissions. They use the same backup and restart recovery as portable Windows installs.
- Linux DEB installations at `/usr/bin/databeam` use `pkexec /usr/bin/dpkg --install` and the desktop's administrator authorization prompt. They require PolicyKit and dpkg. Package-manager failures are reported on restart; this path does not implement package downgrades as rollback.

## Validation

- 104 application tests pass, with four network/manual tests ignored by default. Coverage includes version ordering, exact platform asset selection, missing/untrusted assets, manifest/API checksum disagreement, offline checks, and transfer guards.
- A real native executable fixture holds the old Windows executable open, then exits. The updater replaces it, starts the new executable, and checks exact file hashes. A separate fixture deliberately fails startup; the previous executable is restored and relaunched. The same test verifies staging cleanup waits for helper exit and preserves the installed executable.
- The ignored `app_update::tests::public_release_download_passes_checksum_verification` test passed against the live GitHub release, downloading and verifying the Windows EXE without installing it.
- The updater modules were type-checked for `aarch64-apple-darwin` using a minimal UI-context harness. Actual macOS and Linux installation, authorization dialogs, and restart behavior still need device testing.

Existing v0.6.6 release packages have not been rebuilt for this feature. Users must first install an updater-capable build; that build can then update to later stable versions. Replacing a release's assets without changing its version does not trigger an update badge.

## Windows Update-button test — 2026-10-28

A temporary v0.6.6 bootstrap build was made from the v0.6.7 source in an isolated directory. Only its package version and release lookup policy differed: it queried the exact v0.6.7 preview and allowed that pinned preview through the release filter. Its installer, UI, backend, and guide-monitoring source matched production after line-ending normalization.

The app displayed `v0.6.7 is available` with the Update badge. The user clicked Update. The app downloaded the published Windows EXE, replaced itself, and restarted as v0.6.7. The badge cleared and its normal stable-release check reported it up to date rather than offering the older v0.6.6 stable release.

The installed EXE matched the published asset SHA-256 `4e6becab879307b8ad607922b1821906e7d6aec75bf41686a621ec4fd55972a8`. The seeded receive folder, recent code, one-shot setting, and tray preference survived. Default user settings were unchanged, staging/backup directories were removed, the helper exited, and the restarted app remained running. Local evidence is in `target/updater-e2e-windows/verification.json`.

The complete Windows button/download/install/restart flow has now passed on this PC. macOS and Linux installation still require real-device testing.

## Mac architecture-check failure

The first Mac device test failed during preparation with `lipo: unknown architecture`. The updater invoked `lipo -verify_arch arm64 /path/to/databeam` (or x86_64), but Apple's argument parser consumes the path as another architecture. The corrected call is `lipo /path/to/databeam -verify_arch arm64`. Validation still rejects missing architectures; it is not bypassed. Because the failure happened before helper handoff, the running app was not replaced.

A macOS-only regression test copies the native test executable to a path containing spaces and an apostrophe, verifies its actual architecture using the system `lipo`, and checks that i386 is rejected. Both the normal Mac release workflow and the bootstrap artifact workflow run this test before packaging.
