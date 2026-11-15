package com.vinaywinai.databeam

import org.json.JSONObject

/** Presentation depends on direction: a stopped sender starts a new session. */
object TransferLabels {
    fun senderFinished(s:JSONObject)=s.optString("action")=="send" &&
        s.optString("phase")=="Waiting for receiver" && !s.optBoolean("troubleshooting") && !s.optBoolean("preparing") &&
        s.optJSONObject("timing")?.optBoolean("complete")==true && (s.optJSONObject("senderRoutes")?.length() ?: 0)==0
    fun sentAndSharing(s:JSONObject)=s.optString("status")=="running" && senderFinished(s)
    fun interruptedMessage(s:JSONObject):String {
        if(s.optString("action")=="send")return if(s.optString("phase")=="Waiting for receiver")
            "App closed while waiting for a receiver. Tap Restart sharing to try again."
        else "Sharing stopped when the app closed. Tap Restart sharing to try again."
        return "Receiving stopped when the app closed. Partial data is kept. Keep the sender sharing, then tap Resume."
    }
    fun title(s:JSONObject):String {
        val receiving=s.optString("action")=="receive"
        return when(s.optString("status")) {
            "running"->when {
                s.optBoolean("troubleshooting")->"Checking public relays…"
                sentAndSharing(s)->"Sent"
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
            "paused"->if(receiving)"Partial data is kept. Keep the sender sharing, then tap Resume." else "Tap Restart sharing to start a new session."
            "stopped"->"Tap Restart sharing to use the previous files or text."
            "interrupted"->when {
                raw.startsWith("App restarted") || raw.startsWith("App closed") || raw.isBlank()->interruptedMessage(s)
                raw.contains("Clear the transfer card",ignoreCase=true)->raw
                receiving && (raw.contains("invalid ticket",ignoreCase=true) || raw.contains("did not supply",ignoreCase=true))->"$raw\nConfirm both devices use ${s.optString("mode")}. Paste the correct code or ticket, then tap Receive."
                !receiving && raw.contains("files are missing",ignoreCase=true)->"$raw\nSelect the files again, then tap Send files."
                receiving->"$raw\nKeep the sender sharing, then tap Resume. If its code or ticket changed, paste the new one first."
                else->"$raw\nTap Restart sharing to try again, or select the files again if they are no longer available."
            }
            "complete"->if(raw in listOf("Complete", ""))if(receiving)"Transfer finished." else "Transfer finished. Use Share again to send to another device." else raw
            "running"->when {
                s.optBoolean("troubleshooting")->"Checking which public relays are reachable. Please wait for the result."
                sentAndSharing(s)->if(s.optString("mode")=="EasySendMe")
                    "Files sent successfully. Sharing stays open so this receiver can reconnect. When finished, tap Stop sharing. To send to another device, stop sharing and start a new share."
                else "Files sent successfully. Sharing stays open for another receiver. Tap Stop sharing when finished."
                s.optString("phase")=="Waiting for receiver"->"On the receiving device, select ${s.optString("mode")}, choose Receive and enter this ${if(s.optString("mode")=="Sendme")"ticket" else "code"}."
                s.optString("phase")=="Connecting"->"Keep the sender sharing while the devices connect."
                s.optString("phase") in listOf("Verifying files","Verifying and saving","Saving files")->"Finishing file checks and saving. Wait for the saved location before opening the files."
                raw in listOf("Preparing…","Complete")->""
                else->raw
            }
            "stopping"->if(receiving)"Keeping partial data so you can resume." else "Closing the sharing session."
            else->raw
        }
    }
}
