package com.vinaywinai.databeam

import org.json.JSONObject
import java.util.Locale

/** Monotonic active time. Only accumulated durations survive process death. */
class TransferTiming(saved:JSONObject?=null) {
    private var elapsed=saved?.optLong("elapsedMs")?.coerceAtLeast(0) ?: 0
    private var payload=saved?.optLong("payloadMs")?.coerceAtLeast(0) ?: 0
    private var bytes=saved?.optLong("bytes")?.coerceAtLeast(0) ?: 0
    private var complete=saved?.optBoolean("complete") ?: false
    private var completedElapsed=if(complete && saved?.has("completedElapsedMs")==true)saved.optLong("completedElapsedMs") else null
    private var tick:Long?=null
    private var payloadActive=false
    private var lastDone:Long?=null
    private var started=saved?.optBoolean("started",saved.optLong("bytes")>0) ?: false

    fun resume(now:Long){tick=now}
    fun checkpoint(now:Long){
        tick?.let{previous->val delta=(now-previous).coerceAtLeast(0);if(!complete && payloadActive){elapsed+=delta;payload+=delta};tick=now}
    }
    fun beginPayload(now:Long){
        checkpoint(now)
        if(complete){elapsed=0;payload=0;bytes=0;lastDone=null;complete=false;completedElapsed=null;started=false}
    }
    fun progress(done:Long,now:Long){
        // Final counters may repeat after completion. A native sender's next
        // transfer has an explicit begin event; duplicates must not erase its average.
        if(complete)return
        checkpoint(now)
        // A first report can include cached data; never count that as traffic.
        lastDone?.let{previous->
            val delta=(done-previous).coerceAtLeast(0)
            if(delta>0){
                // Start at observed payload traffic, never at a zero/cached
                // sample that may precede a long connection wait.
                bytes+=delta;started=true;payloadActive=true
            }
        }
        lastDone=done
    }
    fun finishPayload(now:Long){checkpoint(now);if(!complete)completedElapsed=elapsed;payloadActive=false;complete=true}
    fun pause(now:Long){checkpoint(now);tick=null;payloadActive=false;lastDone=null}
    fun save(state:JSONObject){state.put("timing",JSONObject().put("elapsedMs",elapsed).put("payloadMs",payload).put("bytes",bytes).put("started",started).put("complete",complete).apply{completedElapsed?.let{put("completedElapsedMs",it)}})}

    companion object {
        fun duration(ms:Long):String {
            val seconds=ms.coerceAtLeast(0)/1000
            return if(seconds>=3600)String.format(Locale.ROOT,"%d:%02d:%02d",seconds/3600,seconds/60%60,seconds%60)
            else String.format(Locale.ROOT,"%d:%02d",seconds/60,seconds%60)
        }
        fun elapsedText(state:JSONObject):String=state.optJSONObject("timing")?.let{
            // Older native sender sessions did not retain their completion time.
            // Their live session duration cannot be presented as transfer duration.
            if(state.optString("action")=="send" && it.optBoolean("complete") && !it.has("completedElapsedMs"))return@let ""
            val elapsed=if(state.optString("action")=="send" && it.optBoolean("complete"))it.optLong("completedElapsedMs",it.optLong("elapsedMs")) else it.optLong("elapsedMs")
            "Elapsed ${duration(elapsed)}"
        } ?: ""
        fun average(state:JSONObject):Double? {
            // The existing sender counters follow the latest receiver. They
            // cannot provide an honest aggregate for concurrent receivers.
            if(state.optBoolean("timingConcurrent"))return null
            val timing=state.optJSONObject("timing") ?: return null
            val ms=timing.optLong("payloadMs");val bytes=timing.optLong("bytes")
            return if(timing.optBoolean("complete") && ms>0 && bytes>0)bytes.toDouble()*1000/ms else null
        }
        fun averageText(state:JSONObject):String=average(state)?.let{
            "Avg ${if(state.optString("action")=="receive")"download" else "upload"} · ${ProgressEvents.size(it.toLong())}/s"
        } ?: ""
    }
}
