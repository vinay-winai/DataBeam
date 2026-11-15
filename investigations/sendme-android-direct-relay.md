# Android Sendme direct-path timeout

Reproduced with ordinary Iroh routing between this Android 16 Samsung phone and a Windows peer on the same LAN. The connection initially selected the phone’s private IPv4 UDP path, then Iroh abandoned it as TimedOut and selected a relay. Payloads could continue on Relay, sometimes slowly. This is separate from Croc public-relay code selection.

Upstream sources checked on 2026-11-14:
- https://github.com/n0-computer/iroh/issues/4555: cached UDP GSO limits cause failing batches to be retransmitted until a connection times out. Comments include Samsung Android 16 with Iroh 1.4.1.
- https://github.com/n0-computer/iroh/pull/4645: proposed dynamic-limit fix; still open.
- https://github.com/n0-computer/sendme/issues/45: earlier Sendme offload failures.
- https://github.com/n0-computer/sendme/issues/162: separate Android JNI-context startup issue, relevant to correct native integration rather than this payload timeout.

Rechecked 2026-11-15: issue #4555 is still open; PR #4645 is open with `merged=false` and no merge date. The latest published Iroh release is still [v1.4.1](https://github.com/n0-computer/iroh/releases/tag/v1.4.1), also used by this build. The proposed fix has not landed and inclusion in the next release is not confirmed.

Controlled diagnostics: after adding Android’s required DNS context/network callbacks, five of six 64 MiB upload attempts still fell back from Direct to Relay; one completed Direct. A temporary experiment disabling segmentation offload passed three 64 MiB uploads, six further public-handoff 64 MiB uploads, and two public-handoff 512 MiB uploads, all with Direct route evidence and SHA-256 verification. This strongly supports the offload diagnosis, but is not a kernel-level proof for every device.

**Earlier shipping decision (superseded by the Android-only decision below):** the user requested documentation instead of a workaround while upstream is tracking it and relay transfers remain functional. The experimental transport override was removed. The final Android build uses Iroh’s normal transport defaults and route selection. The proper application-context initialization and Android network callbacks remain. No local upstream transport patch, throwaway socket probe or general app retry loop is shipped.

Ignored diagnostic logs and generated-only fixtures are under android/.tools/smoke/. Final installed-build checks are in android/VALIDATION.md. Extended overnight, cellular switching and additional-device tests were explicitly skipped by the user.


## Current Android workaround — 2026-11-15

The user subsequently authorized disabling UDP GSO for Android while upstream fixes are pending. Both Android sender and receiver endpoints now build their QUIC transport with `enable_segmentation_offload(false)`, guarded by `cfg(target_os = "android")`. Desktop retains Iroh transport defaults. Croc is unaffected. No system-wide NIC setting, relay-only routing or general transfer retry was added.

This is the earlier small configuration workaround, not the proposed upstream dynamic-limit implementation. It can increase CPU work or limit throughput on hardware where GSO works correctly; CPU/battery/max-throughput costs have not been measured. The user requested only a small functional check for this update, not a repeat of the large-file trials.

### Required review when updating Iroh or Sendme

1. Recheck issue #4555 and PR #4645 (or the replacement fix) and confirm which released Iroh version contains it; do not assume an open PR is included.
2. Check the resolved Iroh version in both applicable lockfiles, not only the declared dependency range.
3. Trial removal of the two Android-only GSO blocks, then use small phone/PC checksum transfers with actual Direct route evidence. If the upstream release resolves the issue, remove the workaround; otherwise retain it and record the tested dependency version.
