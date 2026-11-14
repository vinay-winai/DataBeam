# Android source references and staging

Checked on 2026-11-15.

## DataBeam today

Desktop passes the selected filesystem paths directly to Croc. The shared Sendme sender imports original paths with `ImportMode::TryReference` (`third_party/sendme/src/core/send.rs`). DataBeam does not create Android-style full staging copies on desktop. This is a reference import, not necessarily a filesystem symbolic link.

Android receives `content://` document/share URIs. `Storage.stage` currently reads them through `ContentResolver` into `filesDir/outgoing/<session>/` before calling the path-based engines. Folder selections are enumerated and copied. The current local sender-restart action reuses those staged paths; it does not stage the selection a second time. A device regression verified restarting a cached file after deleting the original generated MediaStore URI.

## What DashBeam does

The latest release checked was [DashBeam v0.7.2](https://github.com/tonyantony300/dashbeam/releases/tag/v0.7.2). Its Android code also copies picker/share selections:

- [NativeUtils.kt](https://github.com/tonyantony300/dashbeam/blob/v0.7.2/src-tauri/plugins/tauri-plugin-native-utils/android/src/main/java/com/dashbeam/plugin/native_utils/NativeUtils.kt#L529): `startUriCopy` creates a directory under `cacheDir/file_cache` and copies one or more selected URIs into it.
- [CopyUtils.kt](https://github.com/tonyantony300/dashbeam/blob/v0.7.2/src-tauri/plugins/tauri-plugin-native-utils/android/src/main/java/com/dashbeam/plugin/native_utils/CopyUtils.kt#L87): `copyUri` reads the provider's input stream and writes an actual destination file. The folder variant repeats this for the selected tree's files.

The same routines were present on main at `c5de16552fc946ec9d0ad3b3792875868e826a05`; the checked released copy utility had the same file SHA as main. This is evidence of Android staging, not of a direct URI-reference send path.

## Avoiding full copies

Android can open an original URI as a file descriptor. Copying every selection is an implementation choice, not an Android requirement. A future direct-reference bridge needs to preserve names, permissions and descriptor lifetime, and support the transfer engines' file-access behavior. Croc runs in a separate process; the native Sendme engine runs through JNI. Passing a URI string to their existing filesystem-path interfaces is insufficient.

[ContentResolver.openFileDescriptor](https://developer.android.com/reference/android/content/ContentResolver#openFileDescriptor(android.net.Uri,%20java.lang.String)) can return a regular file or a streaming pipe/socket. A direct reader must account for seekability and provider offsets/lengths rather than treating every descriptor as a whole ordinary file. Persistable grants should be retained where offered; temporary share grants can expire, and an original can be changed or removed. A staged fallback remains useful for unsupported providers and restart guarantees.

No direct URI/descriptor send adapter is shipped by this UI revision.
