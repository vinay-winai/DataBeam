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
    private val random=SecureRandom()
    @Volatile private var words=emptyList<String>()
    fun initializeWordList(text:String) {
        val parsed=text.lineSequence().map{it.trim()}.filter{it.isNotEmpty()}.toList()
        require(parsed.size==1296 && parsed.distinct().size==1296){"Invalid Croc word list"}
        words=parsed
    }
    fun newCode():String {
        val dictionary=words
        check(dictionary.isNotEmpty()){"Croc word list is not loaded"}
        return List(3){dictionary[random.nextInt(dictionary.size)]}.joinToString("-")
    }
    val relays=(1..4).map{"$it.getcroc.com:9009"}
    fun relay(code:String):String {
        val hash=MessageDigest.getInstance("SHA-256").digest(code.toByteArray(Charsets.UTF_8))
        return relays[(hash.last().toInt() and 255)%4]
    }
    fun transportError(error:String):Boolean {
        val text=error.lowercase()
        if(localChannelFailure(text))return false
        if(listOf("password","passphrase","pake","room ","lookup ","permission denied","decompress","corrupt input").any{text.contains(it)})return false
        return text.contains("relay connection failed") ||
            (text.contains("could not connect to ") &&
            listOf("i/o timeout","timed out","connection refused","connection reset","network is unreachable","no route to host","relay closed","lookup ").any{text.contains(it)})
    }
    fun localChannelFailure(error:String):Boolean {
        if(!error.contains("could not connect transfer ports",ignoreCase=true))return false
        return Regex("dial tcp ([0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+):").findAll(error).any{match->
            val p=match.groupValues[1].split('.').mapNotNull{it.toIntOrNull()}
            p.size==4 && p.all{it in 0..255} && (p[0]==10 || p[0]==127 || (p[0]==192 && p[1]==168) || (p[0]==172 && p[1] in 16..31) || (p[0]==169 && p[1]==254) || (p[0]==100 && p[1] in 64..127))
        } || Regex("dial tcp \\[(?:fe[89ab]|f[cd]|::1)[^]]*]",RegexOption.IGNORE_CASE).containsMatchIn(error)
    }
    fun failureMessage(output:String,exit:Int):String {
        val ansi=Regex("\u001B\\[[0-?]*[ -/]*[@-~]")
        val error=output.lineSequence().map{it.replace(ansi,"").trim()}.filter{it.isNotEmpty()}.toList().asReversed().firstOrNull{line->
            val lower=line.lowercase()
            !lower.startsWith("databeam_") && listOf("could not", "relay connection failed", "error:", "failed:", "password", "passphrase", "pake", "permission denied", "room not found", "room is full", "connection reset", "connection refused", "timed out", "i/o timeout", "decompress", "corrupt input", "no such file", "no public relay", "sharing different files", "cannot verify this sender", "no saved file list", "verification failed for", "the saved partial transfer was kept").any{lower.contains(it)}
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
        repeat(256){
            val code=newCode()
            if(code!=old && relay(code)==endpoint)return code
        }
        error("Could not generate a replacement code. Retry or use Sendme.")
    }
}
