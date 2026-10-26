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
