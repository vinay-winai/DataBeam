package com.vinaywinai.databeam
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class TransferLabelsTest {
    private fun state(action:String,status:String,phase:String="")=JSONObject().put("action",action).put("status",status).put("phase",phase).put("mode","EasySendMe")
    @Test fun closedWaitingSenderOffersRestartAndNeverResume(){
        val s=state("send","interrupted","Waiting for receiver").put("message","App restarted; saved data retained. Press Resume to reconnect.")
        assertEquals("Sharing stopped",TransferLabels.title(s))
        assertTrue(TransferLabels.message(s).contains("waiting for a receiver"))
        assertTrue(TransferLabels.message(s).contains("Restart sharing"))
        assertFalse(TransferLabels.message(s).contains("Resume"))
    }
    @Test fun receivingAndSendingHaveDifferentProgressAndRecoveryActions(){
        assertEquals("Receiving",TransferLabels.title(state("receive","running","Transferring")))
        assertEquals("Sending",TransferLabels.title(state("send","running","Transferring")))
        assertTrue(TransferLabels.message(state("receive","paused")).contains("Resume"))
        assertTrue(TransferLabels.message(state("send","stopped")).contains("Restart sharing"))
        assertTrue(TransferLabels.message(state("receive","interrupted").put("message","Network disconnected")).contains("paste the new one"))
    }
    @Test fun completeAndVerificationDoNotClaimTheSameThing(){
        assertEquals("Received",TransferLabels.title(state("receive","complete")))
        assertTrue(TransferLabels.message(state("receive","running","Verifying and saving")).contains("Wait for the saved location"))
        val s=state("receive","complete").put("message","Saved to Downloads / DataBeam")
        assertEquals("Saved to Downloads / DataBeam",TransferLabels.message(s))
    }
    @Test fun invalidContentAndTicketErrorsOfferTheActionThatCanResolveThem(){
        val changed=state("receive","interrupted").put("message","This code points to different files. Clear the transfer card and start a new receive.")
        assertFalse(TransferLabels.message(changed).contains("Resume"))
        val invalid=state("receive","interrupted").put("message","Sender did not supply a valid Sendme ticket")
        assertTrue(TransferLabels.message(invalid).contains("tap Receive"))
        assertFalse(TransferLabels.message(invalid).contains("tap Resume"))
    }
}
