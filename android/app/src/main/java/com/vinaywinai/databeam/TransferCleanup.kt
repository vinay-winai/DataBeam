package com.vinaywinai.databeam

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.file.Files
import java.nio.file.SimpleFileVisitor
import java.nio.file.FileVisitResult
import java.nio.file.attribute.BasicFileAttributes
import java.security.MessageDigest
import java.util.concurrent.locks.ReentrantLock

/** Private working storage only. Public documents are verification inputs, never delete targets. */
object TransferCleanup {
    val lock=ReentrantLock()
    const val RETENTION_MS=7L*24*60*60*1000
    data class Candidate(val relative:String,val bytes:Long,val kind:String)
    data class Report(val reclaimed:Long,val candidates:List<Candidate>,val protectedBytes:Long)
    private val validId=Regex("[A-Za-z0-9_-]{1,100}")
    private val namespaces=setOf("received","outgoing","blobs")
    fun target(root:File,relative:String):File {
        val parts=relative.split('/')
        require(parts.size==2 && parts[0] in namespaces && validId.matches(parts[1])){"Invalid private transfer directory"}
        val base=root.canonicalFile;val parent=File(base,parts[0])
        require(parent.canonicalFile==parent){"Unsafe transfer storage location"}
        val result=File(parent,parts[1]);require(result.canonicalFile==result){"Unsafe transfer directory"};return result
    }
    fun size(file:File):Long {
        if(Files.isSymbolicLink(file.toPath()))return 0
        return if(file.isFile)runCatching{android.system.Os.lstat(file.absolutePath).st_blocks*512}.getOrDefault(file.length()) else file.listFiles()?.sumOf{size(it)} ?: 0
    }
    fun remove(root:File,relative:String):Long {
        val file=target(root,relative);if(!file.exists())return 0
        val bytes=size(file)
        Files.walkFileTree(file.toPath(),object:SimpleFileVisitor<java.nio.file.Path>(){
            override fun visitFile(path:java.nio.file.Path,attributes:BasicFileAttributes):FileVisitResult {Files.delete(path);return FileVisitResult.CONTINUE}
            override fun postVisitDirectory(path:java.nio.file.Path,error:java.io.IOException?):FileVisitResult {if(error!=null)throw error;Files.delete(path);return FileVisitResult.CONTINUE}
        })
        return bytes
    }
    private fun catalog(c:Context)=File(c.filesDir,"transfer-records").apply{mkdirs()}
    fun record(c:Context,state:JSONObject,status:String=state.optString("status")) {
        val id=state.optString("id");if(!validId.matches(id))return
        val value=JSONObject().put("id",id).put("status",status).put("updatedAt",System.currentTimeMillis())
        for(key in listOf("action","mode","code","ticket","paths","output","cacheBase","saveFolder","published","publishedDirectories","workingCleaned","referencedSources"))if(state.has(key))value.put(key,state.get(key))
        val file=File(catalog(c),"$id.json");val temp=File(file.parentFile,"$id.tmp")
        synchronized(this){temp.writeText(value.toString());check(temp.renameTo(file)){"Cannot record transfer cleanup state"}}
    }
    private fun records(c:Context):Map<String,JSONObject> = catalog(c).listFiles()?.filter{it.extension=="json"}?.mapNotNull{file->runCatching{JSONObject(file.readText())}.getOrNull()?.let{it.optString("id") to it}}?.toMap() ?: emptyMap()
    private fun protected(c:Context,extra:List<String>):Set<String> {
        val current=TransferService.state(c);val result=mutableSetOf<String>();val id=current.optString("id")
        fun keep(path:String){
            val root=c.filesDir.canonicalFile.toPath();val file=File(path).absoluteFile;val candidate=File(file.parentFile.canonicalFile,file.name).toPath().normalize()
            if(candidate.startsWith(root)){
                val relative=root.relativize(candidate).toString().replace('\\','/').split('/')
                if(relative.size>=2 && relative[0] in namespaces && validId.matches(relative[1]))result.add(relative.take(2).joinToString("/"))
            }
        }
        if(validId.matches(id)){
            if(current.optString("action")=="receive" && current.optString("status")!="complete")result.add("received/$id")
            result.add("blobs/$id")
            if(current.optString("action")=="send")result.add("outgoing/$id")
        }
        current.optJSONArray("paths")?.let{a->for(i in 0 until a.length())keep(a.getString(i))}
        current.optString("cacheBase").takeIf{it.isNotEmpty()}?.let{keep(it)}
        extra.forEach{keep(it)}
        val hash=NativeBridge.hash(current.optString("ticket"))
        if(hash.isNotEmpty() && current.optString("status")!="complete")File(c.filesDir,"blobs").listFiles()?.filter{File(it,".sendme-recv-$hash").exists()}?.forEach{result.add("blobs/${it.name}")}
        return result
    }
    private fun hash(input:java.io.InputStream,check:()->Unit={}):Pair<Long,ByteArray> {
        val digest=MessageDigest.getInstance("SHA-256");var bytes=0L
        input.use{stream->val buffer=ByteArray(256*1024);while(true){check();val n=stream.read(buffer);if(n<0)break;digest.update(buffer,0,n);bytes+=n}}
        return bytes to digest.digest()
    }
    fun matches(c:Context,file:File,uri:Uri,check:()->Unit={}):Boolean = runCatching {
        if(uri.scheme=="file" && File(uri.path!!).canonicalFile.toPath().startsWith(File(c.applicationInfo.dataDir).canonicalFile.toPath()))return@runCatching false
        if(Files.isSymbolicLink(file.toPath()) || !file.isFile)return@runCatching false
        val original=hash(file.inputStream(),check);val saved=c.contentResolver.openInputStream(uri)?.let{hash(it,check)} ?: return@runCatching false
        original.first==saved.first && original.second.contentEquals(saved.second)
    }.getOrDefault(false)
    private fun verify(c:Context,directory:File,published:JSONObject,dirs:JSONObject,check:()->Unit):Boolean {
        if(!directory.isDirectory)return false
        for(file in directory.walkTopDown()){
            check();if(Files.isSymbolicLink(file.toPath()))return false
            if(file.isFile){val relative=file.relativeTo(directory).invariantSeparatorsPath;val uri=published.optString(relative);if(uri.isEmpty() || !matches(c,file,Uri.parse(uri),check))return false}
            else if(file!=directory && file.list()?.isEmpty()==true){
                val relative=file.relativeTo(directory).invariantSeparatorsPath;val uri=dirs.optString(relative)
                if(uri.isEmpty())return false
                val exists=if(uri.startsWith("file:"))File(Uri.parse(uri).path!!).isDirectory else runCatching{c.contentResolver.query(Uri.parse(uri),arrayOf(android.provider.DocumentsContract.Document.COLUMN_DOCUMENT_ID),null,null,null)?.use{it.moveToFirst()} ?: false}.getOrDefault(false)
                if(!exists)return false
            }
        }
        return true
    }
    fun saved(c:Context,state:JSONObject):Long {
        val id=state.getString("id");val directory=target(c.filesDir,"received/$id")
        require(verify(c,directory,state.optJSONObject("published") ?: JSONObject(),state.optJSONObject("publishedDirectories") ?: JSONObject(),{})){"Saved files could not be verified. Working copies were kept; resume to save them again."}
        record(c,state,"complete") // Durable publication receipts precede deletion.
        val reclaimed=remove(c.filesDir,"received/$id");state.put("workingCleaned",true);record(c,state,"complete");return reclaimed
    }
    private fun migratePublished(c:Context,directory:File,check:()->Unit):JSONObject? {
        if(android.os.Build.VERSION.SDK_INT<29)return null
        val published=JSONObject();val dirs=JSONObject()
        for(file in directory.walkTopDown()){
            check();if(Files.isSymbolicLink(file.toPath()))return null
            val relative=file.relativeTo(directory).invariantSeparatorsPath
            if(file.isFile){
                val parent="Download/DataBeam/"+relative.substringBeforeLast('/',"").let{if(it.isEmpty())"" else "$it/"}
                val candidates=mutableListOf<Uri>()
                c.contentResolver.query(MediaStore.Downloads.EXTERNAL_CONTENT_URI,arrayOf(MediaStore.Downloads._ID),"${MediaStore.Downloads.RELATIVE_PATH}=? AND ${MediaStore.Downloads.DISPLAY_NAME}=? AND ${MediaStore.Downloads.SIZE}=?",arrayOf(parent,file.name,file.length().toString()),null)?.use{cursor->while(cursor.moveToNext())candidates.add(android.content.ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI,cursor.getLong(0)))}
                val found=candidates.firstOrNull{matches(c,file,it,check)} ?: return null;published.put(relative,found.toString())
            } else if(file!=directory && file.list()?.isEmpty()==true){
                val destination=File(android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS),"DataBeam/$relative")
                if(!destination.isDirectory)return null;dirs.put(relative,Uri.fromFile(destination).toString())
            }
        }
        return JSONObject().put("id",directory.name).put("action","receive").put("output",directory.absolutePath).put("published",published).put("publishedDirectories",dirs)
    }
    /** Run while idle. Unknown legacy caches are offered for deletion, never silently aged out. */
    fun scan(c:Context,extra:List<String> = emptyList(),migrate:Boolean=true):Report {
        if(!lock.tryLock())return Report(0,emptyList(),0)
        try {
            fun idle(){check(!TransferService.isBusy()){"Finish or pause the transfer before cleaning storage"}}
            idle();var reclaimed=0L;var kept=0L;val found=mutableListOf<Candidate>();val known=records(c);val now=System.currentTimeMillis()
            for(kind in namespaces)for(directory in File(c.filesDir,kind).listFiles().orEmpty()){
                idle();if(!validId.matches(directory.name))continue
                val relative="$kind/${directory.name}";val checked=runCatching{target(c.filesDir,relative)}.getOrNull() ?: continue
                if(!checked.isDirectory)continue
                val protected=protected(c,extra)
                if(relative in protected){kept+=size(checked);continue}
                var entry=known[directory.name]
                if(kind=="received" && migrate){
                    if(entry==null)entry=migratePublished(c,checked,::idle)
                    if(entry!=null && entry.optString("action")=="receive" && (entry.optString("status")=="complete" || entry.has("published")) && verify(c,checked,entry.optJSONObject("published") ?: JSONObject(),entry.optJSONObject("publishedDirectories") ?: JSONObject(),::idle)){
                        idle();reclaimed+=saved(c,entry);continue
                    }
                }
                val bytes=size(checked)
                val references=known.values.filter{it.optString("id")==directory.name || (it.optString("cacheBase").isNotEmpty() && File(it.optString("cacheBase")).canonicalFile==checked)}
                val last=references.maxOfOrNull{it.optLong("updatedAt")} ?: now
                // Zero allocated bytes can still mean an unsaved zero-byte file,
                // sparse file, empty folder tree or stale descriptor alias.
                if(checked.list()?.isEmpty()==true || (references.isNotEmpty() && now-last>RETENTION_MS)){
                    idle();reclaimed+=remove(c.filesDir,relative)
                }else found.add(Candidate(relative,bytes,kind))
            }
            return Report(reclaimed,found,kept)
        } finally{lock.unlock()}
    }
    fun deleteUnused(c:Context,selected:List<Candidate>,extra:List<String> = emptyList()):Long {
        lock.lock()
        try {
            check(!TransferService.isBusy()){"Finish or pause the transfer before cleaning storage"}
            val protected=protected(c,extra);var bytes=0L
            for(item in selected){check(!TransferService.isBusy()){"Transfer started; cleanup stopped"};if(item.relative !in protected)bytes+=remove(c.filesDir,item.relative)}
            return bytes
        }finally{lock.unlock()}
    }
}
