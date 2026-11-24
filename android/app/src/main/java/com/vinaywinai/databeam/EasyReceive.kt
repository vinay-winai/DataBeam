package com.vinaywinai.databeam

import org.json.JSONObject

/** Croc exchanges a ticket once; subsequent receives reconnect to that ticket. */
object EasyReceive {
    fun needsLookup(config:JSONObject)=config.optString("ticket").isEmpty() || config.optBoolean("refresh") ||
        (config.has("ticketCode") && config.optString("ticketCode")!=config.optString("code"))

    fun hasSavedTicket(config:JSONObject,hash:(String)->String):Boolean =
        config.optString("ticket").let { it.isNotEmpty() && hash(it).isNotEmpty() }

    fun reuseSavedTicket(config:JSONObject,hash:(String)->String):Boolean {
        if(!hasSavedTicket(config,hash))return false
        val savedCode=config.optString("ticketCode").ifEmpty { config.optString("code") }
        config.put("code",savedCode).put("ticketCode",savedCode)
        config.remove("refresh")
        return true
    }

    fun requestLookup(config:JSONObject,code:String){
        if(config.optString("ticket").isNotEmpty() && !config.has("ticketCode")){
            config.put("ticketCode",config.optString("code"))
        }
        config.put("code",code).put("refresh",true)
    }

    fun takeRequestedCode(config:JSONObject):String? {
        if(!config.optBoolean("refresh"))return null
        val requested=config.optString("code")
        config.remove("refresh")
        val savedCode=config.optString("ticketCode")
        if(config.optString("ticket").isNotEmpty() && savedCode.isNotEmpty())config.put("code",savedCode)
        return requested
    }

    fun accept(config:JSONObject,ticket:String,hash:(String)->String){
        val next=hash(ticket)
        require(next.isNotEmpty()){"Sender did not supply a valid Sendme ticket"}
        val previous=config.optString("ticket")
        require(previous.isEmpty() || hash(previous)==next){"This code now refers to different files. Clear the transfer card and start a new receive."}
        config.put("ticket",ticket).put("ticketCode",config.optString("code"))
        config.remove("refresh") // A refresh is one lookup, never a persistent resume mode.
    }
}
