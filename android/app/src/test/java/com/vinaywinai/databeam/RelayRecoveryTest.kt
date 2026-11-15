package com.vinaywinai.databeam
import org.junit.Assert.*
import org.junit.Test
import org.junit.Before

class RelayRecoveryTest {
    private lateinit var words:Set<String>
    private fun isPhrase(value:String,count:Int=3):Boolean=if(count==1)value in words else
        value.indices.any{index->value[index]=='-' && value.take(index) in words && isPhrase(value.substring(index+1),count-1)}
    @Before fun loadDesktopWords() {
        val text=javaClass.getResourceAsStream("/eff-short-wordlist-1.txt")!!.bufferedReader().use{it.readText()}
        words=text.lineSequence().filter{it.isNotBlank()}.toSet()
        RelayRecovery.initializeWordList(text)
    }
    @Test fun bareRelayConnectionFailureTriggersTroubleshooting(){
        assertTrue(RelayRecovery.transportError("relay connection failed"))
        assertTrue(RelayRecovery.transportError("relay connection failed: 3.getcroc.com:9009"))
        assertFalse(RelayRecovery.transportError("relay connection failed: incorrect password"))
        assertFalse(RelayRecovery.transportError("room not found"))
    }
    @Test fun generatedCodesUseThreeWordsFromTheDesktopDictionary() {
        assertEquals(1296,words.size)
        repeat(100){
            val code=RelayRecovery.newCode()
            assertTrue(code,isPhrase(code))
        }
    }
    @Test fun replacementConnectsPeersToTheChosenReachableRelay() {
        for(endpoint in RelayRecovery.relays) {
            val code=RelayRecovery.replacement("my-custom-code",endpoint)
            assertNotEquals("my-custom-code",code)
            assertEquals(endpoint,RelayRecovery.relay(code))
            assertTrue(code,isPhrase(code))
        }
    }
    @Test fun hyphenatedDictionaryWordsStillCountAsOneWord(){
        assertTrue("yo-yo" in words)
        assertTrue(isPhrase("yo-yo-${words.first()}-${words.last()}"))
        assertFalse(isPhrase("yo-yo-${words.first()}"))
    }
    @Test fun probesReturnOnlyReachableRelaysAndHandleExceptions() {
        val result=RelayRecovery.healthy(stopped={false},connect={endpoint->
            if(endpoint.startsWith("3."))error("Simulated network failure")
            endpoint.startsWith("1.") || endpoint.startsWith("4.")
        })
        assertEquals(listOf("1.getcroc.com:9009","4.getcroc.com:9009"),result)
        assertTrue(RelayRecovery.healthy(stopped={false},connect={false}).isEmpty())
    }
    @Test fun cancellationDoesNotWaitForSlowProbes() {
        val start=System.nanoTime()
        val result=RelayRecovery.healthy(stopped={true},connect={Thread.sleep(10000);true})
        assertTrue(result.isEmpty())
        assertTrue((System.nanoTime()-start)/1000000<1000)
    }
    @Test fun authenticationAndFileErrorsNeverTriggerCodeReplacement() {
        assertTrue(RelayRecovery.transportError("could not connect to 3.getcroc.com:9009: dial tcp: i/o timeout"))
        for(error in listOf("error: wrong password","room not found","decompress: corrupt input","file permission denied","could not connect to 1.getcroc.com:9009: lookup example.com: i/o timeout")) {
            assertFalse(error,RelayRecovery.transportError(error))
        }
    }
    @Test fun terminalErrorIsVisibleWithoutALogPane() {
        val message=RelayRecovery.failureMessage("DATABEAM_RATE_V1 {speed:0}\n\u001b[31merror: permission denied\u001b[0m\nclosing...",1)
        assertTrue(message.contains("permission denied"))
        assertFalse(message.contains("DATABEAM_RATE"))
        assertFalse(message.contains("\u001b"))
    }
    @Test fun unrecognizedFailureHasActionableStatus() {
        val message=RelayRecovery.failureMessage("connecting...\nwaiting...",1)
        assertTrue(message.contains("Check the code"))
        assertFalse(message.contains("log"))
    }
}
