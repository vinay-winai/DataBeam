# Contributing

## Branching

- `main`: stable branch for releases.
- `develop`: integration branch for ongoing work.
- Feature/fix branches: create from `develop` using:
  - `feat/<short-name>`
  - `fix/<short-name>`
  - `chore/<short-name>`

## Pull Requests

- Open PRs into `develop`.
- Keep PRs focused and small.
- Ensure CI is green before requesting review.
- Include a short test note in the PR description.

## Versioning and Releases

1. Merge release-ready changes into `main`.
2. Bump version in `Cargo.toml`.
3. Create and push a tag:
   - `git tag v0.1.6`
   - `git push origin v0.1.6`
4. GitHub Actions `Release` workflow builds platform artifacts and publishes them to GitHub Releases.

## Local Checks

### Patched Croc engine

Install Python 3.9+, Git, and Go 1.27.0. Prepare the engine before building:

```bash
# Windows x64; use the Rust target for your platform.
python scripts/prepare_croc.py --target x86_64-pc-windows-msvc
cargo build --release --features release-single-instance
```

Other release targets are `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
and `x86_64-apple-darwin`. For macOS universal builds, pass `--target` twice
to prepare both payloads before building both Rust targets. The script verifies
the pinned source archive SHA-256, applies the tracked patch, runs its Go tests,
and builds the embedded engine. It uses Go's normal platform resolver; it does
not force the `netgo` build tag used by the official CLI release.

Payloads are generated under `target/croc-bundle/<rust-target>`. An alternative
directory can be passed to the script with `--output` and to Cargo through
`DATABEAM_CROC_BUNDLE_DIR`. For offline builds, use `--source-archive` with the
verified source ZIP and prepopulate the Go toolchain/module cache.

Release builds fail if the payload is missing or stale. Development checks
without a payload still compile, but Croc transfers then need a system engine
whose version exactly matches the patched version. CI and release workflows
prepare the payload on Windows, Linux, and macOS.

To retire the patch after an official fix, update the recipe and builder,
retest receiver-first text/files and croc web in both directions, and migrate
to a new engine identity. Do not reuse the patched cache identity for official
binaries or enable unverified self-updates.

### Rust checks

Run before opening PRs:

```bash
cargo check
cargo test
```

### Release packaging

The release workflow pins `cargo-bundle` to 0.12.0:

```bash
cargo install cargo-bundle --version 0.12.0 --locked
```

Keep bundle metadata compatible with that version. Linux terminal settings
belong under `[package.metadata.bundle.linux]` as `use_terminal`, as described
in the [0.12.0 manifest documentation](https://github.com/burtonageo/cargo-bundle/blob/v0.12.0/Readme.md#linux-specific-settings).
Packaging uses `--binary-path` to reuse the release binary already built by CI.
When rebuilding an already-published release, dispatch all targets with
`replace_existing=true`. After successful uploads, the workflow moves that
existing tag to the build commit. Notes in `release-notes/<tag>.md`, when
present, replace generated notes on the published release.
The macOS and Linux release jobs run `non_windows_` regression tests to verify
automatic Croc checks and tray initialization stay disabled, including when
saved settings request a tray icon.

The ignored `croc_real_text_and_file_round_trip` test normally forces public
relay transfers. Set `DATABEAM_CROC_TEST_BINARY` to the prepared engine and
`DATABEAM_CROC_TEST_REQUIRE_DIRECT=1` to exercise the production sender with
LAN discovery enabled instead. This mode requires both peers to report a
direct route and verifies text/file bytes in both startup orders. Running
both peers on one PC checks local transfer behavior; it does not replace a
test between two separate devices on Wi-Fi.
