# Sendme premature sender completion

## Reproduction

On 2026-10-26, a Mac sender and Windows receiver with VPNs enabled selected
Sendme's relay path. Croc delivered the ticket and the receiver discovered a
43.1 KB payload, but the sender reported completion before the payload arrived.
The receiver subsequently reported `error: read: connection lost`. A copy of
the receiver's store contained 233 local bytes and an incomplete collection.
The same setup worked over a direct path with the sender's VPN disabled.

A local protocol probe reproduced completion after only
`get_hash_seq_and_sizes`, with sender progress still at `0:44100`. Delaying the
next request exposed a timing race independent of VPN software.

## Cause and correction

The sender inferred completion from the number of finished requests and a
500 ms quiet interval. It counted the metadata/size-discovery request as a
transfer. One-shot mode then stopped the sharing runtime before the receiver
could request the actual payload.

The sender now requires an explicitly successful request that included file
data before it can emit `transfer-completed`. Size probes are recognized from
their requested ranges, including repeated probes with nonzero request IDs.
Hash-sequence and collection-metadata items (indices 0 and 1) do not count as
file payload. Payload requests work even when they are request zero. A dropped
progress channel no longer credits remaining bytes or infers success.

The existing short grace period after a successful payload request remains;
it can no longer turn a quiet interval following metadata into completion.
No transfer-engine dependency or wire-protocol changes are involved.

## Validation

The native regression failed before the correction with
`sender completed after metadata, before any payload request`. It now checks
repeated probes separated by 1.1-second gaps, followed by actual downloads:

- A 44,100-byte file.
- A folder with two files.
- An empty file.
- A payload request without a preceding size probe.
- A resumed download with one file already cached.

Each case verifies the complete local collection, exact file bytes, and a
completion event after payload transfer. All 17 native Windows tests pass.
The exact Mac-to-Windows VPN setup still requires a test with the fixed sender.
The separate Croc relay-probe timeout observed during a later retry is outside
this sender-completion correction.

## Follow-up: queued bytes versus acknowledged delivery

The rebuilt preview still failed with Proton VPN enabled. The user confirmed
both devices had been updated and later reproduced the failure in both
directions. The earlier apparent Mac-only behavior is therefore not a reliable
distinction.

Provider progress measures writes into QUIC's send buffer. The upstream QUIC
adapter's `sync()` is a no-op, so its request-completion notification can precede
network delivery. Stopping the one-shot runtime after a 500 ms grace interval
can still abandon those buffered response bytes.

The local delivery-ack test build registers a protocol adapter that finishes the
response stream and awaits `SendStream::stopped()` with successful acknowledgment
before the provider reports request completion. Explicit stream stops and
connection failures remain errors. The existing read-only event mask, including
rejection of uploads to the sender, is retained. Per-connection stream tasks are
owned by a JoinSet and cancelled with the connection handler.

A loopback UDP proxy test holds response packets for 1.2 seconds. Its control
uses the original adapter and completes while packets are still held; the new
adapter remains pending until delivery and acknowledgment. The test uses the
same underlying QUIC streams without iroh multipath, which could otherwise
bypass the intentionally delayed proxy. All 18 native tests pass, including
the metadata-completion, payload-resume, exact-content and upload-rejection
regressions.

The local build also preserves the receiver's error chain and stops translating
explicit sender failures or an unexpectedly closed event channel into a
successful disconnect based only on queued-byte counters. This verifies the
shutdown correction; the exact Proton VPN setup still needs a real-device test
to establish whether another network-path problem remains.
