package com.vinaywinai.databeam

import android.app.Activity
import android.app.AlertDialog
import android.app.Instrumentation
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import org.json.JSONObject
import java.util.UUID

internal object CrocHistoryQrRegression {
    fun run(runner: Instrumentation) {
        val context=runner.targetContext
        val id=UUID.randomUUID().toString()
        val result=Bundle()
        var activity:Activity?=null
        var exit=1
        try {
            val prior=TransferService.state(context).optString("id")
            check(!TransferService.isBusy())
            activity=runner.startActivitySync(
                Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
            runner.waitForIdleSync()
            CrocBatchReview.receipt(context,id).writeText("""{"strong":true}""")
            fun field(name:String)=MainActivity::class.java.getDeclaredField(name)
                .apply{isAccessible=true}.get(activity)
            fun scanButton(view:View):Button? {
                if(view is Button && view.text=="Scan QR")return view
                if(view is ViewGroup)for(index in 0 until view.childCount)
                    scanButton(view.getChildAt(index))?.let{return it}
                return null
            }
            val monitor=runner.addMonitor(QrScannerActivity::class.java.name,Instrumentation.ActivityResult(
                Activity.RESULT_OK,Intent().putExtra(QrScannerActivity.RESULT_CODE,"updated-croc-code")
            ),true)
            try {
                runner.runOnMainSync {
                    MainActivity::class.java.getDeclaredMethod("showCrocResume",JSONObject::class.java,String::class.java)
                        .apply{isAccessible=true}.invoke(activity,JSONObject().put("id",id).put("code","original-croc-code"),null)
                    val dialog=field("crocHistoryDialog") as AlertDialog
                    check(scanButton(dialog.window!!.decorView)!!.performClick())
                }
                runner.waitForIdleSync()
                runner.runOnMainSync {
                    check((field("crocHistoryInput") as EditText).text.toString()=="updated-croc-code")
                    (field("crocHistoryDialog") as AlertDialog).dismiss()
                    val parse=MainActivity::class.java.getDeclaredMethod("acceptScannedCode",String::class.java).apply{isAccessible=true}
                    val mode=MainActivity::class.java.getDeclaredMethod("selectedMode").apply{isAccessible=true}
                    parse.invoke(activity,"updated-croc-codeesm")
                    check(mode.invoke(activity)=="EasySendMe")
                    parse.invoke(activity,"updated-croc-code")
                    check(mode.invoke(activity)=="Croc")
                }
            }finally{runner.removeMonitor(monitor)}
            check(TransferService.state(context).optString("id")==prior)
            result.putString("result","PASS history Scan QR filled the resume field, and the esm marker selected EasySendMe.")
            exit=0
        }catch(error:Throwable){result.putString("error",error.stackTraceToString())}
        finally{
            CrocBatchReview.clear(context,id)
            runner.runOnMainSync{activity?.finish()}
        }
        runner.finish(exit,result)
    }
}
