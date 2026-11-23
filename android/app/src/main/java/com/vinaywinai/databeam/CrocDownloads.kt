package com.vinaywinai.databeam

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore.Downloads as Downloads
import android.system.Os
import android.system.OsConstants
import org.json.JSONObject
import java.io.File

/** Croc can seek and write inside app-owned pending Downloads rows. */
object CrocDownloads {
    private const val relativeRoot = "Download/DataBeam/"
    private fun root() = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "DataBeam").canonicalFile

    private data class Download(val disk: File, val parent: String, val name: String, val pending: Boolean)

    private fun lookup(c: Context, uri: Uri): Download? {
        val columns = arrayOf(Downloads.DATA, Downloads.RELATIVE_PATH, Downloads.DISPLAY_NAME, Downloads.IS_PENDING)
        return c.contentResolver.query(uri, columns, null, null, null)?.use {
            if (it.moveToFirst()) Download(File(it.getString(0)), it.getString(1), it.getString(2), it.getInt(3) == 1) else null
        }
    }

    private fun child(parent: File, path: String): File {
        require(path.split('/').all { it.isNotBlank() && it != "." && it != ".." && !it.contains('\\') })
        val file = File(parent, path)
        require(file.absoluteFile == file.canonicalFile) { "The download destination changed" }
        return file
    }

    private fun location(wrapper: String, path: String) = relativeRoot + wrapper + "/" +
        path.substringBeforeLast('/', "").let { if (it.isEmpty()) "" else "$it/" }

    fun reserve(c: Context, preferred: String): String {
        val base = root()
        check(base.isDirectory || base.mkdirs()) { "Could not open Downloads / DataBeam" }
        repeat(20) {
            val name = Storage.availableWrapper(c, "", preferred)
            if (child(base, name).mkdir()) return name
        }
        error("Could not create a new download folder")
    }

    fun prepare(
        c: Context, offer: JSONObject, wrapper: String, rows: JSONObject, stopped: () -> Boolean,
        store: () -> Unit,
    ): JSONObject {
        val base = child(root(), wrapper)
        check(base.isDirectory) { "The saved Croc folder is missing" }
        val paths = JSONObject()
        val entries = offer.getJSONArray("entries")
        for (index in 0 until entries.length()) {
            check(!stopped()) { "Paused" }
            val entry = entries.getJSONObject(index)
            if (entry.getString("kind") != "file") continue
            val path = entry.getString("path")
            val file = child(base, path)
            val parent = location(wrapper, path)
            val saved = rows.optString(path)
            val previous = saved.takeIf { it.isNotEmpty() }?.let(Uri::parse)
            val uri = if (previous != null && runCatching { lookup(c, previous) }.getOrNull() != null) previous else {
                val created = c.contentResolver.insert(Downloads.EXTERNAL_CONTENT_URI, ContentValues().apply {
                    put(Downloads.DISPLAY_NAME, file.name)
                    put(Downloads.MIME_TYPE, java.net.URLConnection.guessContentTypeFromName(file.name) ?: "application/octet-stream")
                    put(Downloads.RELATIVE_PATH, parent)
                    put(Downloads.IS_PENDING, 1)
                }) ?: error("Could not reserve ${file.name} in Downloads")
                rows.put(path, created.toString())
                store()
                created
            }
            c.contentResolver.openFileDescriptor(uri, "rw")?.close() ?: error("Could not open ${file.name} in Downloads")
            val item = lookup(c, uri) ?: error("Could not find ${file.name} in Downloads")
            check(item.pending && item.parent == parent && item.name == file.name &&
                item.disk.parentFile?.canonicalFile == file.parentFile &&
                item.disk.name.startsWith(".pending-")) { "Downloads changed the requested file path" }
            Os.close(Os.open(item.disk.path, OsConstants.O_RDWR or OsConstants.O_NOFOLLOW, 0))
            paths.put(path, item.disk.path)
        }
        return paths
    }

    fun publish(c: Context, offer: JSONObject, wrapper: String, rows: JSONObject, stopped: () -> Boolean): JSONObject {
        val base = child(root(), wrapper)
        val entries = offer.getJSONArray("entries")
        val directories = JSONObject()
        for (index in 0 until entries.length()) {
            check(!stopped()) { "Paused" }
            val entry = entries.getJSONObject(index)
            val path = entry.getString("path")
            val file = child(base, path)
            if (entry.getString("kind") == "directory") {
                check(file.isDirectory || file.mkdirs()) { "Could not save folder $path" }
                directories.put(path, Uri.fromFile(file).toString())
                continue
            }
            val uri = Uri.parse(rows.getString(path))
            val item = lookup(c, uri) ?: error("Missing saved download $path")
            check(item.parent == location(wrapper, path) && item.name == file.name &&
                item.disk.length() == entry.getLong("size")) { "The saved download changed: $path" }
            if (item.pending) {
                check(c.contentResolver.update(uri, ContentValues().apply { put(Downloads.IS_PENDING, 0) }, null, null) == 1) {
                    "Could not publish $path"
                }
            }
        }
        return directories
    }

    fun discard(c: Context, wrapper: String, rows: JSONObject) {
        val base = child(root(), wrapper)
        for (path in rows.keys()) {
            val uri = Uri.parse(rows.getString(path))
            val item = runCatching { lookup(c, uri) }.getOrNull() ?: continue
            if (item.pending && item.parent == location(wrapper, path)) c.contentResolver.delete(uri, null, null)
        }
        removeEmptyFolders(base, rows.keys().asSequence().map { it.substringBeforeLast('/', "") })
    }

    fun resetRevision(
        c: Context, wrapper: String, rows: JSONObject, revision: JSONObject, store: () -> Unit,
    ) {
        val base = child(root(), wrapper)
        check(base.isDirectory) { "The saved Croc folder is missing" }
        val paths = revision.getJSONArray("files")
        for (index in 0 until paths.length()) {
            val path = paths.getString(index)
            val file = child(base, path)
            val saved = rows.optString(path)
            if (saved.isEmpty()) continue
            val uri = Uri.parse(saved)
            check(uri.authority == "media" && uri.path.orEmpty().matches(Regex("/(external|external_primary)/downloads/[0-9]+")))
            val item = lookup(c, uri)
            if (item != null) {
                val owner = c.contentResolver.query(uri, arrayOf(Downloads.OWNER_PACKAGE_NAME), null, null, null)
                    ?.use { if (it.moveToFirst()) it.getString(0) else null }
                check(owner == c.packageName && item.pending && item.parent == location(wrapper, path) &&
                    item.name == file.name && item.disk.parentFile?.canonicalFile == file.parentFile) {
                    "The pending download changed: $path"
                }
                check(c.contentResolver.delete(uri, null, null) == 1) { "Could not clear the old partial file: $path" }
            }
            rows.remove(path)
            store()
        }
        val directories = revision.getJSONArray("directories")
        for (index in 0 until directories.length()) {
            val folder = child(base, directories.getString(index))
            if (folder.isDirectory) folder.delete()
        }
    }

    private fun removeEmptyFolders(folder: File, relativeDirectories: Sequence<String>) {
        val paths = relativeDirectories.flatMap { path ->
            generateSequence(path.takeIf(String::isNotEmpty)) { it.substringBeforeLast('/', "").takeIf(String::isNotEmpty) }
        }.distinct().sortedByDescending { it.length }
        paths.forEach { child(folder, it).takeIf(File::isDirectory)?.delete() }
        if (folder.isDirectory && folder.canonicalFile == folder) folder.delete()
    }

    fun moveOut(
        c: Context, wrapper: String, rows: JSONObject, directories: JSONObject, targets: JSONObject,
        stopped: () -> Boolean = { false }, store: () -> Unit,
    ) {
        val base = child(root(), wrapper)
        check(base.isDirectory || (!base.exists() && targets.length() > 0)) { "The saved folder is missing" }
        val names = rows.keys().asSequence().plus(directories.keys().asSequence()).map { it.substringBefore('/') }.distinct()
        val occupied = root().list().orEmpty().toMutableSet()
        occupied.addAll(targets.keys().asSequence().map(targets::getString))
        for (name in names) {
            if (!targets.has(name)) {
                val target = Storage.uniqueName(name, occupied)
                occupied.add(target)
                targets.put(name, target)
            }
        }
        store()
        fun movedPath(path: String): String = targets.getString(path.substringBefore('/')) + path.substringAfter('/', "").let {
            if (it.isEmpty()) "" else "/$it"
        }
        for (path in rows.keys()) {
            check(!stopped()) { "Paused" }
            val uri = Uri.parse(rows.getString(path))
            val item = lookup(c, uri) ?: error("Missing saved file $path")
            val relative = movedPath(path)
            val moved = child(root(), relative)
            val parent = relativeRoot + relative.substringBeforeLast('/', "").let { if (it.isEmpty()) "" else "$it/" }
            check(!item.pending) { "Finish saving before moving files" }
            if (item.parent == parent && item.name == moved.name) continue
            check(item.parent == location(wrapper, path) && !moved.exists()) { "The destination changed for $path" }
            check(c.contentResolver.update(uri, ContentValues().apply {
                put(Downloads.RELATIVE_PATH, parent)
                put(Downloads.DISPLAY_NAME, moved.name)
            }, null, null) == 1) { "Could not move $path" }
            val saved = lookup(c, uri)
            check(saved?.parent == parent && saved.name == moved.name) { "Downloads renamed $path while moving it" }
        }
        for (path in directories.keys()) {
            check(!stopped()) { "Paused" }
            val directory = child(root(), movedPath(path))
            check(directory.isDirectory || directory.mkdirs()) { "Could not move folder $path" }
            directories.put(path, Uri.fromFile(directory).toString())
        }
        removeEmptyFolders(base, rows.keys().asSequence().map { it.substringBeforeLast('/', "") } + directories.keys().asSequence())
    }
}
