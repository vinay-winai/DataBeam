package com.vinaywinai.databeam

object TransferRoute {
    /** Croc's direction line describes the payload path; relay configuration does not. */
    fun croc(line:String):String? {
        val clean=line.replace(Regex("\u001B\\[[0-?]*[ -/]*[@-~]"),"").trim().lowercase()
        val body=when {
            clean.startsWith("sending (")->clean.removePrefix("sending (")
            clean.startsWith("receiving (")->clean.removePrefix("receiving (")
            else->return null
        }.substringBefore(')').trim()
        if(body.contains('\''))return null
        val arrow=Regex("->|<-").find(body) ?: return null
        if(body.substring(arrow.range.last+1).isBlank())return null
        return if(arrow.range.first==0)"relay" else "direct"
    }
    fun label(routes:Collection<String>):String {
        val known=routes.filter{it in listOf("direct","relay")}.toSet()
        val result=when {
            known.size==2->"Direct + Relay"
            "direct" in known->"Direct"
            "relay" in known->"Relay"
            else->"Connecting…"
        }
        return if(known.isNotEmpty() && routes.any{it !in known})"$result · other connection pending" else result
    }
}
