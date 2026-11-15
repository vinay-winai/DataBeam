package com.vinaywinai.databeam

import android.app.Instrumentation
import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import android.os.Bundle
import android.system.Os
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.file.Files
import java.util.UUID

object CleanupRegression {
    fun run(runner:Instrumentation){
        val c=runner.targetContext;val nonce=UUID.randomUUID().toString();val root=File(c.filesDir,"cleanup-fixture-$nonce").apply{mkdirs()}
        val fake=object:ContextWrapper(c){
            override fun getFilesDir()=root
            override fun getSharedPreferences(name:String,mode:Int)=c.getSharedPreferences("cleanup-$nonce-$name",mode)
        }
        val uris=mutableSetOf<String>();var code=1;val result=Bundle()
        fun directory(kind:String,id:String)=File(root,"$kind/$id").apply{mkdirs()}
        fun record(id:String,kind:String="receive",status:String="interrupted",paths:JSONArray=JSONArray()):JSONObject {
            val s=JSONObject().put("id",id).put("action",kind).put("status",status).put("paths",paths)
            TransferCleanup.record(fake,s)
            val file=File(root,"transfer-records/$id.json");val saved=JSONObject(file.readText()).put("updatedAt",System.currentTimeMillis()-TransferCleanup.RETENTION_MS-1000);file.writeText(saved.toString());return s
        }
        try {
            check(!TransferService.isBusy()){"A transfer is active"}
            val source=directory("received","saved");val name="cleanup-$nonce.txt";File(source,name).writeText("verified published bytes")
            val published=JSONObject();Storage.publishDownloads(fake,source,published,{false}){_,uri->uris.add(uri)}
            val receipt=JSONObject().put("id","saved").put("action","receive").put("output",source.absolutePath).put("published",published)
            val savedBlob=directory("blobs","saved");File(savedBlob,"data").writeText("temporary blob")
            check(TransferCleanup.saved(fake,receipt)>0);check(!source.exists());check(c.contentResolver.openInputStream(Uri.parse(published.getString(name)))!!.use{String(it.readBytes())}=="verified published bytes")
            check(!savedBlob.exists()){"Exported receive blobs survived verification"}

            val corrupt=directory("received","corrupt");File(corrupt,name).writeText("incoming bytes")
            val original=Uri.parse(published.getString(name));val corruptReceipt=JSONObject().put("id","corrupt").put("action","receive").put("published",published)
            check(runCatching{TransferCleanup.saved(fake,corruptReceipt)}.isFailure);check(corrupt.isDirectory)
            // An existing but different published file is kept, and publication creates a fresh one.
            Storage.publishDownloads(fake,corrupt,published,{false}){_,uri->uris.add(uri)}
            check(Uri.parse(published.getString(name))!=original)
            check(c.contentResolver.openInputStream(original)!!.use{String(it.readBytes())}=="verified published bytes")
            TransferCleanup.saved(fake,corruptReceipt);check(!corrupt.exists())

            val paused=directory("received","paused");File(paused,"partial").writeText("keep partial")
            val pausedBlob=directory("blobs","paused");File(pausedBlob,"data").writeText("keep blobs")
            val pausedState=record("paused").put("status","paused").put("cacheBase",pausedBlob.absolutePath)
            fake.getSharedPreferences("transfer",0).edit().putString("state",pausedState.toString()).commit()
            val expired=directory("blobs","expired");File(expired,"data").writeBytes(ByteArray(8192){3});record("expired")
            val unknown=directory("blobs","unknown");File(unknown,"data").writeBytes(ByteArray(8192){7})
            val zero=directory("received","unknown-zero");File(zero,"zero.bin").writeBytes(byteArrayOf());File(zero,"empty folder").mkdirs()
            val otherName=directory("received","different-name");File(otherName,"not-a-saved-name-$nonce.txt").writeText("verified published bytes")
            val legacy=directory("received","legacy");val legacyName="legacy-$nonce.txt";File(legacy,legacyName).writeText("legacy saved copy")
            Storage.publishDownloads(fake,legacy,JSONObject(),{false}){_,uri->uris.add(uri)}
            val outside=File(c.cacheDir,"outside-$nonce").apply{mkdirs()};val sentinel=File(outside,"keep").apply{writeText("outside stays")}
            Os.symlink(outside.absolutePath,File(root,"received/escape").absolutePath)
            val plan=TransferCleanup.scan(fake)
            check(!expired.exists());check(!legacy.exists());check(paused.isDirectory && pausedBlob.isDirectory)
            check(zero.exists() && File(zero,"zero.bin").isFile){"Unsaved zero-byte files or empty folders were silently deleted"}
            check(otherName.isDirectory){"Matching bytes under an unrelated public filename do not prove this transfer was saved"}
            check(plan.candidates.any{it.relative=="blobs/unknown"});check(plan.candidates.none{it.relative.endsWith("paused") || it.relative.endsWith("escape")})
            TransferCleanup.deleteUnused(fake,plan.candidates)
            check(!unknown.exists());check(sentinel.readText()=="outside stays");check(paused.isDirectory && pausedBlob.isDirectory)
            check(!zero.exists())

            val old=directory("outgoing","old");val oldFile=File(old,"source").apply{writeText("restart source")};record("old","send","stopped")
            val sender=record("new","send","stopped",JSONArray().put(oldFile.absolutePath))
            fake.getSharedPreferences("transfer",0).edit().putString("state",sender.toString()).commit()
            TransferCleanup.scan(fake,migrate=false);check(!old.exists()){"Inactive sender copies were retained for a removed restart action"}
            val failedBlob=directory("blobs","new");File(failedBlob,"data").writeText("stopped sender blob")
            val senderText=directory("received","new");File(senderText,"text").writeText("stopped Croc text")
            val orphan=directory("outgoing","orphan");File(orphan,"data").writeText("orphan sender copy")
            TransferCleanup.scan(fake,migrate=false);check(!orphan.exists() && !failedBlob.exists() && !senderText.exists())
            Files.delete(File(root,"received/escape").toPath());sentinel.delete();outside.delete()
            result.putString("result","PASS export/blob cleanup, corruption/overwrite protection, paused receive protection, automatic inactive sender cleanup and symlink escape rejection");code=0
        }catch(error:Throwable){result.putString("error",error.stackTraceToString())}
        finally{
            uris.forEach{runCatching{c.contentResolver.delete(Uri.parse(it),null,null)}}
            File(root,"received/escape").delete()
            check(root.canonicalFile.parentFile==c.filesDir.canonicalFile)
            root.deleteRecursively();fake.getSharedPreferences("transfer",0).edit().clear().commit()
        }
        runner.finish(code,result)
    }
}
