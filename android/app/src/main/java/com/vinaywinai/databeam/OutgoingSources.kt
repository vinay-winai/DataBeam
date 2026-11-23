package com.vinaywinai.databeam

import android.content.Context
import android.content.res.AssetFileDescriptor
import android.icu.lang.UCharacter
import android.net.LocalServerSocket
import android.net.Uri
import android.os.Process
import android.provider.DocumentsContract as D
import android.system.Os
import android.system.OsConstants
import java.io.Closeable
import java.io.DataInputStream
import java.io.File
import java.io.FileDescriptor
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.file.Files
import java.security.MessageDigest
import java.text.Normalizer
import java.util.UUID

/** Named aliases to held provider descriptors; no payload copy for ordinary seekable files. */
class OutgoingSources(private val context:Context,val directory:File,private val stopped:()->Boolean,private val copying:()->Unit,private val allowRestrictedDescriptors:Boolean=false):Closeable {
    private data class Entry(val uri: Uri, val name: String, val folder: Boolean = false)

    private val held=mutableListOf<AssetFileDescriptor>()
    private val aliases=mutableListOf<File>()
    private val restrictedDescriptors=mutableMapOf<File,AssetFileDescriptor>()
    private var broker:LocalServerSocket?=null
    var referenced=0;private set
    var copied=0;private set
    init{check(directory.mkdirs() || directory.isDirectory)}
    private fun safe(name:String):String {
        require(name.isNotBlank() && name!="." && name!=".." && name.none{it=='/' || it=='\\' || it=='\u0000'}){"Unsafe file name"};return name
    }
    fun addAll(uris: List<Uri>): List<File> = stageEntries(uris.map { uri ->
        check(!stopped()) { "Stopped" }
        if (D.isTreeUri(uri)) {
            document(D.buildDocumentUriUsingTree(uri, D.getTreeDocumentId(uri)))
        } else {
            val name = if (uri.scheme == "file") File(uri.path!!).name else Storage.name(context, uri)
            Entry(uri, safe(name))
        }
    }, directory)

    private fun nameKey(name: String): String =
        UCharacter.foldCase(Normalizer.normalize(name, Normalizer.Form.NFC), true)

    private fun stageEntries(entries: List<Entry>, parent: File): List<File> {
        val claimed = parent.list().orEmpty().map(::nameKey).toMutableSet()
        val reserved = entries.mapTo(claimed.toMutableSet()) { nameKey(it.name) }
        val names = arrayOfNulls<String>(entries.size)
        // Stable assignments let selecting the same documents in another order resume.
        for (index in entries.indices.sortedBy { entries[it].uri.toString() }) {
            val entry = entries[index]
            val name = if (claimed.add(nameKey(entry.name))) {
                entry.name
            } else {
                Storage.uniqueName(entry.name) { nameKey(it) in reserved }.also {
                    val key = nameKey(it)
                    reserved.add(key)
                    claimed.add(key)
                }
            }
            names[index] = name
        }
        return entries.mapIndexed { index, entry ->
            check(!stopped()) { "Stopped" }
            val target = File(parent, checkNotNull(names[index]))
            if (entry.folder) stageFolder(entry.uri, target) else file(entry.uri, target)
        }
    }

    private fun document(uri: Uri): Entry {
        check(!stopped()){"Stopped"}
        return context.contentResolver.query(uri,arrayOf(D.Document.COLUMN_DISPLAY_NAME,D.Document.COLUMN_MIME_TYPE),null,null,null)!!.use{cursor->
            check(cursor.moveToFirst())
            Entry(uri, safe(cursor.getString(0)), cursor.getString(1) == D.Document.MIME_TYPE_DIR)
        }
    }

    private fun stageFolder(uri: Uri, target: File): File {
        check(target.mkdir()) { "Could not prepare folder ${target.name}" }
        val children = context.contentResolver.query(
            D.buildChildDocumentsUriUsingTree(uri, D.getDocumentId(uri)),
            arrayOf(D.Document.COLUMN_DOCUMENT_ID), null, null, null,
        )!!.use { cursor ->
            buildList {
                while (cursor.moveToNext()) add(D.buildDocumentUriUsingTree(uri, cursor.getString(0)))
            }
        }
        stageEntries(children.map(::document), target)
        return target
    }
    private fun file(uri:Uri,target:File):File {
        require(!target.exists() && !Files.isSymbolicLink(target.toPath())){"Two selected items have the same name. Select them separately."}
        var descriptor:AssetFileDescriptor?=null
        try {
            descriptor=context.contentResolver.openAssetFileDescriptor(uri,"r")
            val source=descriptor
            val seekable=source!=null && held.size<256 && runCatching{
                val stat=Os.fstat(source.fileDescriptor)
                OsConstants.S_ISREG(stat.st_mode) && source.startOffset==0L &&
                    (source.declaredLength==AssetFileDescriptor.UNKNOWN_LENGTH || source.declaredLength==stat.st_size) &&
                    Os.lseek(source.fileDescriptor,0,OsConstants.SEEK_CUR)>=0
            }.getOrDefault(false)
            if(seekable && source!=null){
                val fd=source.parcelFileDescriptor.fd
                val procPath="/proc/${Process.myPid()}/fd/$fd"
                val canReopen=runCatching{java.io.FileInputStream(procPath).use{true}}.getOrDefault(false)
                val canPass= !canReopen && allowRestrictedDescriptors && runCatching{
                    Os.fstat(source.fileDescriptor).st_size==0L ||
                        Os.pread(source.fileDescriptor,ByteArray(1),0,1,0)==1
                }.getOrDefault(false)
                if(canReopen || canPass){
                    Os.symlink(if(canPass)"databeam-fd:$fd" else procPath,target.absolutePath)
                    if(canPass)restrictedDescriptors[target]=source
                    held.add(source);aliases.add(target);descriptor=null;referenced++;return target
                }
            }
            copying()
            val input=source?.createInputStream() ?: context.contentResolver.openInputStream(uri) ?: error("Cannot read selected file")
            input.use{from->target.outputStream().use{to->
                val buffer=ByteArray(256*1024)
                while(true){check(!stopped()){"Stopped"};val count=from.read(buffer);if(count<0)break;to.write(buffer,0,count)}
            }}
            copied++;return target
        }catch(error:Throwable){target.delete();throw error}
        finally{descriptor?.close()}
    }

    fun descriptor(file:File):FileDescriptor? = restrictedDescriptors[file]?.fileDescriptor

    fun sendmeDescriptors(paths:List<File>):Map<String,Int> {
        val selections=paths.map{it.toPath()}
        return restrictedDescriptors.entries.filter{(file,_)->selections.any{file.toPath().startsWith(it)}}
            .associate{(file,source)->file.absolutePath to source.parcelFileDescriptor.fd}
    }

    fun startCrocDescriptorBroker():String? {
        if(restrictedDescriptors.isEmpty())return null
        val name="databeam-croc-${UUID.randomUUID()}"
        val listener=LocalServerSocket(name)
        val files=restrictedDescriptors.values.associateBy{it.parcelFileDescriptor.fd}
        synchronized(this){broker=listener}
        Thread({
            while(true){
                val peer=try{listener.accept()}catch(_:IOException){break}
                try{
                    peer.use{socket->
                        if(socket.peerCredentials.uid!=Process.myUid())return@use
                        socket.soTimeout=1000
                        val requested=DataInputStream(socket.inputStream).readInt()
                        synchronized(this){
                            val heldFile=files[requested]
                            if(broker===listener && heldFile!=null){
                                socket.setFileDescriptorsForSend(arrayOf(heldFile.fileDescriptor))
                                socket.outputStream.write(1)
                            }
                        }
                    }
                }catch(_:IOException){/* Croc stopped while requesting a descriptor. */}
            }
        },"DataBeam Croc source descriptors").apply{isDaemon=true;start()}
        return name
    }

    @Synchronized override fun close(){
        broker?.close();broker=null
        // Remove aliases before descriptor numbers can be reused for another file.
        val root=directory.canonicalFile.toPath()
        aliases.forEach{alias->runCatching{if(alias.parentFile.canonicalFile.toPath().startsWith(root) && Files.isSymbolicLink(alias.toPath()))Files.deleteIfExists(alias.toPath())}}
        held.forEach{runCatching{it.close()}};held.clear();aliases.clear();restrictedDescriptors.clear()
    }
}

internal fun withoutDuplicateFiles(
    paths: List<File>, stopped: () -> Boolean, descriptor: (File) -> FileDescriptor? = { null }, checking: () -> Unit,
): List<File> {
    val sizes = paths.map { path -> descriptor(path)?.let { Os.fstat(it).st_size } ?: path.takeIf(File::isFile)?.length() }
    val counts = sizes.filterNotNull().groupingBy { it }.eachCount()
    if (counts.values.none { it > 1 }) return paths

    checking()
    val hashes = mutableSetOf<ByteBuffer>()
    val buffer = ByteArray(256 * 1024)
    return paths.filterIndexed { index, file ->
        check(!stopped()) { "Stopped" }
        val size = sizes[index]
        if (size == null || counts.getValue(size) == 1) return@filterIndexed true
        val fd = descriptor(file)
        val modified = fd?.let { Os.fstat(it).st_mtime } ?: file.lastModified()
        val digest = MessageDigest.getInstance("SHA-256")
        if(fd!=null){
            var offset=0L
            while(offset<size){
                check(!stopped()){ "Stopped" }
                val count=Os.pread(fd,buffer,0,minOf(buffer.size.toLong(),size-offset).toInt(),offset)
                check(count>0){ "File changed while checking duplicates. Select it again." }
                digest.update(buffer,0,count);offset+=count
            }
            val after=Os.fstat(fd)
            check(after.st_size==size && after.st_mtime==modified){ "File changed while checking duplicates. Select it again." }
        }else file.inputStream().use { stream ->
            while (true) {
                check(!stopped()) { "Stopped" }
                val count = stream.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
            check(file.length() == size && file.lastModified() == modified) { "File changed while checking duplicates. Select it again." }
        }
        hashes.add(ByteBuffer.wrap(digest.digest()))
    }
}
