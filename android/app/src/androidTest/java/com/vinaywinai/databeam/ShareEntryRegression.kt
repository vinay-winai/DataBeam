package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.widget.*
import org.json.JSONObject
import java.util.UUID

object ShareEntryRegression {
    fun run(runner:Instrumentation){
        val context=runner.targetContext;val created=mutableListOf<Uri>();val opened=mutableListOf<Activity>()
        val prefs=context.getSharedPreferences("transfer",0);val original=prefs.getString("state",null)
        var resultCode=0;var result=Bundle()
        fun field(activity:Activity,name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        // startActivitySync cannot match an alias name to the instantiated target
        // class. Alias discovery is checked above; exercise its MainActivity here.
        fun open(intent:Intent):Activity {
            runner.sendStatus(1,Bundle().apply{putString("step","Opening ${intent.action}")})
            return runner.startActivitySync(intent.setComponent(ComponentName(context,MainActivity::class.java)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_MULTIPLE_TASK)).also{opened.add(it);runner.waitForIdleSync()}
        }
        try {
            check(TransferService.state(context).optString("status") !in listOf("running","stopping")){"A user transfer is active"}
            for(action in listOf(Intent.ACTION_SEND,Intent.ACTION_SEND_MULTIPLE,Intent.ACTION_PROCESS_TEXT)){
                val matches=context.packageManager.queryIntentActivities(Intent(action).setType(if(action==Intent.ACTION_PROCESS_TEXT)"text/plain" else "*/*").setPackage(context.packageName),0)
                check(matches.any{it.activityInfo.name.endsWith(".ShareActivity") && it.loadLabel(context.packageManager).toString()=="Send with DataBeam"}){"Share target missing for $action"}
            }
            prefs.edit().putString("state",JSONObject().put("id","share-test-prior").put("action","receive").put("mode","Sendme").put("status","complete").toString()).commit()
            val text="Shared text\nUnicode: αβγ"
            var activity=open(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT,text))
            fun checkText(a:Activity){runner.runOnMainSync{
                check(field(a,"tab")=="send");check((field(a,"mode") as Spinner).selectedItem=="Croc")
                check((field(a,"message") as EditText).text.toString()==text)
            }}
            checkText(activity)
            var helpTarget:String?=null
            val helpMonitor=object:Instrumentation.ActivityMonitor(){
                override fun onStartActivity(intent:Intent):Instrumentation.ActivityResult? {
                    if(intent.action!=Intent.ACTION_VIEW)return null
                    helpTarget=intent.dataString
                    return Instrumentation.ActivityResult(Activity.RESULT_CANCELED,null)
                }
            }
            runner.addMonitor(helpMonitor)
            runner.runOnMainSync{
                fun find(view:android.view.View):android.view.View? {
                    if(view.contentDescription=="Troubleshoot")return view
                    if(view is android.view.ViewGroup)for(i in 0 until view.childCount)find(view.getChildAt(i))?.let{return it}
                    return null
                }
                check(find(activity.window.decorView)?.performClick()==true){"Bottom help button missing"}
            }
            runner.waitForIdleSync();runner.removeMonitor(helpMonitor)
            check(helpTarget=="https://github.com/vinay-winai/DataBeam/blob/main/troubleshoot.md"){"Help redirect was incorrect"}
            val monitor=runner.addMonitor(MainActivity::class.java.name,null,false)
            runner.runOnMainSync{activity.recreate()}
            activity=runner.waitForMonitorWithTimeout(monitor,10000) ?: error("Share recreation did not return")
            opened.add(activity);runner.removeMonitor(monitor);runner.waitForIdleSync();checkText(activity)
            activity=open(Intent(Intent.ACTION_PROCESS_TEXT).setType("text/plain").putExtra(Intent.EXTRA_PROCESS_TEXT,text).putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY,true));checkText(activity)
            fun document():Uri {
                val uri=context.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{
                    put(MediaStore.Downloads.DISPLAY_NAME,"share-entry-${UUID.randomUUID()}.txt");put(MediaStore.Downloads.MIME_TYPE,"text/plain");put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeam/")
                })!!;created.add(uri);context.contentResolver.openOutputStream(uri)!!.use{it.write("generated share fixture".toByteArray())};return uri
            }
            val first=document();val second=document()
            activity=open(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_STREAM,first).putExtra(Intent.EXTRA_TEXT,"caption must not replace the attachment"))
            runner.runOnMainSync{check((field(activity,"sources") as List<*>).size==1);check((field(activity,"message") as EditText).text.isEmpty());check(field(activity,"tab")=="send")}
            val staged=java.io.File(context.cacheDir,"share-entry-stage-${UUID.randomUUID()}").apply{mkdirs()}
            check(Storage.stage(context,first,staged).readText()=="generated share fixture")
            activity=open(Intent(Intent.ACTION_SEND_MULTIPLE).setType("text/plain").putParcelableArrayListExtra(Intent.EXTRA_STREAM,arrayListOf(first,second,first)))
            runner.runOnMainSync{check((field(activity,"sources") as List<*>).size==2)}
            runner.runOnMainSync{runner.callActivityOnNewIntent(activity,Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT,text))}
            runner.waitForIdleSync();checkText(activity)
            runner.runOnMainSync{check((field(activity,"sources") as List<*>).isEmpty())}
            check(TransferService.state(context).optString("id")=="share-test-prior"){"Opening the share sheet started a transfer"}
            check(ProgressEvents.croc("DATABEAM_PREPARE_V1 {\"done\":50,\"total\":100,\"speed_bps\":20}")?.preparation?.done==50L)
            check(ProgressEvents.croc("DATABEAM_PREPARE_V1 {\"done\":200,\"total\":100,\"speed_bps\":20}")==null)
            check(ProgressEvents.croc("DATABEAM_RATE_V1 {\"speed_bps\":-1}")==null)
            result.putString("result","PASS share target, selected text, files/multiple files, attachment caption protection, draft recreation and structured preparation progress")
        }catch(error:Throwable){resultCode=1;result.putString("error",error.stackTraceToString())}
        finally {
            runner.runOnMainSync{opened.forEach{it.finish()}}
            if(original==null)prefs.edit().remove("state").commit() else prefs.edit().putString("state",original).commit()
            created.forEach{runCatching{context.contentResolver.delete(it,null,null)}}
        }
        runner.finish(resultCode,result)
    }
}
