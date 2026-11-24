package com.vinaywinai.databeam

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class EasyReceiveTest {
    private fun hash(ticket:String)=when(ticket){"old","new"->"same-content";"other"->"other-content";else->""}
    @Test fun refreshedTicketIsConsumedOnceAndSurvivesRepeatedResume(){
        val config=JSONObject().put("code","new-code").put("ticket","old").put("refresh",true)
        assertTrue(EasyReceive.needsLookup(config))
        EasyReceive.accept(config,"new",::hash)
        assertFalse(config.has("refresh"))
        repeat(3){assertFalse(EasyReceive.needsLookup(JSONObject(config.toString())))}
        config.put("code","changed-code");assertTrue(EasyReceive.needsLookup(config))
    }
    @Test fun explicitRefreshWorksEvenWithUnchangedCode(){
        val config=JSONObject().put("code","same-code").put("ticket","old")
        assertFalse(EasyReceive.needsLookup(config)) // Legacy saved tickets remain usable.
        config.put("refresh",true);assertTrue(EasyReceive.needsLookup(config))
        EasyReceive.accept(config,"new",::hash);assertFalse(EasyReceive.needsLookup(config))
    }
    @Test fun wrongContentNeverReplacesTheSavedTicket(){
        val config=JSONObject().put("ticket","old").put("refresh",true)
        try{EasyReceive.accept(config,"other",::hash);fail("Accepted different content")}catch(_:IllegalArgumentException){}
        assertEquals("old",config.getString("ticket"));assertTrue(config.getBoolean("refresh"))
    }

    @Test fun senderStillSharingAfterReopeningUsesSavedTicketDespiteAnInterruptedLookup(){
        val original=JSONObject().put("code","first-code").put("ticket","old")
        EasyReceive.requestLookup(original,"replacement-code")
        val reopened=JSONObject(original.toString())
        assertTrue(EasyReceive.needsLookup(reopened))
        assertTrue(EasyReceive.reuseSavedTicket(reopened,::hash))
        assertEquals("first-code",reopened.getString("code"))
        assertEquals("old",reopened.getString("ticket"))
        assertFalse(EasyReceive.needsLookup(reopened))
    }

    @Test fun requestingACodeDoesNotPersistTheLookupAsTheNextResumeAction(){
        val snapshot=JSONObject().put("code","first-code").put("ticket","old")
        EasyReceive.requestLookup(snapshot,"replacement-code")
        assertEquals("replacement-code",EasyReceive.takeRequestedCode(snapshot))
        assertFalse(EasyReceive.needsLookup(JSONObject(snapshot.toString())))
        assertEquals("first-code",snapshot.getString("code"))
    }

    @Test fun interruptedInitialLookupHasNoSavedTicketToReconnectWith(){
        val config=JSONObject().put("code","first-code").put("ticket","")
        assertFalse(EasyReceive.hasSavedTicket(config,::hash))
        assertFalse(EasyReceive.reuseSavedTicket(config,::hash))
        assertTrue(EasyReceive.needsLookup(config))
    }
}
