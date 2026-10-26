# Croc 11.5.5 engine upgrade

Compared and built on 2026-10-26 from official tag `v11.5.5`, commit
`8a806c36956533f9c003c55f16e31fcf14133f8e`. The codeload ZIP is pinned by SHA-256
`c8f6b6cea7d57aed8cdddda4d13d468b15c4d6b195c75ff389d1d64effabdc1b` in
[`croc-engine.json`](../patches/croc-engine.json).

## Existing patches

The newly downloaded official source already contains both runtime fixes from
DataBeam's older 11.2.5 patch:

- `receiveControlFrame` skips relay keepalives during the local PAKE and
  encrypted endpoint-list responses. It also applies a bounded read deadline.
- Route state is explicit (`setRelayControlRoute` and `peerToPeerDataPath`),
  and `transferDirection` prints the direct/relay format DataBeam parses.

Consequently the old runtime patch is not applied twice. Its opaque-response,
disconnect-after-keepalive, and direct-to-relay regression coverage is retained
in [`croc-v11.5.5-databeam.patch`](../patches/croc-v11.5.5-databeam.patch), using
the upstream implementations. The old patch is retained for historical builds.

Real Windows transfers exposed two additional compatibility issues, fixed in
the new patch:

- A receiver-created text filename uses a backslash prefix on Windows.
  `path.Base` did not remove that prefix, so the already validated text
  transfer prompted to overwrite its own temporary file. Use `filepath.Base`
  for this local filename. The exemption still requires `SendingText`, whose
  offer validation rejects disguised files, symlinks, and arbitrary paths.
  Normal file conflicts still require approval.
- The new control-frame reader allowed only 500 ms for a relay response. A
  captured public-relay trial exceeded that deadline and then handed a late,
  uncompressed PAKE reply to the compressed transfer decoder. Allow 30 seconds
  per response, without extending the deadline for keepalives, and fail the
  transfer on an incomplete probe instead of falling through into a different
  protocol phase. A delayed-response regression covers the old 500 ms limit.

The earlier investigation's claim that 11.5.5 still lacked the keepalive fix
does not match this freshly verified source and is superseded by this audit.

## Local TCP startup errors

The old `tcp.go:229` readiness check dialled a wildcard address such as `:9013`
after binding its listener, and cancelled the server when this dial failed.
11.5.5 reserves the socket group before startup and passes the actual listeners
to the workers. That particular self-dial has been removed. Existing upstream
tests cover reservation, cancellation, worker failure, and busy ports.

This addresses the code path behind the reported error; a VPN-specific diagnosis
or guarantee requires a separate run with the affected VPN configuration.

## DataBeam integration

- Engine identity/cache: `11.5.5-databeam`, separate from all older payloads.
- Go toolchain: 1.27.2 for local preparation and CI/release jobs.
- Keep OS relay-name resolution, source/payload hash checks, and cache repair.
- Keep automatic replacement pinned to the verified custom build.
- Retain upstream transport defaults; relay-only regression trials explicitly
  pass `--transport relay`, since `--no-local` alone does not disable Tailcat.
- Build tracking follows the patch path declared by the recipe.
- The local development DataBeam EXE remains multi-instance; the existing release
  feature still controls production single-instance behavior.

## Checks

`scripts/prepare_croc.py` runs DataBeam's inherited regressions and upstream
keepalive, direction-format, local-listener, and listener-lifecycle tests before
publishing a payload. Rust tests exercise bundle identity, extraction, cache
repair, version pinning, and progress/route parsing. Opt-in integration tests
cover real Croc text/files in both startup orders, direct and public-relay
routes, and receiver-first EasySendMe ticket exchange followed by native payload
download and byte comparison.

### Windows results

- Go compatibility, keepalive, route, local-listener, text-validation, and
  overwrite-protection tests: passed. Three upstream symlink-creation subcases
  are skipped on Windows because they require developer mode/admin privileges;
  declined-symlink protection remains tested. Other platforms run those cases.
- `cargo test --locked`: 83 passed; the 3 opt-in integration tests are ignored
  by the default suite and were run separately as described below.
- Actual public-relay text and two-file transfers: all four startup-order cases
  passed, with byte comparisons and Relay reported by both peers.
- Actual local direct text and two-file transfers: all four startup-order cases
  passed, with byte comparisons and Direct reported by both peers. Both peers
  ran on one Windows PC; this does not establish separate-device Wi-Fi behavior.
- Receiver-first EasySendMe: Croc delivered the exact native ticket, then Sendme
  transferred the random 256 KiB payload and the exported bytes matched.
- Official live Croc web: DataBeam sent and received the exact synthetic text;
  the browser confirmed verification/completion in both directions.

Local logs are under ignored `target/croc-1155-*.log`; browser proof is
`target/croc-1155-web-proof.png`. No VPN configuration was changed during testing.
