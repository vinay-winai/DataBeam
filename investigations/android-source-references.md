# Android source access and measured storage behavior

Checked on 2026-11-15. These changes apply to Android only.

## DataBeam source access

Desktop passes original filesystem paths to Croc and imports Sendme paths with `ImportMode::TryReference`. Its behavior is unchanged.

Android opens selected Content URIs with `ContentResolver.openAssetFileDescriptor`. `OutgoingSources` holds accessible seekable, whole-file descriptors and creates named aliases under its private transfer tree pointing to `/proc/<app PID>/fd/<fd>`. Engines read original bytes through those aliases without a payload staging copy. Display filenames, folder hierarchy, zero-byte files and empty folders are preserved. Closing a sender removes aliases before releasing descriptors, preventing descriptor-number reuse from redirecting stale aliases.

The Android-only Sendme adapter preserves descriptor aliases during import. A separately applied Android Croc patch recognizes aliases only beneath the declared private source tree, pointing to the parent app's numeric FD. Enumeration, hashing and source validation use regular target bytes. Desktop Croc's recipe is unchanged; both applications remain on 11.5.5.

A private copy remains necessary for nonseekable pipes, bounded asset slices, provider-private inodes that Android allows reading but forbids reopening, and selections beyond 256 held descriptors. Being a regular seekable FD is insufficient: an actual independent open must succeed. The bounded fallback uses the asset stream so bytes outside the permitted slice are excluded.

Restart reopens referenced original URIs. Originals must remain available and unchanged; temporary share grants may expire. An intact all-copy fallback snapshot can still be reused after its original is unavailable. This differs from the earlier always-copy implementation, which could restart every source from its private snapshot. No receiver remotely restarts a stopped sender.

## DashBeam 0.7.1: observed on the installed phone

These findings come from real share selection, transfers, Android per-UID storage statistics and received-file checksums. The installed package was `com.dashbeam.android`, version 0.7.1 (7001).

| Stage | App data bytes | Cache bytes (included in data) |
| --- | ---: | ---: |
| Before selecting generated 64 MiB source | 10,018,816 | 1,363,968 |
| After selection, before transfer | 77,242,368 | 68,587,520 |
| After exiting/reopening the earlier share | 9,412,608 | 765,952 |

Selection added approximately a full 64 MiB copy. That transfer exceeded the 240-second receiver deadline. The native receiver observed Direct briefly, then repeated Relay paths with roughly 14–16-second relay RTT. This explains slowness during this run, not every DashBeam transfer.

A separate generated 1 MiB phone-to-PC transfer completed with an exact SHA-256 match. Cache remained about 1,822,720 bytes after completion and pressing Done. The larger run's later reduction shows lifecycle cleanup can reclaim temporary copies; these measurements do not establish immediate cleanup on Stop or Done. No routing workaround was added to DataBeam.

`DashBeamStorageProbe` reproduces the experiment and deletes its generated original MediaStore item afterward. Android's per-UID `dataBytes` includes cache; the columns must not be added. DashBeam's own data was not reset.

## DataBeam cleanup

At the user's request, all DataBeam private app data was reset before this revision's tests: 3,045,110 KiB fell to 4 KiB. The thirteen existing public Downloads files were preserved. Old pre-reset preferences are not restored.

Publication records destination URIs and verifies lengths plus SHA-256 before deleting a private received working directory. A mismatch retains the work; re-publication creates a distinct name instead of replacing a changed public file. Legacy working copies are removed automatically only after matching their public saved bytes. Resume blob caches are separate from these exported working copies.

While idle, cleanup offers Keep/Delete for unused transfer data and ages known non-current caches after seven days. Unknown legacy data is offered for deletion rather than silently expired. Current resume/restart sources and caches are protected. Cleanup confines deletion to validated private transfer roots, does not follow symlink targets, and never deletes public saved documents. Checks run at startup or from Settings, not at an exact scheduled time.

## Other clients: source inspection on 2026-11-15

These are source findings, not storage measurements on installed builds.

- [FlCroc](https://github.com/576576/FlCroc/blob/ce94b4930ea208e85f7c6ef284ba17e839634c02/lib/views/send/send.dart#L204) uses `file_picker` for file selections; the checked Android plugin implementation's `FileUtils.openFileStream` copies URI bytes to its cache. Folder selection can return a real path and FlCroc passes that path directly. FlCroc also contains an explicit temporary-file branch for Content URI paths; its presence does not prove that branch successfully reads an Android URI using Dart's ordinary `File` API.
- [Dking's croc-app](https://github.com/Dking08/croc-app/blob/037d93107311870e555fe0d7206ffae4b0002473/app/src/main/java/com/dking/crocapp/ui/send/SendViewModel.kt#L453) explicitly copies picked file bytes to `cacheDir/croc-send` before sending. Folder selection recursively copies to `croc-send-folder`. It calls staging cleanup after completion/cancellation and other session actions.
- [bakirgdev's CrocApp](https://github.com/bakirgdev/CrocApp/blob/3f8902fe3d594df192d53b66820bc65403838d72/app/CrocApp/Views/SendView.swift#L270) passes in-app file/folder selections as URLs and holds security-scoped access while the engine sends them. Photos/videos and share-extension handoffs use staging copies. This distinction concerns app-created copies; it does not establish whether an OS/cloud file provider downloads or materializes a source itself.
