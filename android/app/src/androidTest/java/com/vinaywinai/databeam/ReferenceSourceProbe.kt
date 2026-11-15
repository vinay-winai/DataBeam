package com.vinaywinai.databeam

import android.app.Instrumentation
import android.content.ContentValues
import android.os.Bundle
import android.os.Process
import android.provider.MediaStore
import android.system.Os
import java.io.File
import java.util.UUID

object ReferenceSourceProbe {
    fun run(runner:Instrumentation){
        val c=runner.targetContext;val id=UUID.randomUUID().toString();val directory=File(c.filesDir,"reference-probe/$id").apply{mkdirs()}
        var uri:android.net.Uri?=null;var descriptor:android.content.res.AssetFileDescriptor?=null;var code=1;val result=Bundle()
        try {
            uri=c.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{put(MediaStore.Downloads.DISPLAY_NAME,"reference-$id.bin");put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeamProbe-$id/");put(MediaStore.Downloads.MIME_TYPE,"application/octet-stream")})!!
            val data=ByteArray(1024*1024).also{java.util.Random(9).nextBytes(it)}
            c.contentResolver.openOutputStream(uri)!!.use{it.write(data)}
            descriptor=c.contentResolver.openAssetFileDescriptor(uri,"r")!!
            val fd=descriptor.parcelFileDescriptor.fd
            val alias=File(directory,"original-name.bin");Os.symlink("/proc/${Process.myPid()}/fd/$fd",alias.absolutePath)
            val process=ProcessBuilder("/system/bin/cat",alias.absolutePath).start()
            val received=process.inputStream.readBytes();val errors=process.errorStream.bufferedReader().readText();val exit=process.waitFor()
            check(exit==0){"Descriptor access from child failed: $errors"};check(received.contentEquals(data)){"Child did not read original descriptor bytes"}
            result.putString("result","PASS child reads original provider FD via named alias; no staging copy")
            result.putString("offset",descriptor.startOffset.toString());result.putString("length",descriptor.declaredLength.toString());result.putString("mode",Os.fstat(descriptor.fileDescriptor).st_mode.toString());code=0
        }catch(error:Throwable){result.putString("error",error.stackTraceToString())}
        finally{File(directory,"original-name.bin").delete();descriptor?.close();uri?.let{c.contentResolver.delete(it,null,null)};directory.delete()}
        runner.finish(code,result)
    }
}
