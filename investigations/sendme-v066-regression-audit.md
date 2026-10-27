# Sendme v0.6.6 VPN regression audit

Compared DataBeam `v0.6.5` with the initial v0.6.6 commit `10bfdc4` on
2026-10-26. The user recalls transfers working the previous day and reproduced
VPN failures in both directions. The exact version/path used on the previous
day is not yet established by an old-versus-new network comparison.

## What changed

| Component | v0.6.5 | Initial v0.6.6 |
| --- | --- | --- |
| Customized Sendme package label | 0.36.0 | 0.37.1 |
| iroh / iroh-relay | 1.0.2 | 1.0.2 |
| noq / noq-udp | 1.0.1 | 1.0.1 |
| noq-proto | 1.1.1 | 1.1.1 |
| iroh-blobs | 0.103.0 | 0.103.1 |
| Tokio | 1.49.0 | 1.53.3 |
| Mio | 1.1.1 | 1.2.4 |
| libc | 0.2.182 | 0.2.191 |
| rustls | 0.23.36 | 0.23.46 |
| socket2 selected by noq / noq-udp | 0.6.2 | 0.5.10 |
| socket2 selected by netwatch | 0.6.2 | 0.6.5 |

The custom sender implementation itself did not change in the initial update.
Both the metadata-completion heuristic and the lack of response acknowledgment
were already present in v0.6.5. Reproducing those defects therefore establishes
real bugs, but does not establish that they caused a new version regression.

The new receive-side path watcher subscribes to connection-path updates. It
does not configure route selection or force a relay path. Its effect on a real
VPN transfer has not been independently isolated.

## Security patch review

A source comparison of cached iroh-blobs 0.103.0 and 0.103.1 found changes to
request-type authorization, error logging in the provider/store, and hash-list
iteration. The download implementation is unchanged. `GetRequest` still uses
the same `EventMask.get` setting; the authorization fix selects the correct
settings for the other request types, including disabled pushes.

This agrees with the official [Sendme update PR](https://github.com/n0-computer/sendme/pull/144)
and [iroh-blobs changelog](https://github.com/n0-computer/iroh-blobs/blob/main/CHANGELOG.md).
It makes the security fix a less direct suspect, but does not by itself prove
the complete updated dependency graph is free of regressions.

## Controlled comparisons

- A local v0.6.5 Windows control EXE matches the official release SHA-256:
  `a0cb72aed98fa2145001c0b6eb0fa8fa647aeaf4b25a49886b882ca1b18ddac3`.
- The user agreed to test v0.6.5 on both devices with the same file, direction,
  VPN servers and protocols, and confirmed that both v0.6.5 devices also fail
  with `error: io: connection lost`. This shows that the v0.6.6 update is not
  required to trigger the VPN failure. It does not establish whether dependency
  changes affect its frequency or timing.
- An isolated comparison build keeps the current acknowledgment and metadata
  corrections, security patch, and all dependency versions unchanged, but
  changes the `noq` and `noq-udp` socket2 dependency edges from 0.5.10 to the
  existing 0.6.5 package. Its source and lockfile live under the ignored
  `target/transfer-watch/socket2-compare-src` directory. This is a diagnostic
  comparison, not a production rollback or a claim that socket2 is faulty.

If both v0.6.5 peers succeed and both v0.6.6 peers fail under the same network
conditions, an update regression is supported. Mixed-version and socket-only
comparisons can then isolate the side and dependency change. A failure with
only one old peer does not rule out a regression in the other, updated peer.

## CLI export error and real relay test

The user also sent from the official Sendme CLI to DataBeam and reached a
separate file-export error. Inspection found that a plain filename was created
as a directory by destination resolution and then reused as the target file.
The same wrapper-folder assumption exists in v0.6.5.

The CLI-style export regression failed before the local correction and passes
after it, including flat names containing spaces, mixed top-level/nested
collections, a file named `databeam_hash.txt`, and local-cache re-export. A
completed download now explicitly shuts down its store before reuse; cached
export also closes the store before cleanup on Windows. Export failures retain
their underlying I/O error chain and target path.

On 2026-10-27, receiver-side wrapping was added at the user's request. A CLI
collection with no common enclosing folder is normalized for export into a
`_databeam` folder, using the GUI sender's largest-item naming convention.
Existing enclosing folders are preserved. Only export names are normalized:
ticket hashes, stored collections and verified file hashes remain unchanged.
Fresh downloads and local-cache exports use the same normalization. Tests
verify the exact folder names, nested paths, preservation of an old erroneous
directory named after a file, and that GUI collections are not wrapped twice.

A separate public-relay-only integration test passed with a synthetic 256 KiB
flat CLI-style file. The server's IP transport was disabled, the receiver
reported Relay, and the exported bytes matched exactly. An earlier wrapped-file
relay trial also passed. This verifies real public-relay operation on the local
Windows test machine, not the exact two-device Proton VPN environment.
