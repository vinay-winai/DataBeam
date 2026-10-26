# Sendme 0.37.1 synchronization

Compared on 2026-10-26 against official
[`n0-computer/sendme` at `21881bd`](https://github.com/n0-computer/sendme/commit/21881bd),
updated from the previous reference at `8dda1e5`.

DataBeam embeds a customized alt-sendme library. This update ports applicable
upstream changes into that library; it does not replace the engine with the CLI.
The embedded engine version is now 0.37.1; this sync ships in DataBeam 0.6.6.

## Applied

- Require `iroh-blobs >= 0.103.1` and lock both builds to 0.103.1.
  [Upstream #144](https://github.com/n0-computer/sendme/pull/144) fixes a request-mode
  authorization bug: enabling download progress notifications also allowed peers
  to push arbitrary blobs into the sender's store. Push remains disabled by default.
- Update both lockfiles to `rustls 0.23.46`, `rustls-webpki 0.103.16`, and
  `webpki-roots` / `webpki-root-certs 1.0.11` (the latter are newer than upstream's
  1.0.10). These include the fixes carried by
  [#149](https://github.com/n0-computer/sendme/pull/149) and
  [#151](https://github.com/n0-computer/sendme/pull/151).
- Update Tokio to 1.53.3, matching the official lockfile, and update yanked
  `chacha20`, `der`, and `spin` versions following
  [upstream's lockfile cleanup](https://github.com/n0-computer/sendme/commit/0661eb2).
- Port [#148](https://github.com/n0-computer/sendme/pull/148): observe the selected
  path with `Connection::paths_stream()`. Sendme and EasySendMe receivers show
  **Transfer mode: Direct** or **Transfer mode: Relay**, including route changes,
  in the running transfer details, completion summary, and log. No route is inferred
  from ticket addresses or the Croc code-exchange leg. Clear it between transfers
  and when the connection has no selected path. The watcher is cancelled when
  the network download ends or is cancelled. Sender-side route display is not
  added; upstream's change also applies to receiving only.
- Give CI read-only repository permissions and disable checkout credential
  persistence, following the applicable parts of upstream's CI hardening.
- Add native loopback transfer/upload-rejection coverage to CI, alongside a GUI
  regression test for relay/direct changes and clearing route state.

## Kept specific to DataBeam

Multi-file selection, custom short codes, broadcast sharing, retry/resume,
configurable caches, and destination conflict handling remain in the customized
engine. Existing iroh 1.0.2 is retained: the route API and patched blob library
work with it. This is a targeted synchronization, not a full dependency refresh
to upstream's iroh 1.3.0 graph.

The CLI help improvement and terminal key-event handling do not apply to the
native GUI. Upstream's release workflow and contribution policy are specific to
its repository and are not copied into DataBeam.

## Verification

```powershell
cargo check --locked
cargo test --locked
cargo test --manifest-path third_party/sendme/Cargo.toml --lib --no-default-features --locked --target-dir target
```

The loopback test verifies received file bytes, a real direct-path event, and
rejection of a push into the actual sharing engine. The GUI test verifies both
route labels, reconnect/reset behavior, and isolation from Croc's route logs.
This does not replace a separate-device or public-relay connectivity test.
