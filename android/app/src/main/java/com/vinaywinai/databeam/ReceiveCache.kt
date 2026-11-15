package com.vinaywinai.databeam

import org.json.JSONObject
import java.io.File

/** Every operation is restricted to the exact content hash inside private receive stores. */
object ReceiveCache {
    private fun stores(root:File,ticket:String):List<File> {
        val hash=NativeBridge.hash(ticket)
        require(hash.matches(Regex("[0-9a-f]{64}"))){"Invalid Sendme ticket"}
        val canonical=root.canonicalFile
        val bases=listOf(canonical)+(canonical.listFiles()?.filter{it.isDirectory && it.canonicalFile.parentFile==canonical} ?: emptyList())
        return bases.map{File(it,".sendme-recv-$hash")}.filter{it.isDirectory && it.canonicalFile==File(it.parentFile.canonicalFile,it.name)}
    }
    fun inspect(root:File,ticket:String):JSONObject? = stores(root,ticket).mapNotNull{store->
        val raw=NativeBridge.inspectCache(ticket,store.parentFile!!.absolutePath)
        if(raw=="null")null else JSONObject(raw).also{
            require(!it.has("error")){it.optString("error")}
            it.put("base",store.parentFile!!.absolutePath)
        }
    }.filter{it.optBoolean("complete") || it.optLong("bytes")>0}.maxWithOrNull(compareBy<JSONObject>{it.optBoolean("complete")}.thenBy{it.optLong("bytes")})

    fun delete(root:File,ticket:String) {
        for(store in stores(root,ticket))check(store.deleteRecursively()){"Could not delete saved transfer data. Try again after closing the transfer."}
    }
}
