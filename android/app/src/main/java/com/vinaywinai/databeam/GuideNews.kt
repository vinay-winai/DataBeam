package com.vinaywinai.databeam

import java.security.MessageDigest

/** Same content-revision/read rule as desktop's src/troubleshoot.rs. */
data class GuideNews(var seen:String,var latest:String?=null){
    fun hasUnread()=latest?.let{it!=seen} ?: false
    fun observe(revision:String){latest=revision}
    fun markRead(){latest?.let{seen=it}}
    companion object {
        fun revision(contents:String)=MessageDigest.getInstance("SHA-256").digest(contents.replace("\r\n","\n").toByteArray(Charsets.UTF_8)).joinToString(""){"%02x".format(it)}
    }
}
