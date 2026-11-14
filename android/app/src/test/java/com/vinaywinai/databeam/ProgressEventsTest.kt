package com.vinaywinai.databeam

import org.junit.Assert.*
import org.junit.Test

class ProgressEventsTest {
    @Test fun nativeRatesUseScaledUnitsAndRejectInvalidCounters(){
        assertEquals(ProgressEvents.Sample(64,128,1048576.0),ProgressEvents.native("64:128:1048576000"))
        for(value in listOf("-1:2:0","3:2:0","1:2:-1","1:2","x:2:0"))assertNull(value,ProgressEvents.native(value))
    }
    @Test fun connectionPhasesCannotBeMistakenForUserFileNames(){
        assertEquals("Authenticating code",ProgressEvents.crocPhase("authenticating code..."))
        assertEquals("Transferring",ProgressEvents.crocPhase("Sending (->203.0.113.1)"))
        assertNull(ProgressEvents.crocPhase("Sending 'connecting...' (1 MB)"))
    }
    @Test fun estimatesHideUnknownOrCompletedWork(){
        assertNull(ProgressEvents.eta(10,100,0.0));assertNull(ProgressEvents.eta(100,100,20.0))
        assertEquals("1m 30s",ProgressEvents.eta(10,100,1.0));assertEquals(50,ProgressEvents.percent(50,100))
        assertEquals("512.0 MiB",ProgressEvents.size(512L*1024*1024))
    }
    @Test fun crocOverallCountersIncludeResumeAndRejectInvalidTotals(){
        val value=ProgressEvents.croc("DATABEAM_TRANSFER_V1 {\"done\":250,\"total\":400,\"speed_bps\":50,\"files_done\":1,\"files_total\":3}")!!
        assertEquals(ProgressEvents.Sample(250,400,50.0),value.transfer)
        assertEquals(1,value.filesDone);assertEquals(3,value.filesTotal)
        assertTrue(ProgressEvents.croc("DATABEAM_VERIFY_V1")!!.verifying)
        assertEquals(100,ProgressEvents.percent(0,0,2,2))
        for(json in listOf("{\"done\":401,\"total\":400,\"speed_bps\":0,\"files_done\":1,\"files_total\":3}","{\"done\":0,\"total\":0,\"speed_bps\":0,\"files_done\":4,\"files_total\":3}"))assertNull(ProgressEvents.croc("DATABEAM_TRANSFER_V1 $json"))
    }
}
