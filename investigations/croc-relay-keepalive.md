# Croc relay keepalive investigation

Investigated on 2026-10-23, Windows x64. **Root cause reproduced outside
DataBeam; a minimal Croc receiver patch passed the targeted regression tests.**
DataBeam now bundles the patched engine as `11.2.5-databeam-keepalive`.
The build recipe verifies the upstream source archive, applies the tracked
patch, tests it, and generates a compressed payload for each release target.
Runtime extraction verifies SHA-256 and uses a separate version/architecture
cache. Restore and update paths preserve the patch; they do not download the
unpatched official binary. See [build instructions](../CONTRIBUTING.md#patched-croc-engine).

## Root cause

Croc's receiver asks the sender for local-network endpoints over the relay
before entering the compressed transfer protocol. This exchange uses plain
JSON for its PAKE handshake, followed by an encrypted endpoint list.

The relay sends framed keepalives with the single-byte payload `01` while a
peer is waiting. Both v11.2.5 and v11.5.5 read the next frame as the PAKE reply
without skipping these keepalives. If the receiver joins first, or a keepalive
is already queued, it reads `01` instead of the expected `pake2` JSON.

The failed probe is treated as optional, so the receiver sends `handshake`
and starts the compressed transfer protocol anyway. The sender then answers
the earlier probe with an uncompressed `pake2` JSON message. The receiver
passes that JSON to Deflate and reports:

```text
decompress message: decompress data: flate: corrupt input before offset 6
```

This is a protocol sequencing bug. The captured sender and receiver frames
have matching SHA-256 hashes: the relay forwarded the JSON intact. No file
data had been transferred, and changing file compression cannot fix it.
The DNS timeout reported separately has a different cause.

Upstream source:

- [v11.2.5 receiver local probe](https://github.com/schollz/croc/blob/v11.2.5/src/croc/croc.go#L1881)
- [v11.5.5 receiver local probe](https://github.com/schollz/croc/blob/v11.5.5/src/croc/croc.go#L2036)
- [v11.2.5 relay keepalive sender](https://github.com/schollz/croc/blob/v11.2.5/src/tcp/tcp.go#L295)

## Captured failing sequence

Transparent loopback TCP proxies forwarded each CLI connection to the same
public relay IP and recorded complete `croc` frames. Only synthetic text and
files were used. The critical sequence with official v11.2.5 was:

| Direction | Frame | Receiver action |
| --- | --- | --- |
| Receiver → sender | Plain JSON `Kind: pake1` | Wait for probe reply |
| Relay → receiver | One byte `01` | Reject as invalid PAKE response |
| Receiver → sender | Plain `handshake`, then compressed transfer PAKE | Enter transfer protocol |
| Sender → receiver | 1,150-byte plain JSON `Kind: pake2`, prefix `7b22427974657322` (`{"Bytes"`) | Attempt Deflate decoding; fail at offset 6 |

The sender subsequently received EOF and reported `could not secure channel`.

## Results

The public-relay runs below forced the sender to use the relay with
`send --no-local`; they could not silently succeed using LAN discovery.
Receiver-first runs left the receiver waiting before starting the sender.

| Receiver / sender | Relay | Payload | Result |
| --- | --- | --- | --- |
| Official 11.2.5 / 11.2.5, sender first | 3.getcroc.com | Text, 2 attempts | Both passed |
| Official 11.2.5 / 11.2.5, receiver first | 4.getcroc.com | Text, 2 attempts | Both failed with the captured keepalive → JSON → Deflate sequence |
| Official 11.5.5 / 11.5.5, receiver first | 4.getcroc.com | Text | Same failure |
| Official 11.0.1 / 11.0.1, receiver first | 4.getcroc.com | Text | Same failure |
| Patched 11.2.5 / official 11.2.5, receiver first | 4.getcroc.com | Text, 2 attempts | Both passed |
| Patched 11.2.5 / official 11.5.5, receiver first | 2.getcroc.com | Random and compressible files, 512 KiB total | Passed; destination bytes matched |
| Patched 11.2.5 / official 11.5.5, receiver first, extra keepalive injected during PAKE | 3.getcroc.com | Same file workload | Passed; destination bytes matched |

The Go unit tests also passed for repeated keepalives before both reply
formats, and for a disconnect after a keepalive (EOF or a wrapped closed-pipe error).

The earlier local-relay pass and the v11.2.5 pin did not establish that this
bug was fixed: a quick sender-first transfer can avoid the triggering timing.
The exact commit that first introduced the bug was not bisected.

## DataBeam integration validation

- 79 Rust tests passed, including random-code UI completion, sender code
  publication, embedded engine version, bad-cache repair, and preserving a
  working binary if an invalid replacement is presented.
- The embedded Windows payload reports `11.2.5-databeam-keepalive`; the
  official `11.2.5` version string is rejected as a replacement for this engine.
- DataBeam's transfer wrappers passed text and mixed random/compressible
  files through public relays in both sender-first and receiver-first order.
  Receiver-first trials wait three seconds before starting the sender.
- The same wrappers passed live croc web text and file transfers in both
  directions. The received file was checked byte-for-byte in the wrapper;
  the browser download's SHA-256 also matched the source:
  `d08505dc1e58d5c7d775948be7a8a579f0345edba8e6cb2b109c5909c24ba435`.
  The isolated browser used croc web's supported in-memory download fallback
  by disabling `showDirectoryPicker` for that test page. An earlier trial
  waited at the unattended folder picker and timed out before payload
  transfer; it was not counted as a protocol failure or successful delivery.
- The engine cross-compiled for Windows x64, Linux x64, and both macOS
  architectures. Native Linux/macOS application execution is left to CI
  and platform testing; it was not claimed from a Windows cross-build.
- The Windows application builds in release mode with `release-single-instance`.

See the separate [DNS investigation](croc-dns.md) for the official latest
CLI/web comparison and the reason DataBeam retains OS-resolved relay routing.

## Tested patch

[`patches/croc-v11.2.5-relay-keepalive.patch`](../patches/croc-v11.2.5-relay-keepalive.patch)
adds a receiver helper that skips only complete single-byte `01` frames while
waiting for the PAKE response and the encrypted endpoint-list response. It
preserves all other payload bytes and propagates disconnect/read errors.
Encryption, authentication checks, compression, and LAN discovery remain intact.
The sender needs no change for this particular failure.

Apply to a clean official v11.2.5 source checkout:

```powershell
git apply croc-v11.2.5-relay-keepalive.patch
go test ./src/croc -run TestLocalProbeResponse -count=1 -timeout 60s
go build -buildvcs=false -ldflags '-X github.com/schollz/croc/v11/src/cli.Version=11.2.5-databeam-keepalive' -o croc-keepalive.exe .
```

The build requires Go 1.27.0 as specified by upstream's `go.mod`.
The diagnostic Windows binary reports `11.2.5-databeam-keepalive` so it can
be distinguished from the official release. It is a locally patched test
engine, not an upstream release or a replacement DataBeam application.

## Reproduction without DataBeam

Use two official Croc CLIs with a fresh matching `CROC_SECRET`, the same
public `--relay` endpoint, and `--yes --ignore-stdin`. Start the receiver
first and leave it waiting several seconds; then start the sender with
`send --no-local --text "synthetic handshake regression"`.

An example endpoint used in the captured failure was `4.getcroc.com:9009`.
Resolve the name through the host OS and pass the resulting IP if Go DNS
timeouts would otherwise mask the handshake bug. Public relay availability
and timing can vary; the receiver log should identify the invalid one-byte
local PAKE response before the Deflate error. Use `--debug` on both CLIs to
see that stage. A fresh secret is necessary for each independent trial.

The ignored `temp_croc_debug/trace-roundtrip.py` harness and captured frame
files are retained locally for this investigation, not included in releases.
