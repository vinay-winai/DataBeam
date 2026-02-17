# DataBeam ⚡

**A premium GUI wrapper for [croc](https://github.com/schollz/croc) and [sendme](https://github.com/n0-computer/sendme) — fast, secure, cross-platform file transfer.**

> Single executable. No runtime dependencies. GPU-accelerated UI.

## Features

- 🐊 **Croc integration** — E2E encrypted relay-based transfer with multi-file/folder support
- 📡 **Sendme integration** — Iroh-powered P2P with NAT traversal and blake3 verification
- 📦 **Multi-file transfers** — Send multiple files and folders in a single transfer (Croc)
- 🔐 **End-to-end encryption** — All transfers encrypted in transit
- 🌐 **Cross-platform** — Linux, macOS, and Windows from a single codebase
- ⚡ **Lightweight** — ~5MB single binary, GPU-accelerated immediate-mode GUI
- 🎨 **Premium dark UI** — Modern design with smooth animations and intuitive UX

## Prerequisites

You need at least one of these CLI tools installed:

- **croc**: `brew install croc` / `scoop install croc` / [releases](https://github.com/schollz/croc/releases)
- **sendme**: `cargo install sendme` / [releases](https://github.com/n0-computer/sendme/releases)

DataBeam auto-detects which tools are available and shows their status on the home screen.

## Building

### Quick build (current platform)

```bash
cargo build --release
```

The binary will be at `target/release/databeam`.

### Release build (optimized for size)

The `Cargo.toml` is already configured with aggressive optimizations:
- `opt-level = "z"` — optimize for binary size
- `lto = true` — link-time optimization
- `strip = true` — strip debug symbols
- `codegen-units = 1` — single codegen unit for better optimization

```bash
cargo build --release
# Binary: target/release/databeam (~5-8MB)
```

### Cross-compile targets

```bash
# Linux (from macOS, needs cross-compiler)
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu

# Windows (from macOS, needs cross-compiler)
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

> **Tip:** Use [cross](https://github.com/cross-rs/cross) for hassle-free cross-compilation:
> ```bash
> cargo install cross
> cross build --release --target x86_64-unknown-linux-gnu
> cross build --release --target x86_64-pc-windows-gnu
> ```

## Usage

1. Launch `databeam`
2. Select your preferred transfer engine (Croc or Sendme) on the home screen
3. **To send:** Navigate to Send → choose files/folders → click Send → share the generated code
4. **To receive:** Navigate to Receive → paste the code/ticket → choose save location → click Receive

## Architecture

```
src/
├── main.rs        # App structure, views, navigation
├── backend.rs     # CLI process management, output parsing, channels
├── widgets.rs     # Custom egui widgets (buttons, cards, progress, etc.)
└── theme.rs       # Color palette, styling, typography
```

- **Rust + egui/eframe** — Immediate-mode GPU-accelerated GUI
- **Process channels** — Background threads with `mpsc` for non-blocking CLI interaction
- **Zero web dependencies** — Pure native rendering, no Electron/browser overhead

## GitHub Releases

Prebuilt binaries are published in the GitHub Releases page on every version tag (`v*`):

- `databeam_vX.Y.Z_windows-x64.zip`
- `databeam_vX.Y.Z_linux-x64.tar.gz`
- `databeam_vX.Y.Z_macos-arm64.tar.gz`
- `SHA256SUMS.txt`

Release assets are produced by `.github/workflows/release.yml`.

### Create a release

```bash
# 1) bump version in Cargo.toml
# 2) commit and push to main
git tag vX.Y.Z
git push origin vX.Y.Z
```

This triggers the release workflow and uploads cross-platform artifacts automatically.

## License

MIT
