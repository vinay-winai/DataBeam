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
}
