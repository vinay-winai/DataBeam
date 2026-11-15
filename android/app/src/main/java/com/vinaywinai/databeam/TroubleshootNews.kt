package com.vinaywinai.databeam

import android.content.Context
import android.os.SystemClock
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import java.util.concurrent.atomic.AtomicBoolean

/** A bounded background guide check. Offline failures retain the last unread badge. */
object TroubleshootNews {
    const val GUIDE_URL="https://github.com/vinay-winai/DataBeam/blob/main/troubleshoot.md"
    private const val RAW_URL="https://raw.githubusercontent.com/vinay-winai/DataBeam/main/troubleshoot.md"
    private val checking=AtomicBoolean(false)
    @Volatile private var nextCheck=0L
    private var bundled:String?=null
    private fun prefs(c:Context)=c.getSharedPreferences("troubleshoot-news",0)
    @Synchronized private fun load(c:Context):GuideNews {
        val baseline=bundled ?: GuideNews.revision(c.assets.open("troubleshoot.md").bufferedReader().use{it.readText()}).also{bundled=it}
        return GuideNews(prefs(c).getString("seen",baseline)!!,prefs(c).getString("latest",null))
    }
    fun hasUnread(c:Context)=load(c).hasUnread()
    @Synchronized fun markRead(c:Context){val news=load(c);news.markRead();prefs(c).edit().putString("seen",news.seen).commit()}
    @Synchronized fun observe(c:Context,revision:String){prefs(c).edit().putString("latest",revision).commit()}
    fun check(c:Context,changed:()->Unit){
        if(SystemClock.elapsedRealtime()<nextCheck || !checking.compareAndSet(false,true))return
        Thread{
            try {
                val revision=fetch()
                observe(c,revision);nextCheck=SystemClock.elapsedRealtime()+30*60*1000L;changed()
            }catch(_:Exception){nextCheck=SystemClock.elapsedRealtime()+5*60*1000L}
            finally{checking.set(false)}
        }.apply{name="DataBeam guide news";start()}
    }
    private fun fetch():String {
        val start=SystemClock.elapsedRealtime()
        val connection=URL(RAW_URL).openConnection() as HttpsURLConnection
        try {
            connection.connectTimeout=5000;connection.readTimeout=10000;connection.instanceFollowRedirects=false
            connection.setRequestProperty("User-Agent","DataBeam-Android")
            check(connection.responseCode==200){"Guide unavailable"}
            val bytes=java.io.ByteArrayOutputStream()
            connection.inputStream.use{input->val buffer=ByteArray(16384);while(true){
                val remaining=15000-(SystemClock.elapsedRealtime()-start);check(remaining>0){"Guide check timed out"}
                connection.readTimeout=minOf(10000,remaining.toInt())
                val n=input.read(buffer);if(n<0)break
                check(bytes.size()+n<=1024*1024){"Guide exceeds size limit"};bytes.write(buffer,0,n)
            }}
            val text=Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes.toByteArray())).toString()
            check(text.isNotBlank()){"Empty guide"};return GuideNews.revision(text)
        }finally{connection.disconnect()}
    }
}
