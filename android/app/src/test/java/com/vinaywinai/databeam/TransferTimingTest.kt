package com.vinaywinai.databeam
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class TransferTimingTest {
    @Test fun preparingZeroSamplesAndReceiverWaitDoNotStartElapsed(){
        val s=JSONObject().put("action","receive");val t=TransferTiming();t.resume(0)
        t.checkpoint(5000);t.progress(0,5000);t.checkpoint(60000);t.save(s)
        assertEquals("Elapsed 0:00",TransferTiming.elapsedText(s));assertNull(TransferTiming.average(s))
        t.progress(1000,61000);t.progress(3000,62000);t.finishPayload(63000);t.pause(63000);t.save(s)
        assertEquals(2000,s.getJSONObject("timing").getLong("elapsedMs"))
        assertEquals(1500.0,TransferTiming.average(s)!!,0.001)
        t.checkpoint(999999);t.save(s);assertEquals("Elapsed 0:02",TransferTiming.elapsedText(s))
    }
    @Test fun resumedCacheAndAppDowntimeCannotInflateSpeedOrDuration(){
        val s=JSONObject();val t=TransferTiming();t.resume(0);t.progress(1000,100)
        t.progress(2000,1100);t.progress(3000,2100);t.pause(3100);t.save(s)
        val next=TransferTiming(s.getJSONObject("timing"));next.resume(100000)
        next.progress(3000,100500);next.progress(4000,102500);next.progress(6000,103500)
        next.finishPayload(104500);next.pause(104500);next.save(s)
        assertEquals(5000,s.getJSONObject("timing").getLong("bytes"))
        assertEquals(4000,s.getJSONObject("timing").getLong("elapsedMs"))
        assertEquals(1250.0,TransferTiming.average(s)!!,0.001)
    }
    @Test fun retainedCrocPauseFreezesAndResumeWaitIsExcluded(){
        val s=JSONObject();val t=TransferTiming();t.resume(0);t.progress(0,0);t.progress(1000,1000);t.pause(2000)
        t.checkpoint(20000);t.save(s);assertEquals(1000,s.getJSONObject("timing").getLong("elapsedMs"))
        t.resume(20000);t.progress(1000,21000);t.progress(2000,22000);t.finishPayload(23000);t.save(s)
        assertEquals(2000,s.getJSONObject("timing").getLong("elapsedMs"))
    }
    @Test fun offlineCachedExportHasNoTransferTimeOrDownloadAverage(){
        val s=JSONObject();val t=TransferTiming();t.resume(0);t.progress(999999,1000);t.finishPayload(2000);t.pause(2000);t.save(s)
        assertEquals("Elapsed 0:00",TransferTiming.elapsedText(s));assertNull(TransferTiming.average(s))
    }
    @Test fun completedSenderFreezesAndNextBroadcastRecipientStartsFresh(){
        val s=JSONObject().put("action","send");val t=TransferTiming();t.resume(0);t.progress(0,1000);t.progress(2000,2000);t.finishPayload(3000)
        t.checkpoint(12000);t.finishPayload(12000);t.progress(2000,12000);t.save(s)
        assertEquals("Elapsed 0:01",TransferTiming.elapsedText(s));assertEquals(2000.0,TransferTiming.average(s)!!,0.001)
        t.beginPayload(12000);t.progress(0,12000);t.progress(1000,13000);t.progress(3000,14000);t.finishPayload(15000);t.save(s)
        assertEquals("Elapsed 0:02",TransferTiming.elapsedText(s));assertEquals(1500.0,TransferTiming.average(s)!!,0.001)
    }
    @Test fun formattingSupportsHoursAndLegacySessions(){
        assertEquals("1:01:05",TransferTiming.duration(3665000));assertEquals("0:00",TransferTiming.duration(-1))
        assertEquals("",TransferTiming.elapsedText(JSONObject()));assertNull(TransferTiming.average(JSONObject()))
        assertEquals("",TransferTiming.elapsedText(JSONObject().put("action","send").put("timing",JSONObject().put("complete",true).put("elapsedMs",999999))))
    }
}
