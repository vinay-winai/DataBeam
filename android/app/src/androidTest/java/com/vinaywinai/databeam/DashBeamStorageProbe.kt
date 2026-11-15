package com.vinaywinai.databeam

import android.app.Instrumentation
import android.app.usage.StorageStatsManager
import android.content.ContentValues
import android.content.Intent
import android.os.Bundle
import android.os.Process
import android.os.storage.StorageManager
import android.provider.MediaStore
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.UUID

/** Measures the installed third-party app through Android's live storage API. */
object DashBeamStorageProbe {
    fun run(runner:Instrumentation,args:JSONObject){
        val c=runner.targetContext;val id=args.getString("id");UUID.fromString(id)
        val directory=File(c.filesDir,"dashbeam-probe/$id").apply{mkdirs()}
        val samples=File(directory,"samples.jsonl");val status=File(directory,"status.json")
        var uri:android.net.Uri?=null;var code=1;val result=Bundle()
        try {
            runner.uiAutomation.adoptShellPermissionIdentity("android.permission.PACKAGE_USAGE_STATS","android.permission.QUERY_ALL_PACKAGES")
            val manager=c.getSystemService(StorageStatsManager::class.java)
            fun sample(label:String){
                val stats=manager.queryStatsForUid(StorageManager.UUID_DEFAULT,args.getInt("uid"))
                val value=JSONObject().put("at",System.currentTimeMillis()).put("label",label).put("appBytes",stats.appBytes).put("dataBytes",stats.dataBytes).put("cacheBytes",stats.cacheBytes)
                samples.appendText(value.toString()+"\n")
            }
            sample("baseline")
            status.writeText(JSONObject().put("phase","ready").toString())
            var label="baseline";var sequence=-1
            val end=System.currentTimeMillis()+10*60*1000
            while(System.currentTimeMillis()<end){
                val command=runCatching{JSONObject(File(directory,"command.json").readText())}.getOrNull()
                if(command!=null && command.optInt("sequence")!=sequence){
                    sequence=command.getInt("sequence")
                    when(command.getString("op")){
                        "label"->label=command.getString("label")
                        "share"->{
                            sample("before-source")
                            val name="dashbeam-storage-$id.bin"
                            uri=c.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{
                                put(MediaStore.Downloads.DISPLAY_NAME,name);put(MediaStore.Downloads.MIME_TYPE,"application/octet-stream");put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeamProbe-$id/")
                            }) ?: error("Cannot create generated fixture")
                            val digest=MessageDigest.getInstance("SHA-256");val block=ByteArray(1024*1024).also{java.util.Random(id.hashCode().toLong()).nextBytes(it)}
                            c.contentResolver.openOutputStream(uri!!)!!.use{out->repeat(args.optInt("mib",64)){out.write(block);digest.update(block)}}
                            status.writeText(JSONObject().put("phase","shared").put("uri",uri.toString()).put("name",name).put("sha256",digest.digest().joinToString(""){"%02x".format(it)}).toString())
                            runner.uiAutomation.dropShellPermissionIdentity()
                            c.startActivity(Intent(Intent.ACTION_SEND).setPackage("com.dashbeam.android").setType("application/octet-stream").putExtra(Intent.EXTRA_STREAM,uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION))
                            runner.uiAutomation.adoptShellPermissionIdentity("android.permission.PACKAGE_USAGE_STATS","android.permission.QUERY_ALL_PACKAGES")
                            label="selected"
                        }
                        "finish"->{sample("final");code=0;break}
                    }
                }
                sample(label);Thread.sleep(750)
            }
            check(code==0){"Storage experiment timed out"};result.putString("result","PASS storage monitoring")
        }catch(error:Throwable){status.writeText(JSONObject().put("phase","failed").put("error",error.stackTraceToString()).toString());result.putString("error",error.stackTraceToString())}
        finally{runner.uiAutomation.dropShellPermissionIdentity();uri?.let{runCatching{c.contentResolver.delete(it,null,null)}}}
        runner.finish(code,result)
    }
}
