Linux-only Flatpak test with the tray bug fix. macOS behavior is unchanged; Windows and macOS packages remain on v0.7.2.

- Automatically use X11/XWayland when available, and hide the Linux window on close instead of minimizing it into the taskbar.
- Package native x64 and ARM64 builds with shared Flatpak runtimes. **DroidDeck: choose ARM64.**
- Include networking, home/removable-storage access, the file-picker portal and tray registration permissions. Updates are managed through Flatpak packages.

Install the downloaded bundle using `flatpak install --user ./databeam_linux-arm64.flatpak` (or the x64 bundle), then launch DataBeam from the desktop menu. This is a GitHub test bundle, not a Flathub listing.

Please test Show/Exit from the tray, file/folder selection, receiving to SD/USB storage, and transfers. DroidDeck must expose the intended Android storage paths to its Linux guest; package permissions cannot grant missing Android access.

[Installation and permissions](https://github.com/vinay-winai/DataBeam/blob/main/packaging/flatpak/README.md) · [DataBeam website](https://vinay-winai.github.io/DataBeam/)
