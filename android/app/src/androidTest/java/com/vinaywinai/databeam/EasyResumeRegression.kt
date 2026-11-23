package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.os.Bundle
import android.view.*
import android.widget.*
import org.json.JSONObject
import java.io.File
import java.util.UUID

/** Real main/card buttons, real one-shot Croc handoffs, and a restarted native sender. */
object EasyResumeRegression {
    fun run(r:Instrumentation,args:Bundle){
        val c=r.targetContext;val id=UUID.fromString(args.getString("run")).toString()
        val marker=File(c.filesDir,"easy-resume-$id.json");val ready=File(c.filesDir,"easy-ready-$id.json")
        val prefs=c.getSharedPreferences("transfer",0);val original=prefs.getString("state",null)
        var activity:MainActivity?=null;var session="";var exit=1;val out=Bundle();var last=JSONObject()
        val sessions=mutableListOf<String>()
        val oneShot=args.getString("one_shot")=="true"
        val nativeMode=args.getString("mode") ?: "EasySendMe"
        fun mark(stage:String){marker.writeText(JSONObject().put("session",session).put("sessions",org.json.JSONArray(sessions)).put("stage",stage).toString())}
        fun main(block:()->Unit){var failure:Throwable?=null;r.runOnMainSync{try{block()}catch(e:Throwable){failure=e}};failure?.let{throw it}}
        fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        fun render(){main{MainActivity::class.java.getDeclaredMethod("render").apply{isAccessible=true}.invoke(activity)}}
        fun waitFor(seconds:Int=35,allowInterrupted:Boolean=false,predicate:(JSONObject)->Boolean):JSONObject{
            val end=System.currentTimeMillis()+seconds*1000L
            while(System.currentTimeMillis()<end){
                last=TransferService.state(c)
                if(last.optString("id")==session){
                    val busy=TransferService.isBusy()
                    check(!last.optBoolean("awaitingCacheChoice")){"Receive asked for cache confirmation"}
                    if(predicate(last))return last
                    check(last.optString("status") !in listOf("complete","interrupted") || busy || (allowInterrupted && last.optString("status")=="interrupted")){"Unexpected ${last.optString("status")}: ${last.optString("message")}"}
                }
                Thread.sleep(50)
            }
            error("Timed out at ${last.optString("phase")}: ${last.optString("message")}")
        }
        fun pause():JSONObject{
            render();main{(field("pause") as Button).performClick()}
            val saved=waitFor{it.optString("status")=="paused" && !TransferService.isBusy()}
            check(saved.optString("ticket").isNotEmpty());render()
            main{check((field("resumeButton") as View).visibility==View.VISIBLE)}
            return saved
        }
        fun resumeAndAdvance(before:JSONObject,primary:Boolean=false):JSONObject{
            render();main{
                // The card must ignore an unrelated draft; main Receive uses its input.
                (field("code") as EditText).setText(if(primary)before.getString("code") else "unrelated-draft-code")
                (field(if(primary)"primary" else "resumeButton") as Button).performClick()
                if(!primary){
                    val dialog=field("resumeDialog") as AlertDialog
                    check(dialog.findViewById<TextView>(android.R.id.message).text.contains("exact same files"))
                    dialog.getButton(DialogInterface.BUTTON_NEUTRAL).performClick()
                }
            }
            // startForegroundService is asynchronous: the old interrupted card
            // can remain persisted briefly before onStartCommand commits running.
            val startedBy=System.currentTimeMillis()+5000
            while(System.currentTimeMillis()<startedBy){
                if(TransferService.isBusy() && TransferService.state(c).optString("status")=="running")break
                Thread.sleep(20)
            }
            val active=waitFor{it.optString("status")=="running" && it.has("firstPayloadDone") && it.optLong("done")>before.optLong("done")+4*1024*1024}
            check(active.optString("ticket")==before.optString("ticket")){"Resume replaced the saved endpoint"}
            check(active.optLong("firstPayloadDone")>0){"Resume restarted progress from zero"}
            check(active.optLong("cachedReceiveBytes")>0){"Resume did not find saved bytes"}
            check(!active.optBoolean("refresh")){"Refresh flag survived a ticket lookup"}
            out.putString("resumed_${if(primary)"main" else "card"}_${before.optLong("done")}",active.getLong("firstPayloadDone").toString())
            return active
        }
        try{
            check(!TransferService.isBusy())
            AppSettings.setEngineEnabled(c,"Sendme",true);AppSettings.setEngineEnabled(c,"Croc",true)
            AppSettings.setRelay(c,"127.0.0.1:19009");AppSettings.setFolder(c,null)
            activity=r.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            main{activity!!.selectEngineForTest(nativeMode);(field("receiveTab") as Button).performClick();(field("code") as EditText).setText(args.getString(if(nativeMode=="Sendme")"ticket" else "code"));(field("primary") as Button).performClick()}
            val start=System.currentTimeMillis()+10000
            while(System.currentTimeMillis()<start){val s=TransferService.state(c);if(s.optString(if(nativeMode=="Sendme")"ticket" else "code")==args.getString(if(nativeMode=="Sendme")"ticket" else "code")){session=s.getString("id");break};Thread.sleep(50)}
            check(session.isNotEmpty());sessions.add(session);mark("started")
            waitFor{it.optLong("done")>=8*1024*1024 && it.optLong("done")<it.optLong("total")}
            val first=pause()
            val second=if(oneShot)first else {resumeAndAdvance(first);pause()}
            mark("restart-sender")
            val end=System.currentTimeMillis()+35000
            while(!ready.exists() && System.currentTimeMillis()<end)Thread.sleep(100)
            check(ready.exists()){"Host did not restart sharing"};val replacement=JSONObject(ready.readText())
            main{
                (field("resumeButton") as Button).performClick()
                (field("resumeDialog") as AlertDialog).getButton(DialogInterface.BUTTON_POSITIVE).performClick()
            }
            r.waitForIdleSync()
            main{
                val dialog=field("reconnectDialog") as AlertDialog
                fun entry(v:View):EditText?{if(v is EditText)return v;if(v is ViewGroup)for(i in 0 until v.childCount)entry(v.getChildAt(i))?.let{return it};return null}
                entry(dialog.window!!.decorView)!!.setText(replacement.getString(if(nativeMode=="Sendme")"ticket" else "code"))
                dialog.getButton(DialogInterface.BUTTON_POSITIVE).performClick()
            }
            val fresh=waitFor{it.optString("ticket")==replacement.getString("ticket") && it.has("firstPayloadDone") && it.optLong("done")>second.optLong("done")+4*1024*1024}
            check(fresh.getString("ticket")!=first.getString("ticket"));check(NativeBridge.hash(fresh.getString("ticket"))==NativeBridge.hash(first.getString("ticket")))
            check(!fresh.optBoolean("refresh") && fresh.optLong("firstPayloadDone")>0)
            if(!oneShot){
            val third=pause();resumeAndAdvance(third) // No second Croc sender exists now.
            val fourth=pause();resumeAndAdvance(fourth,primary=true)
            NativeBridge.cancel() // Simulate an interrupted native receive without a user pause.
            val failed=waitFor(allowInterrupted=true){it.optString("status")=="interrupted" && !TransferService.isBusy()}
            resumeAndAdvance(failed)
            pause();ready.delete();mark("fresh-receive")
            val freshBy=System.currentTimeMillis()+25000
            while(!ready.exists() && System.currentTimeMillis()<freshBy)Thread.sleep(100)
            check(ready.exists());val nextCode=JSONObject(ready.readText()).getString("code")
            render();main{
                (field("newTransfer") as Button).performClick()
                activity!!.selectEngineForTest("EasySendMe")
                (field("code") as EditText).setText(nextCode)
                (field("primary") as Button).performClick()
            }
            val newBy=System.currentTimeMillis()+5000
            while(System.currentTimeMillis()<newBy){val s=TransferService.state(c);if(s.optString("code")==nextCode && s.optString("id")!=session){session=s.getString("id");sessions.add(session);mark("fresh-started");break};Thread.sleep(20)}
            check(sessions.size==2){"Fresh receive did not create a new card"}
            val freshActive=waitFor{it.has("firstPayloadDone") && it.optLong("cachedReceiveBytes")>0}
            check(freshActive.getLong("firstPayloadDone")>0){"Fresh Receive ignored the previous card's cached data"}
            out.putString("fresh_receive_from",freshActive.getLong("firstPayloadDone").toString())
            }
            val complete=waitFor(75){it.optString("status")=="complete" && !TransferService.isBusy()}
            val published=complete.getJSONObject("published");check(published.length()==1)
            val digest=java.security.MessageDigest.getInstance("SHA-256")
            c.contentResolver.openInputStream(android.net.Uri.parse(published.getString(published.keys().next())))!!.use{input->val b=ByteArray(65536);while(true){val n=input.read(b);if(n<0)break;digest.update(b,0,n)}}
            check(digest.digest().joinToString(""){"%02x".format(it)}==args.getString("sha256")){"Saved file hash mismatch"}
            check(complete.optBoolean("workingCleaned"));out.putString("result",if(oneShot)"PASS $nativeMode normal desktop sender pause, replacement session, automatic cached continuation and SHA-256" else "PASS EasySendMe card/main resume, one-shot refresh, sender restart, interruption, automatic cache continuation and SHA-256");exit=0
        }catch(e:Throwable){out.putString("error",e.stackTraceToString())}
        finally{
            if(session.isNotEmpty() && TransferService.isBusy()){
                c.startService(Intent(c,TransferService::class.java).setAction("stop"))
                val end=System.currentTimeMillis()+10000;while(TransferService.isBusy() && System.currentTimeMillis()<end)Thread.sleep(50)
            }
            if(session.isNotEmpty()){
                val saved=TransferService.state(c);marker.writeText(saved.put("stage","finished").put("sessions",org.json.JSONArray(sessions)).toString())
                saved.optJSONObject("published")?.let{p->p.keys().forEach{key->c.contentResolver.delete(android.net.Uri.parse(p.getString(key)),null,null)}}
            }
            main{activity?.finish()};r.waitForIdleSync()
            if(original==null)prefs.edit().remove("state").commit() else prefs.edit().putString("state",original).commit()
        }
        r.finish(exit,out)
    }
}
