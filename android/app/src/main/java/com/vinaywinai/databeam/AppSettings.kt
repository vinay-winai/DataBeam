package com.vinaywinai.databeam

import android.content.Context
import android.net.Uri
import org.json.JSONArray

/** Defaults for new transfers. A running/paused transfer retains its own destination. */
object AppSettings {
    private fun prefs(c:Context)=c.getSharedPreferences("settings",Context.MODE_PRIVATE)
    fun relay(c:Context)=prefs(c).getString("relay","")!!
    fun setRelay(c:Context,value:String){prefs(c).edit().putString("relay",value.trim()).apply()}
    fun folder(c:Context)=prefs(c).getString("saveFolder","")!!
    fun folderLabel(c:Context)=prefs(c).getString("saveFolderLabel","Downloads / DataBeam")!!
    fun setFolder(c:Context,uri:Uri?,label:String="Downloads / DataBeam"){
        prefs(c).edit().putString("saveFolder",uri?.toString() ?: "").putString("saveFolderLabel",label).apply()
    }
    fun code(c:Context,mode:String)=prefs(c).getString("code-$mode","")!!
    fun setCode(c:Context,mode:String,code:String){prefs(c).edit().putString("code-$mode",code).apply()}
    fun recent(c:Context,mode:String,action:String):List<String>{
        val values=runCatching{JSONArray(prefs(c).getString("recent-$mode-$action","[]"))}.getOrDefault(JSONArray())
        return (0 until values.length()).map{values.getString(it)}.distinct().take(5)
    }
    @Synchronized fun remember(c:Context,mode:String,action:String,code:String){
        if(code.isBlank() || mode=="Sendme")return
        val values=(listOf(code)+recent(c,mode,action)).distinct().take(5)
        prefs(c).edit().putString("recent-$mode-$action",JSONArray(values).toString()).apply()
        if(action=="send")setCode(c,mode,code)
    }
    fun clearRecent(c:Context,mode:String,action:String){prefs(c).edit().remove("recent-$mode-$action").apply()}
}
