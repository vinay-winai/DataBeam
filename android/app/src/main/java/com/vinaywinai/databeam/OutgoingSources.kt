package com.vinaywinai.databeam

import android.content.Context
import android.content.res.AssetFileDescriptor
import android.net.Uri
import android.os.Process
import android.provider.DocumentsContract as D
import android.system.Os
import android.system.OsConstants
import java.io.Closeable
import java.io.File
import java.nio.file.Files

/** Named aliases to held provider descriptors; no payload copy for ordinary seekable files. */
class OutgoingSources(private val context:Context,val directory:File,private val stopped:()->Boolean,private val copying:()->Unit):Closeable {
    private val held=mutableListOf<AssetFileDescriptor>()
    private val aliases=mutableListOf<File>()
    var referenced=0;private set
    var copied=0;private set
    init{check(directory.mkdirs() || directory.isDirectory)}
    private fun safe(name:String):String {
        require(name.isNotBlank() && name!="." && name!=".." && name.none{it=='/' || it=='\\' || it=='\u0000'}){"Unsafe file name"};return name
    }
    fun add(uri:Uri):File {
        check(!stopped()){"Stopped"}
        if(D.isTreeUri(uri))return document(D.buildDocumentUriUsingTree(uri,D.getTreeDocumentId(uri)),directory)
        val name=if(uri.scheme=="file")File(uri.path!!).name else Storage.name(context,uri)
        return file(uri,File(directory,safe(name)))
    }
    private fun document(uri:Uri,parent:File):File {
        check(!stopped()){"Stopped"}
        var name="";var type=""
        context.contentResolver.query(uri,arrayOf(D.Document.COLUMN_DISPLAY_NAME,D.Document.COLUMN_MIME_TYPE),null,null,null)!!.use{cursor->check(cursor.moveToFirst());name=cursor.getString(0);type=cursor.getString(1)}
        val target=File(parent,safe(name));require(!target.exists() && !Files.isSymbolicLink(target.toPath())){"Duplicate file or folder name: $name"}
        if(type!=D.Document.MIME_TYPE_DIR)return file(uri,target)
        check(target.mkdir())
        context.contentResolver.query(D.buildChildDocumentsUriUsingTree(uri,D.getDocumentId(uri)),arrayOf(D.Document.COLUMN_DOCUMENT_ID),null,null,null)!!.use{cursor->
            while(cursor.moveToNext())document(D.buildDocumentUriUsingTree(uri,cursor.getString(0)),target)
        }
        return target
    }
    private fun file(uri:Uri,target:File):File {
        require(!target.exists() && !Files.isSymbolicLink(target.toPath())){"Two selected items have the same name. Select them separately."}
        var descriptor:AssetFileDescriptor?=null
        try {
            descriptor=context.contentResolver.openAssetFileDescriptor(uri,"r")
            val source=descriptor
            val direct=source!=null && held.size<256 && runCatching{
                val stat=Os.fstat(source.fileDescriptor)
                OsConstants.S_ISREG(stat.st_mode) && source.startOffset==0L &&
                    (source.declaredLength==AssetFileDescriptor.UNKNOWN_LENGTH || source.declaredLength==stat.st_size) &&
                    Os.lseek(source.fileDescriptor,0,OsConstants.SEEK_CUR)>=0 &&
                    // A granted FD may belong to another app's private inode. Android
                    // can permit reads on that FD but forbid reopening /proc/.../fd.
                    // Both engines reopen named paths, so verify that access first.
                    java.io.FileInputStream("/proc/${Process.myPid()}/fd/${source.parcelFileDescriptor.fd}").use{true}
            }.getOrDefault(false)
            if(direct){
                Os.symlink("/proc/${Process.myPid()}/fd/${source!!.parcelFileDescriptor.fd}",target.absolutePath)
                held.add(source);aliases.add(target);descriptor=null;referenced++;return target
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
    override fun close(){
        // Remove aliases before descriptor numbers can be reused for another file.
        val root=directory.canonicalFile.toPath()
        aliases.forEach{alias->runCatching{if(alias.parentFile.canonicalFile.toPath().startsWith(root) && Files.isSymbolicLink(alias.toPath()))Files.deleteIfExists(alias.toPath())}}
        held.forEach{runCatching{it.close()}};held.clear();aliases.clear()
    }
}
