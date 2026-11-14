package com.vinaywinai.databeam
import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract as D
import android.provider.OpenableColumns
import java.io.File
object Storage {
    private data class Child(val name:String,val type:String,val uri:Uri)
    private fun children(c:Context,parent:Uri):List<Child> {
        val list=mutableListOf<Child>()
        val query=D.buildChildDocumentsUriUsingTree(parent,D.getDocumentId(parent))
        c.contentResolver.query(query,arrayOf(D.Document.COLUMN_DOCUMENT_ID,D.Document.COLUMN_DISPLAY_NAME,D.Document.COLUMN_MIME_TYPE),null,null,null)?.use{cursor->
            while(cursor.moveToNext())list.add(Child(cursor.getString(1),cursor.getString(2),D.buildDocumentUriUsingTree(parent,cursor.getString(0))))
        } ?: error("Cannot read the selected save folder. Choose it again in Settings.")
        return list
    }
    private fun uniqueName(name:String,existing:Set<String>):String {
        if(name !in existing)return name
        val dot=name.lastIndexOf('.').takeIf{it>0} ?: name.length
        val base=name.substring(0,dot);val extension=name.substring(dot)
        var number=1
        while("$base ($number)$extension" in existing)number++
        return "$base ($number)$extension"
    }
    private fun createFresh(c:Context,parent:Uri,type:String,name:String):Uri {
        val existing=children(c,parent)
        val uri=D.createDocument(c.contentResolver,parent,type,uniqueName(safe(name),existing.map{it.name}.toSet())) ?: error("Cannot save $name in the selected folder")
        // Never open an existing document for writing, even with an unusual provider.
        check(existing.none{it.uri==uri}){"The folder provider returned an existing file; choose a different folder"}
        return uri
    }
    private fun publishTree(c:Context,file:File,relative:String,tree:Uri,stopped:()->Boolean):Uri {
        var parent=D.buildDocumentUriUsingTree(tree,D.getTreeDocumentId(tree))
        relative.substringBeforeLast('/',"").split('/').filter{it.isNotEmpty()}.forEach{part->
            val found=children(c,parent).firstOrNull{it.name==safe(part) && it.type==D.Document.MIME_TYPE_DIR}
            parent=found?.uri ?: createFresh(c,parent,D.Document.MIME_TYPE_DIR,part)
        }
        check(!stopped()){"Paused"}
        val uri=createFresh(c,parent,java.net.URLConnection.guessContentTypeFromName(file.name) ?: "application/octet-stream",file.name)
        try {
            file.inputStream().use{input->c.contentResolver.openOutputStream(uri,"w")!!.use{out->
                val buffer=ByteArray(256*1024)
                while(true){check(!stopped()){"Paused"};val n=input.read(buffer);if(n<0)break;out.write(buffer,0,n)}
            }}
            return uri
        } catch(e:Throwable){runCatching{D.deleteDocument(c.contentResolver,uri)};throw e}
    }
    private fun safe(s:String):String {
        require(s.isNotBlank() && s!="." && s!=".." && !s.contains('/') && !s.contains('\\') && !s.contains('\u0000')) {"Unsafe file name"}
        return s
    }
    fun name(c:Context,u:Uri):String = c.contentResolver.query(u,arrayOf(OpenableColumns.DISPLAY_NAME),null,null,null)?.use {
        if(it.moveToFirst())it.getString(0) else null
    } ?: "shared-file"
    fun stage(c:Context,u:Uri,parent:File):File {
        if(D.isTreeUri(u)) {
            val doc=D.buildDocumentUriUsingTree(u,D.getTreeDocumentId(u))
            return stageDocument(c,doc,parent)
        }
        val destination=File(parent,safe(name(c,u)))
        require(!destination.exists()) {"Two selected items have the same name. Select them separately."}
        c.contentResolver.openInputStream(u)!!.use {input->destination.outputStream().use{input.copyTo(it)}}
        return destination
    }
    private fun stageDocument(c:Context,u:Uri,parent:File):File {
        var n="folder";var type=""
        c.contentResolver.query(u,arrayOf(D.Document.COLUMN_DISPLAY_NAME,D.Document.COLUMN_MIME_TYPE),null,null,null)!!.use {
            check(it.moveToFirst());n=it.getString(0);type=it.getString(1)
        }
        val dest=File(parent,safe(n));require(!dest.exists()){"Duplicate file or folder name: $n"}
        if(type==D.Document.MIME_TYPE_DIR) {
            check(dest.mkdirs());val children=D.buildChildDocumentsUriUsingTree(u,D.getDocumentId(u))
            c.contentResolver.query(children,arrayOf(D.Document.COLUMN_DOCUMENT_ID),null,null,null)!!.use {
                while(it.moveToNext()) stageDocument(c,D.buildDocumentUriUsingTree(u,it.getString(0)),dest)
            }
        } else c.contentResolver.openInputStream(u)!!.use{input->dest.outputStream().use{input.copyTo(it)}}
        return dest
    }
    fun export(c:Context,source:File,tree:Uri) {
        require(source.isDirectory){"No received files available"}
        val parent=D.buildDocumentUriUsingTree(tree,D.getTreeDocumentId(tree))
        // A fresh wrapper prevents touching existing files in the selected destination.
        val wrapper=D.createDocument(c.contentResolver,parent,D.Document.MIME_TYPE_DIR,"DataBeam-${System.currentTimeMillis()}") ?: error("Cannot create destination")
        source.listFiles()?.forEach{copyOut(c,it,wrapper)}
    }
    private fun copyOut(c:Context,f:File,parent:Uri) {
        val uri=D.createDocument(c.contentResolver,parent,if(f.isDirectory)D.Document.MIME_TYPE_DIR else "application/octet-stream",safe(f.name)) ?: error("Cannot create ${f.name}")
        if(f.isDirectory) f.listFiles()?.forEach{copyOut(c,it,uri)} else f.inputStream().use{input->c.contentResolver.openOutputStream(uri,"w")!!.use{input.copyTo(it)}}
    }

    /** Publish completed files automatically; record each URI to make export retries idempotent. */
    fun publishDownloads(c:Context,source:File,published:org.json.JSONObject,stopped:()->Boolean,folder:String="",directories:org.json.JSONObject=org.json.JSONObject(),recordDirectory:(String,String)->Unit={_,_->},record:(String,String)->Unit) {
        require(source.isDirectory){"Received files are not ready"}
        source.walkTopDown().filter{it.isDirectory && it!=source && it.list()?.isEmpty()==true}.forEach{directory->
            check(!stopped()){"Paused"}
            val relative=directory.relativeTo(source).invariantSeparatorsPath
            val parts=relative.split('/').map{safe(it)}
            val uri=if(folder.isNotEmpty()) {
                val tree=Uri.parse(folder)
                var parent=D.buildDocumentUriUsingTree(tree,D.getTreeDocumentId(tree))
                parts.forEach{part->
                    val found=children(c,parent).firstOrNull{it.name==part && it.type==D.Document.MIME_TYPE_DIR}
                    parent=found?.uri ?: createFresh(c,parent,D.Document.MIME_TYPE_DIR,part)
                }
                parent.toString()
            } else {
                val root=File(android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS),"DataBeam")
                val destination=File(root,parts.joinToString("/"))
                check(destination.isDirectory || destination.mkdirs()){"Cannot preserve empty folders in Downloads. Choose a save folder in Settings and export this transfer there."}
                Uri.fromFile(destination).toString()
            }
            if(directories.optString(relative)!=uri){directories.put(relative,uri);recordDirectory(relative,uri)}
        }
        // The native receiver removes its temporary signature after export.
        // A user file with that name must still be published.
        source.walkTopDown().filter{it.isFile}.forEach{file->
            check(!stopped()){ "Paused" }
            val relative=file.relativeTo(source).invariantSeparatorsPath
            val known=published.optString(relative)
            if(known.isNotEmpty() && runCatching{c.contentResolver.openInputStream(Uri.parse(known))?.use{true} ?: false}.getOrDefault(false))return@forEach
            if(folder.isNotEmpty()) {
                val uri=publishTree(c,file,relative,Uri.parse(folder),stopped).toString()
                published.put(relative,uri);record(relative,uri)
            } else if(android.os.Build.VERSION.SDK_INT>=29) {
                val parent=relative.substringBeforeLast('/',"")
                val values=android.content.ContentValues().apply{
                    put(android.provider.MediaStore.Downloads.DISPLAY_NAME,file.name)
                    put(android.provider.MediaStore.Downloads.MIME_TYPE,java.net.URLConnection.guessContentTypeFromName(file.name) ?: "application/octet-stream")
                    put(android.provider.MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeam/"+parent)
                    put(android.provider.MediaStore.Downloads.IS_PENDING,1)
                }
                val uri=c.contentResolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,values) ?: error("Cannot save to Downloads")
                try {
                    file.inputStream().use{input->c.contentResolver.openOutputStream(uri,"w")!!.use{out->
                        val buffer=ByteArray(256*1024)
                        while(true){check(!stopped()){ "Paused" };val n=input.read(buffer);if(n<0)break;out.write(buffer,0,n)}
                    }}
                    c.contentResolver.update(uri,android.content.ContentValues().apply{put(android.provider.MediaStore.Downloads.IS_PENDING,0)},null,null)
                    published.put(relative,uri.toString());record(relative,uri.toString())
                }catch(e:Throwable){c.contentResolver.delete(uri,null,null);throw e}
            } else {
                val folder=File(android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS),"DataBeam/"+relative.substringBeforeLast('/',""));check(folder.exists() || folder.mkdirs())
                var dest=File(folder,file.name);var suffix=1
                while(dest.exists()){dest=File(folder,"${suffix++}_${file.name}")}
                file.inputStream().use{input->dest.outputStream().use{input.copyTo(it)}}
                android.media.MediaScannerConnection.scanFile(c,arrayOf(dest.absolutePath),null,null)
                val uri=Uri.fromFile(dest).toString();published.put(relative,uri);record(relative,uri)
            }
        }
    }
}
