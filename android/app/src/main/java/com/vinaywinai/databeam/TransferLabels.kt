package com.vinaywinai.databeam

import org.json.JSONObject

/** Presentation depends on direction: a stopped sender starts a new session. */
object TransferLabels {
    private fun senderRetry(s:JSONObject)=if(s.optString("text").isNotEmpty())"Your text is kept. Edit it and tap Send text, or tap Restart sharing."
        else "To resume, reselect the exact same files or folder and start a new transfer. "+
            if(s.optString("mode")=="Croc")"Keep the same Croc code so the receiver can tap Resume with its partial files."
            else "Share the new code or ticket with the receiver; it can continue using its saved data."
    fun broadcast(s:JSONObject)=s.optString("action")=="send" && s.optString("mode")=="Sendme" && s.optBoolean("broadcast")
    fun autoEnd(s:JSONObject)=s.optString("action")=="send" && s.optString("mode") in listOf("Sendme","EasySendMe") &&
        !broadcast(s) && s.optJSONObject("timing")?.optBoolean("complete")==true
    fun senderFinished(s:JSONObject)=s.optString("action")=="send" &&
        s.optString("phase")=="Waiting for receiver" && !s.optBoolean("troubleshooting") && !s.optBoolean("preparing") &&
        s.optJSONObject("timing")?.optBoolean("complete")==true && (s.optJSONObject("senderRoutes")?.length() ?: 0)==0
    fun sentAndSharing(s:JSONObject)=broadcast(s) && s.optString("status")=="running" && senderFinished(s)
    fun interruptedMessage(s:JSONObject):String {
        if(s.optString("action")=="send")return if(s.optString("phase")=="Waiting for receiver")
            "App closed while waiting for a receiver. ${senderRetry(s)}"
        else "Sharing stopped when the app closed. ${senderRetry(s)}"
        return "Receiving stopped when the app closed. ${receiveRetry(s)}"
    }
    fun title(s:JSONObject):String {
        val receiving=s.optString("action")=="receive"
        return when(s.optString("status")) {
            "running"->when {
                s.optBoolean("troubleshooting")->"Checking public relays…"
                senderFinished(s)->"Sent"
                s.optString("phase")=="Transferring"->if(receiving)"Receiving" else "Sending"
                s.optString("phase")=="Waiting for receiver"->"Waiting for a receiver"
                else->s.optString("phase").ifEmpty{"Preparing transfer"}
            }
            "paused"->if(receiving)"Receive paused" else "Sharing paused"
            "interrupted"->if(receiving)"Receive interrupted" else "Sharing stopped"
            "stopping"->if(receiving)"Pausing receive…" else "Stopping sharing…"
            "stopped"->"Sharing stopped"
            "complete"->if(receiving)"Received" else "Sent"
            else->"Preparing transfer"
        }
    }
    fun message(s:JSONObject):String {
        val receiving=s.optString("action")=="receive"
        val raw=s.optString("message")
        return when(s.optString("status")) {
            "paused"->if(receiving)receiveRetry(s) else senderRetry(s)
            "stopped"->senderRetry(s)
            "interrupted"->when {
                raw.startsWith("App restarted") || raw.startsWith("App closed") || raw.isBlank()->interruptedMessage(s)
                raw.contains("Clear the transfer card",ignoreCase=true)->raw
                receiving && (raw.contains("invalid ticket",ignoreCase=true) || raw.contains("did not supply",ignoreCase=true))->"$raw\nConfirm both devices use ${s.optString("mode")}. Paste the correct code or ticket, then tap Receive."
                receiving->"$raw\n${receiveRetry(s)}"
                else->"$raw\n${senderRetry(s)}"
            }
            "complete"->if(receiving)if(raw in listOf("Complete",""))"Received successfully." else raw else "Sent successfully."
            "running"->when {
                s.optBoolean("troubleshooting")->"Checking which public relays are reachable. Please wait for the result."
                senderFinished(s)->if(broadcast(s))"Files sent successfully. Broadcast stays open for other receivers. Tap Stop sharing when finished."
                else "Files sent successfully. Ending sharing automatically."
                s.optString("phase")=="Waiting for receiver"->"On the receiving device, select ${s.optString("mode")}, choose Receive and enter this ${if(s.optString("mode")=="Sendme")"ticket" else "code"}."
                s.optString("phase")=="Connecting"->"Keep the sender sharing while the devices connect."
                s.optString("phase")=="Getting sender ticket"->"Getting the file ticket through Croc. Keep the sender sharing this code."
                s.optString("phase")=="Reconnecting to sender"->"Continuing with the saved ticket and data. If sharing ended or the sender restarted, tap Pause, then Resume → New code."
                s.optString("phase") in listOf("Verifying files","Verifying and saving","Saving files")->"Finishing file checks and saving. Wait for the saved location before opening the files."
                raw in listOf("Preparing…","Complete")->""
                else->raw
            }
            "stopping"->if(receiving)"Keeping partial data so you can resume." else "Closing the sharing session."
            else->raw
        }
    }
    private fun receiveRetry(s:JSONObject)=if(s.optString("mode")=="EasySendMe")
        "Saved data is kept. Tap Resume. Use the current session if the sender is still sharing, or New code if they shared the same files again."
        else if(s.optString("mode")=="Sendme")"Saved data is kept. Tap Resume. Use the current session while it is still sharing, or New ticket after the sender shares the same files again."
        else "Partial data is kept. Keep the sender sharing, then tap Resume. If its code or ticket changed, paste the new one first."
}
