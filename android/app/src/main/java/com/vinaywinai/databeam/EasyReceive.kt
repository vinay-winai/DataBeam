package com.vinaywinai.databeam

import org.json.JSONObject

/** Croc exchanges a ticket once; subsequent receives reconnect to that ticket. */
object EasyReceive {
    fun needsLookup(config:JSONObject)=config.optString("ticket").isEmpty() || config.optBoolean("refresh") ||
        (config.has("ticketCode") && config.optString("ticketCode")!=config.optString("code"))

    fun accept(config:JSONObject,ticket:String,hash:(String)->String){
        val next=hash(ticket)
        require(next.isNotEmpty()){"Sender did not supply a valid Sendme ticket"}
        val previous=config.optString("ticket")
        require(previous.isEmpty() || hash(previous)==next){"This code now refers to different files. Clear the transfer card and start a new receive."}
        config.put("ticket",ticket).put("ticketCode",config.optString("code"))
        config.remove("refresh") // A refresh is one lookup, never a persistent resume mode.
    }
}
