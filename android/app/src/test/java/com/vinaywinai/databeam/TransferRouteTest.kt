package com.vinaywinai.databeam

import org.junit.Assert.*
import org.junit.Test

class TransferRouteTest {
    @Test fun crocReportsOnlyDirectionEvidence(){
        assertEquals("relay",TransferRoute.croc("Sending (->203.0.113.7)"))
        assertEquals("relay",TransferRoute.croc("Receiving (<-203.0.113.7)"))
        assertEquals("direct",TransferRoute.croc("Sending (10.0.0.2->169.254.83.107)"))
        assertEquals("direct",TransferRoute.croc("\u001b[32mReceiving (10.0.0.2<-169.254.83.107)\u001b[0m"))
        for(line in listOf("Sending 'file.bin' (1 MB)","relay connection failed","Receiving (<-)","Sending (file)"))assertNull(TransferRoute.croc(line))
    }
    @Test fun simultaneousReceiversDoNotProduceFalseSinglePath(){
        assertEquals("Direct + Relay",TransferRoute.label(listOf("direct","relay")))
        assertEquals("Direct",TransferRoute.label(listOf("direct","direct")))
        assertEquals("Connecting…",TransferRoute.label(listOf("unknown")))
        assertEquals("Relay · other connection pending",TransferRoute.label(listOf("relay","unknown")))
    }
}
