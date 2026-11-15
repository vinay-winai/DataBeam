package com.vinaywinai.databeam

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class TransferTimingTest {
    @Test fun elapsedIncludesPreparationButAverageUsesPayloadThroughSaving(){
        val state=JSONObject().put("action","receive")
        val time=TransferTiming();time.resume(1000);time.checkpoint(6000);time.save(state)
        assertEquals("Elapsed 0:05",TransferTiming.elapsedText(state));assertNull(TransferTiming.average(state))
        time.progress(0,6000);time.progress(2000,7000);time.finishPayload(8000);time.pause(8000);time.save(state)
        assertEquals(7000,state.getJSONObject("timing").getLong("elapsedMs"))
        assertEquals(1000.0,TransferTiming.average(state)!!,0.001)
        assertEquals("Avg download · 1000 B/s",TransferTiming.averageText(state))
        time.checkpoint(900000);time.save(state)
        assertEquals("Elapsed 0:07",TransferTiming.elapsedText(state))
    }
    @Test fun resumedCacheAndAppDowntimeCannotInflateSpeedOrDuration(){
        val state=JSONObject()
        val first=TransferTiming();first.resume(0);first.progress(1000,100);first.progress(2000,1100);first.pause(1100);first.save(state)
        val next=TransferTiming(state.getJSONObject("timing"));next.resume(100000)
        next.progress(2000,100500);next.progress(4000,102500);next.finishPayload(102500);next.pause(102500);next.save(state)
        val timing=state.getJSONObject("timing")
        assertEquals(3000,timing.getLong("bytes"));assertEquals(3000,timing.getLong("payloadMs"))
        assertEquals(3600,timing.getLong("elapsedMs"));assertEquals(1000.0,TransferTiming.average(state)!!,0.001)
    }
    @Test fun retainedCrocPauseFreezesUntilResume(){
        val state=JSONObject();val time=TransferTiming();time.resume(0);time.progress(0,0);time.progress(1000,1000);time.pause(1000)
        time.checkpoint(20000);time.save(state);assertEquals(1000,state.getJSONObject("timing").getLong("elapsedMs"))
        time.resume(20000);time.progress(2000,21000);time.finishPayload(21000);time.pause(21000);time.save(state)
        assertEquals(2000,state.getJSONObject("timing").getLong("elapsedMs"));assertEquals(1000.0,TransferTiming.average(state)!!,0.001)
    }
    @Test fun offlineCachedExportHasNoDownloadAverage(){
        val state=JSONObject();val time=TransferTiming();time.resume(0);time.progress(999999,1000);time.finishPayload(2000);time.pause(2000);time.save(state)
        assertNull(TransferTiming.average(state));assertEquals("",TransferTiming.averageText(state))
    }
    @Test fun senderWaitDoesNotDiluteCompletedAverageAndNextReceiverStartsFresh(){
        val state=JSONObject().put("action","send");val time=TransferTiming();time.resume(0);time.progress(0,1000);time.progress(2000,2000);time.finishPayload(2000)
        time.checkpoint(12000);time.save(state);assertEquals(2000.0,TransferTiming.average(state)!!,0.001)
        time.progress(2000,12000);time.save(state);assertEquals(2000.0,TransferTiming.average(state)!!,0.001)
        time.beginPayload(12000);time.progress(1000,12000);time.progress(3000,14000);time.finishPayload(14000);time.save(state)
        assertEquals(1000.0,TransferTiming.average(state)!!,0.001);assertEquals("Elapsed 0:14",TransferTiming.elapsedText(state))
    }
    @Test fun formattingSupportsHoursAndLegacySessions(){
        assertEquals("1:01:05",TransferTiming.duration(3665000));assertEquals("0:00",TransferTiming.duration(-1))
        assertEquals("",TransferTiming.elapsedText(JSONObject()));assertNull(TransferTiming.average(JSONObject()))
    }
}
