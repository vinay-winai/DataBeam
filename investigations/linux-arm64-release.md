# Linux ARM64 release packaging (November 4, 2026)

Future releases publish version-free files only. The release workflow no longer
creates versioned compatibility copies for pre-v0.7.0 updaters. Existing published
releases are unchanged; users of those older apps can install a current download
manually if its release has no compatibility assets.

Linux release builds now use native x64 and ARM64 runners. `all`, tag pushes,
and `macos-linux` build both Linux architectures; manual dispatch also offers
`linux-x64` and `linux-arm64` separately. Each runner builds its matching patched
Croc engine and checks the Debian package architecture before upload.

New ARM64 packages:

- `databeam_linux-arm64.deb`
- `databeam_linux-arm64.AppImage`

The updater selects these packages for Rust's `aarch64` Linux architecture and
rejects releases that provide only x64 packages. The website provides explicit
x64 and ARM64 links and uses browser CPU information when available. A missing
ARM64 asset links to the release page rather than downloading an x64 package.

Validation: actionlint passed for both workflows; website download checks cover
ARM64 user agents, CPU client hints, x64, Windows, macOS, missing ARM64 packages,
and both asset orders (12 scenarios passed). Six updater tests passed, with the
public-release download test left ignored. Cargo-bundle 0.12.0's published AppImage packager supports
aarch64. Linux ARM64 binaries and GUI behavior still need the native release run
and on-device testing; this Windows workspace does not provide that environment.
