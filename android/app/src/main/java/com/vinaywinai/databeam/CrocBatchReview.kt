package com.vinaywinai.databeam

import android.content.Context
import android.util.AtomicFile
import org.json.JSONObject
import java.io.File

internal object CrocBatchReview {
    private val tokenFormat = Regex("[0-9a-f]{32}")

    fun receipt(context: Context, transferId: String): File {
        require(transferId.matches(Regex("[A-Za-z0-9_-]{1,100}"))) { "Invalid Croc transfer" }
        return File(File(context.filesDir, "croc-resume").apply { mkdirs() }, "$transferId.json")
    }

    fun read(context: Context, transferId: String, token: String): JSONObject? = runCatching {
        require(tokenFormat.matches(token))
        val base = receipt(context, transferId)
        JSONObject(File(base.path + ".review").readText()).takeIf { it.optString("id") == token }
    }.getOrNull()

    fun respond(context: Context, transferId: String, token: String, apply: Boolean) {
        val state = TransferService.state(context)
        check(TransferService.isBusy() && state.optString("id") == transferId &&
            state.optString("crocReview") == token && read(context, transferId, token) != null
        ) { "The file review has ended. Open Croc receive history to resume again." }
        val decision = AtomicFile(File(receipt(context, transferId).path + ".decision"))
        val stream = decision.startWrite()
        try {
            stream.write(((if (apply) "apply " else "keep ") + token).toByteArray(Charsets.UTF_8))
            decision.finishWrite(stream)
        } catch (error: Exception) {
            decision.failWrite(stream)
            throw error
        }
    }

    fun clear(context: Context, transferId: String) {
        val base = receipt(context, transferId).path
        for (suffix in listOf("", ".tmp", ".review", ".review.tmp", ".revision", ".revision.tmp")) {
            File(base + suffix).delete()
        }
        AtomicFile(File(base + ".decision")).delete()
    }

    fun clearDraft(receipt: File) {
        File(receipt.path + ".review").delete()
        AtomicFile(File(receipt.path + ".decision")).delete()
    }
}
