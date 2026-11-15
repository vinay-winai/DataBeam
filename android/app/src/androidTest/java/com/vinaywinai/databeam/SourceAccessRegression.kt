package com.vinaywinai.databeam

import android.app.Instrumentation
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract as D
import java.io.File
import java.nio.file.Files
import java.util.UUID

object SourceAccessRegression {
    const val AUTHORITY="com.vinaywinai.databeam.test.sourcefixture"
    fun run(runner:Instrumentation){
        val c=runner.targetContext;val root=File(c.filesDir,"outgoing/${UUID.randomUUID()}")
        var sources:OutgoingSources?=null;var code=1;val result=Bundle()
        try {
            c.contentResolver.delete(Uri.parse("content://$AUTHORITY/reset"),null,null)
            sources=OutgoingSources(c,root,{false},{})
            val folder=sources.add(D.buildTreeDocumentUri(AUTHORITY,"root"))
            check(sources.referenced==0 && sources.copied==3){"Private provider, pipe and bounded asset must use safe fallbacks: ${sources.referenced}/${sources.copied}"}
            val original=File(folder,"original-name.bin");check(!Files.isSymbolicLink(original.toPath())){"Private inode cannot be reopened by a path-based engine"}
            val expected=ByteArray(65536).also{java.util.Random(7).nextBytes(it)}
            check(original.readBytes().contentEquals(expected)){"Original descriptor bytes mismatch"}
            check(File(folder,"nested/stream.bin").readBytes().contentEquals(expected)){"Pipe fallback mismatch"}
            check(File(folder,"nested/bounded.bin").readBytes().contentEquals(expected)){"Bounded asset copied unrelated bytes"}
            check(File(folder,"empty directory").isDirectory)
            val process=ProcessBuilder("/system/bin/cat",original.absolutePath).start();check(process.inputStream.readBytes().contentEquals(expected));check(process.waitFor()==0)
            sources.close();sources=null
            result.putString("result","PASS private-provider, bounded/pipe fallbacks, folder names/empty directories and exact child-process bytes");code=0
        }catch(error:Throwable){result.putString("error",error.stackTraceToString())}
        finally{sources?.close();TransferCleanup.remove(c.filesDir,"outgoing/${root.name}")}
        runner.finish(code,result)
    }
}
