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
    const val DAY_MS=24L*60*60*1000
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
        if(status=="complete")value.put("completedAt",state.optLong("completedAt",System.currentTimeMillis()))
        for(key in listOf("action","mode","code","receiveCode","ticket","paths","output","cacheBase","saveFolder","saveFolderLabel","relay","route","published","publishedDirectories","workingCleaned","referencedSources","done","total","message","payloadNames","timing","crocWrapper","crocReadyToSave","crocDirect","crocStaged","crocPending","crocMoveTargets","crocMoved","unwrapCroc","crocUnwrapPending"))if(state.has(key))value.put(key,state.get(key))
        if(status=="complete" && state.optBoolean("workingCleaned")){
            value.remove("code")
            value.remove("receiveCode")
            value.remove("ticket")
            value.remove("crocPending")
            value.remove("crocReadyToSave")
        }
        val file=File(catalog(c),"$id.json");val temp=File(file.parentFile,"$id.tmp")
        synchronized(this){temp.writeText(value.toString());check(temp.renameTo(file)){"Cannot record transfer cleanup state"}}
    }

    fun clearAgedTransfers(c:Context):Long {
        val days=AppSettings.cleanupDays(c)
        if(days==0 || TransferService.isBusy() || !lock.tryLock())return 0L
        try {
            if(TransferService.isBusy())return 0L
            val cutoff=System.currentTimeMillis()-days*DAY_MS
            val saved=records(c)
            val current=TransferService.state(c)
            val currentId=current.optString("id")
            val expired=saved.filter { (id,entry)->
                val age=entry.optLong("completedAt",entry.optLong("updatedAt"))
                val receiving=entry.optString("action")=="receive"
                val unsavedExport=receiving && !entry.optBoolean("workingCleaned") &&
                    (entry.optBoolean("crocReadyToSave") || entry.has("published") || entry.optString("status")=="complete")
                validId.matches(id) && age>0L && age<=cutoff && !unsavedExport
            }
            if(expired.isEmpty())return 0L
            val blobRoot=File(c.filesDir.canonicalFile,"blobs")
            fun blobs(entry:JSONObject):Set<String> = buildSet {
                entry.optString("id").takeIf(validId::matches)?.let(::add)
                entry.optString("cacheBase").takeIf(String::isNotBlank)?.let { path->
                    val candidate=File(path).absoluteFile
                    if(candidate.parentFile==blobRoot && validId.matches(candidate.name))add(candidate.name)
                }
            }
            val keptBlobs=saved.filterKeys{it !in expired}.values.flatMapTo(mutableSetOf(),::blobs)
            if(currentId !in expired)keptBlobs.addAll(blobs(current))
            var reclaimed=0L
            for((id,entry) in expired){
                if(TransferService.isBusy())break
                val result=runCatching {
                    if(entry.optString("mode")=="Croc" && entry.optString("action")=="receive"){
                        if(entry.optString("status")!="complete" && entry.optBoolean("crocDirect") && entry.optString("crocWrapper").isNotEmpty()){
                            CrocDownloads.discard(c,entry.getString("crocWrapper"),entry.optJSONObject("crocPending") ?: JSONObject())
                        }
                        CrocBatchReview.clear(c,id)
                    }
                    var removed=remove(c.filesDir,"received/$id")+remove(c.filesDir,"outgoing/$id")
                    for(blob in blobs(entry)-keptBlobs)removed+=remove(c.filesDir,"blobs/$blob")
                    val receipt=File(catalog(c),"$id.json")
                    check(!receipt.exists() || receipt.delete()){"Could not clear transfer history"}
                    if(id==currentId)c.getSharedPreferences("transfer",0).edit().remove("state").commit()
                    removed
                }
                reclaimed+=result.getOrDefault(0L)
            }
            return reclaimed
        }finally{lock.unlock()}
    }
    private fun records(c:Context):Map<String,JSONObject> = catalog(c).listFiles()?.filter{it.extension=="json"}?.mapNotNull{file->runCatching{JSONObject(file.readText())}.getOrNull()?.let{it.optString("id") to it}}?.toMap() ?: emptyMap()
    fun crocReceiveHistory(c:Context):List<JSONObject> {
        val current=TransferService.state(c)
        val entries=records(c).toMutableMap()
        val currentId=current.optString("id")
        if(current.optString("mode")=="Croc" && current.optString("action")=="receive" && validId.matches(currentId) &&
            entries[currentId]?.optString("status")!="complete"){
            entries[currentId]=JSONObject(current.toString()).put("updatedAt",System.currentTimeMillis())
        }
        return entries.values.filter{
            it.optString("mode")=="Croc" && it.optString("action")=="receive" &&
                (it.optString("status")!="complete" || it.optBoolean("workingCleaned"))
        }.sortedByDescending{it.optLong("updatedAt")}.map {
            if(it.optString("id")!=currentId && it.optString("status") in listOf("running","stopping")){
                it.put("status","interrupted").put("connectionRetained",false)
                it.put("message",TransferLabels.interruptedMessage(it))
            }
            it
        }
    }

    fun discardCrocReceive(c:Context,id:String) {
        lock.lock()
        try {
            check(!TransferService.isBusy()){"Finish the active transfer first"}
            require(validId.matches(id)){"Invalid transfer"}
            val entry=crocReceiveHistory(c).firstOrNull{it.optString("id")==id} ?: error("Transfer no longer available")
            if(entry.optString("status")!="complete"){
                if(entry.optBoolean("crocDirect") && entry.optString("crocWrapper").isNotEmpty())
                    CrocDownloads.discard(c,entry.getString("crocWrapper"),entry.optJSONObject("crocPending") ?: JSONObject())
                remove(c.filesDir,"received/$id")
                remove(c.filesDir,"blobs/$id")
                CrocBatchReview.clear(c,id)
            }
            File(catalog(c),"$id.json").delete()
            if(TransferService.state(c).optString("id")==id)c.getSharedPreferences("transfer",0).edit().remove("state").commit()
        }finally{lock.unlock()}
    }

    fun moveCrocReceive(c:Context,entry:JSONObject){
        lock.lock()
        try{
            check(!TransferService.isBusy() && entry.optString("status")=="complete"){"Finish the active transfer first"}
            val targets=entry.optJSONObject("crocMoveTargets") ?: JSONObject().also{entry.put("crocMoveTargets",it)}
            CrocDownloads.moveOut(c,entry.getString("crocWrapper"),entry.optJSONObject("published") ?: JSONObject(),
                entry.optJSONObject("publishedDirectories") ?: JSONObject(),targets){record(c,entry)}
            entry.put("crocMoved",true)
            record(c,entry)
        }finally{lock.unlock()}
    }
    fun clearInactiveSenders(c:Context):Long {
        if(TransferService.isBusy() || !lock.tryLock())return 0
        try{
            if(TransferService.isBusy())return 0
            var bytes=0L
            for(directory in File(c.filesDir,"outgoing").listFiles().orEmpty()){
                if(TransferService.isBusy())return bytes
                if(validId.matches(directory.name) && runCatching{target(c.filesDir,"outgoing/${directory.name}")}.isSuccess)bytes+=remove(c.filesDir,"outgoing/${directory.name}")
            }
            for(entry in records(c).values.filter{it.optString("action")=="send"}){
                if(TransferService.isBusy())return bytes
                val id=entry.optString("id");if(validId.matches(id)){
                    bytes+=remove(c.filesDir,"blobs/$id")
                    bytes+=remove(c.filesDir,"received/$id") // Croc's sender-side temporary text/output.
                }
            }
            return bytes
        }finally{lock.unlock()}
    }
    private fun protected(c:Context,extra:List<String>):Set<String> {
        val current=TransferService.state(c);val result=mutableSetOf<String>();val id=current.optString("id")
        fun keep(path:String){
            val root=c.filesDir.canonicalFile.toPath();val file=File(path).absoluteFile;val parent=file.parentFile ?: return
            val candidate=File(parent.canonicalFile,file.name).toPath().normalize()
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
        for(entry in records(c).values){
            val savedId=entry.optString("id")
            if(validId.matches(savedId) && entry.optString("mode")=="Croc" && entry.optString("action")=="receive" && !entry.optBoolean("workingCleaned")){
                result.add("received/$savedId")
            }
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
        var reclaimed=remove(c.filesDir,"received/$id")
        if(state.optString("mode")=="Croc")CrocBatchReview.clear(c,id)
        if(state.optString("mode") in listOf("Sendme","EasySendMe") && NativeBridge.hash(state.optString("ticket")).isNotEmpty())ReceiveCache.delete(File(c.filesDir,"blobs"),state.getString("ticket"))
        reclaimed+=remove(c.filesDir,"blobs/$id")
        state.put("workingCleaned",true);record(c,state,"complete");return reclaimed
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
            idle();var reclaimed=clearAgedTransfers(c);var kept=0L;val found=mutableListOf<Candidate>();val known=records(c)
            // File senders now require fresh selection, so no inactive private
            // sender snapshot is reusable. Never prompt to keep these copies.
            reclaimed+=clearInactiveSenders(c)
            for(entry in known.values.filter{it.optString("action")=="receive" && it.optString("status")=="complete" && it.optBoolean("workingCleaned")}){
                val id=entry.optString("id");if(validId.matches(id)){idle();reclaimed+=remove(c.filesDir,"blobs/$id")}
            }
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
                // Zero allocated bytes can still mean an unsaved zero-byte file,
                // sparse file, empty folder tree or stale descriptor alias.
                if(checked.list()?.isEmpty()==true){
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
