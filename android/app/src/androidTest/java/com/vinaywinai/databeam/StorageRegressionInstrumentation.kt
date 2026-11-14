package com.vinaywinai.databeam
import android.app.Instrumentation
import android.os.Bundle
import android.provider.MediaStore
import android.content.ContentValues
import android.net.Uri
import java.io.File
import java.util.UUID
import org.json.JSONObject
object StorageRegression {
 fun run(runner:Instrumentation){
  val targetContext=runner.targetContext
  fun finish(code:Int,values:Bundle)=runner.finish(code,values)
  if(android.os.Build.VERSION.SDK_INT<29){finish(0,Bundle());return}
  val created=mutableListOf<Uri>()
  val cleanup=mutableListOf<File>()
  var result=Bundle();var resultCode=0
  try {
   val c=targetContext;val name="conflict-test-${UUID.randomUUID()}.txt"
   val old=c.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{
    put(MediaStore.Downloads.DISPLAY_NAME,name);put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeam/");put(MediaStore.Downloads.MIME_TYPE,"text/plain")
   })!!;created.add(old)
   c.contentResolver.openOutputStream(old)!!.use{it.write("existing file stays intact".toByteArray())}
   val source=File(c.filesDir,"storage-test-${UUID.randomUUID()}").apply{mkdirs()}
   cleanup.add(source)
   File(source,name).writeText("new incoming data")
   File(source,"nested").mkdirs();File(source,"nested/second.txt").writeText("nested file")
   val emptyName="empty-${UUID.randomUUID()}"
   File(source,"$emptyName/deep/empty").mkdirs()
   File(source,"zero.txt").writeBytes(byteArrayOf())
   File(source,"databeam_hash.txt").writeText("user file, not an engine marker")
   val published=JSONObject()
   val directories=JSONObject()
   Storage.publishDownloads(c,source,published,{false},directories=directories){_,value->created.add(Uri.parse(value))}
   check(published.length()==4)
   check(c.contentResolver.openInputStream(Uri.parse(published.getString("databeam_hash.txt")))!!.use{String(it.readBytes())}=="user file, not an engine marker")
   check(directories.length()==1)
   val empty=File(Uri.parse(directories.getString("$emptyName/deep/empty")).path!!)
   check(empty.isDirectory && empty.list()?.isEmpty()==true)
   cleanup.add(empty.parentFile!!.parentFile!!)
   check(c.contentResolver.openInputStream(old)!!.use{String(it.readBytes())}=="existing file stays intact"){"Existing download was overwritten"}
   val incoming=Uri.parse(published.getString(name));check(incoming!=old)
   check(c.contentResolver.openInputStream(incoming)!!.use{String(it.readBytes())}=="new incoming data")
   Storage.publishDownloads(c,source,published,{false}){_,_->error("Repeat publish created a duplicate")}
   val folder=AppSettings.folder(c)
   if(folder.isNotEmpty()){
    val tree=Uri.parse(folder)
    val root=android.provider.DocumentsContract.buildDocumentUriUsingTree(tree,android.provider.DocumentsContract.getTreeDocumentId(tree))
    val sentinel=android.provider.DocumentsContract.createDocument(c.contentResolver,root,"text/plain",name)!!
    created.add(sentinel);c.contentResolver.openOutputStream(sentinel)!!.use{it.write("keep custom folder file".toByteArray())}
    val saved=JSONObject()
    Storage.publishDownloads(c,source,saved,{false},folder){_,uri->created.add(Uri.parse(uri))}
    check(saved.length()==4)
    check(c.contentResolver.openInputStream(sentinel)!!.use{String(it.readBytes())}=="keep custom folder file")
    check(c.contentResolver.openInputStream(Uri.parse(saved.getString(name)))!!.use{String(it.readBytes())}=="new incoming data")
    Storage.publishDownloads(c,source,saved,{false},folder){_,_->error("Repeat custom save created a duplicate")}
   }
   result=Bundle().apply{putString("result","PASS nested automatic save, zero-byte files, empty directories, existing-file protection, idempotent publication; custom folder tested=$folder")}
  } catch(e:Throwable){resultCode=1;result=Bundle().apply{putString("error",e.stackTraceToString())}}
  finally{created.forEach{runCatching{targetContext.contentResolver.delete(it,null,null)}};cleanup.forEach{it.deleteRecursively()}}
  finish(resultCode,result)
 }
}
