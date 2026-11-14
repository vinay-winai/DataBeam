package com.vinaywinai.databeam

import java.net.InetSocketAddress
import java.net.Socket
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** A TCP probe reports local reachability, not a global relay outage. */
object RelayRecovery {
    val relays=(1..4).map{"$it.getcroc.com:9009"}
    fun relay(code:String):String {
        val hash=MessageDigest.getInstance("SHA-256").digest(code.toByteArray(Charsets.UTF_8))
        return relays[(hash.last().toInt() and 255)%4]
    }
    fun transportError(error:String):Boolean {
        val text=error.lowercase()
        return !text.contains("lookup ") && !text.contains("room ") &&
            (text.contains("could not connect to ") || text.contains("relay connection failed")) &&
            listOf("i/o timeout","connection refused","connection reset","network is unreachable","no route to host","relay closed").any{text.contains(it)}
    }
    fun failureMessage(output:String,exit:Int):String {
        val ansi=Regex("\u001B\\[[0-?]*[ -/]*[@-~]")
        val error=output.lineSequence().map{it.replace(ansi,"").trim()}.filter{it.isNotEmpty()}.toList().asReversed().firstOrNull{line->
            val lower=line.lowercase()
            !lower.startsWith("databeam_") && listOf("could not", "relay connection failed", "error:", "failed:", "password", "passphrase", "pake", "permission denied", "room not found", "room is full", "connection reset", "connection refused", "timed out", "i/o timeout", "decompress", "corrupt input", "no such file", "no public relay").any{lower.contains(it)}
        }
        return error?.take(600) ?: "Croc could not complete the transfer (exit $exit). Check the code, network connection, and that the sender is still sharing."
    }
    fun healthy(stopped:()->Boolean, connect:(String)->Boolean = {endpoint->
        Socket().use{socket->socket.connect(InetSocketAddress(endpoint.substringBefore(':'),9009),3000)}
        true
    }):List<String> {
        val pool=Executors.newFixedThreadPool(4)
        try {
            val tasks=relays.map{endpoint->pool.submit(Callable {
                runCatching {
                    if(connect(endpoint))endpoint else null
                }.getOrNull()
            })}
            val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(5)
            while(!stopped() && System.nanoTime()<deadline && tasks.any{!it.isDone})Thread.sleep(50)
            return tasks.filter{it.isDone && !it.isCancelled}.mapNotNull{runCatching{it.get()}.getOrNull()}.sorted()
        } finally {pool.shutdownNow()}
    }
    fun replacement(old:String,endpoint:String):String {
        val random=SecureRandom()
        repeat(256){
            val bytes=ByteArray(10);random.nextBytes(bytes)
            val code=bytes.joinToString(""){"%02x".format(it)}
            if(code!=old && relay(code)==endpoint)return code
        }
        error("Could not generate a replacement code. Retry or use Sendme.")
    }
}
