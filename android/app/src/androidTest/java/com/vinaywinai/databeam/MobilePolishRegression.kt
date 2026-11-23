package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.os.Bundle
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.view.*
import android.widget.*
import org.json.*
import java.io.File
import java.util.UUID

/** Generated content only; host also snapshots preferences across process death. */
object MobilePolishRegression {
    fun run(r:Instrumentation,args:Bundle){
        val c=r.targetContext;val prefs=c.getSharedPreferences("transfer",0);val original=prefs.getString("state",null)
        val id=UUID.fromString(args.getString("run")).toString()
        val marker=File(c.filesDir,"polish-$id.json")
        var activity:MainActivity?=null;var session="";var result=1;val out=Bundle()
        val published=mutableListOf<String>()
        fun main(block:()->Unit){var error:Throwable?=null;r.runOnMainSync{try{block()}catch(e:Throwable){error=e}};error?.let{throw it}}
        fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        fun render(s:JSONObject){prefs.edit().putString("state",s.toString()).commit();main{MainActivity::class.java.getDeclaredMethod("render").apply{isAccessible=true}.invoke(activity)}}
        fun find(root:View,text:String):Button?{
            if(root is Button && root.text.toString()==text)return root
            if(root is ViewGroup)for(i in 0 until root.childCount)find(root.getChildAt(i),text)?.let{return it}
            return null
        }
        try{
            check(!TransferService.isBusy()){"A user transfer is active"}
            if(args.getString("relay")!=null)AppSettings.setRelay(c,args.getString("relay")!!)
            activity=r.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            r.waitForIdleSync()
            if(args.getString("polish_file")=="true"){
                AppSettings.setFolder(c,null)
                render(JSONObject().put("id",id).put("action","receive").put("mode","Croc").put("status","complete").put("text",args.getString("ticket")))
                r.waitForIdleSync()
                main{
                    check((field("receiveTextTicket") as View).visibility==View.VISIBLE)
                    (field("receiveTextTicket") as Button).performClick()
                }
                val deadline=System.currentTimeMillis()+60000
                while(System.currentTimeMillis()<deadline){
                    val s=TransferService.state(c)
                    if(s.optString("action")!="receive" || s.optString("ticket")!=args.getString("ticket")){Thread.sleep(100);continue}
                    session=s.optString("id")
                    if(s.optString("status")=="interrupted")error(s.optString("crocFailure",s.optString("message")))
                    if(s.optString("status")=="complete" && !TransferService.isBusy()){
                        val saved=s.getJSONObject("published");check(saved.length()==1)
                        val uri=saved.getString(saved.keys().next());published.add(uri)
                        val hash=java.security.MessageDigest.getInstance("SHA-256")
                        c.contentResolver.openInputStream(android.net.Uri.parse(uri))!!.use{input->val b=ByteArray(65536);while(true){val n=input.read(b);if(n<0)break;hash.update(b,0,n)}}
                        check(hash.digest().joinToString(""){"%02x".format(it)}==args.getString("expectedHash"))
                        check(s.optBoolean("workingCleaned") && !File(s.getString("output")).exists())
                        render(s);main{check((field("openReceived") as View).visibility==View.VISIBLE);(field("openReceived") as Button).performClick()}
                        out.putString("route",s.optString("route"));out.putString("saved","verified-and-cleaned");result=0;break
                    }
                    Thread.sleep(100)
                }
                check(result==0){"File receive timed out"}
            }else if(args.getString("polish_text")!=null){
                val text=when(args.getString("polish_text")){
                    "ticket"->args.getString("ticket")!!
                    "large"->buildString{repeat(14000){append("Unicode 日本語 αβγ · line $it\n")}}
                    else->"\n-- Leading dash\r\nQuotes: \"quoted\" 'single'\nUnicode: 日本語 αβγ\tEnd\n"
                }
                val receiving=args.getString("direction")=="receive"
                val expectedCode="polish-${id.take(8)}"
                main{
                    (field("mode") as Spinner).setSelection(0)
                };r.waitForIdleSync()
                main{
                    if(receiving){
                        (field("receiveTab") as Button).performClick()
                        (field("code") as EditText).setText(expectedCode)
                    }else{
                        (field("textChoice") as Button).performClick()
                        (field("message") as EditText).setText(text)
                        (field("sendCode") as EditText).setText(expectedCode)
                    }
                    (field("primary") as Button).performClick()
                }
                val deadline=System.currentTimeMillis()+90000;var waitingChecked=false
                while(System.currentTimeMillis()<deadline){
                    val s=TransferService.state(c)
                    if(s.optString("code")!=expectedCode){Thread.sleep(100);continue}
                    session=s.optString("id")
                    if((s.optString("phase")=="Waiting for receiver" || receiving && s.optString("status")=="running") && !waitingChecked){
                        check(s.getJSONObject("timing").getLong("elapsedMs")==0L){"Waiting counted as transfer time"}
                        marker.writeText(JSONObject().put("code",s.getString("code")).put("expected",text).put("id",session).toString());waitingChecked=true
                    }
                    if(s.optString("status")=="interrupted")error(s.optString("crocFailure",s.optString("message")))
                    if(s.optString("status")=="complete" && !TransferService.isBusy()){
                        if(receiving){
                            check(s.optString("text")==text){"Received text changed or was truncated"}
                            s.optJSONObject("published")?.let{p->p.keys().forEach{published.add(p.getString(it))}}
                        }else check(s.optString("text").isEmpty() && s.getJSONArray("uris").length()==0){"Successful payload retained"}
                        render(s)
                        main{
                            if(!receiving)check((field("message") as EditText).text.isEmpty())
                            check((field("resend") as View).visibility==View.GONE)
                            check((field("receivedText") as View).visibility==if(receiving)View.VISIBLE else View.GONE)
                            if(!receiving)check((field("detail") as TextView).text.contains("Sent successfully"))
                        }
                        out.putInt("bytes",text.toByteArray().size);result=0;break
                    }
                    Thread.sleep(100)
                }
                check(result==0){"Text transfer timed out"}
            }else{
                main{
                    val quick=field("quickActions") as View;val warning=field("backgroundNotice") as View
                    check((quick.parent as ViewGroup).indexOfChild(warning)<(quick.parent as ViewGroup).indexOfChild(quick))
                    check((quick.layoutParams as ViewGroup.MarginLayoutParams).bottomMargin>0)
                    find(field("receivePanel") as View,"Clear")!!.performClick()
                    check((field("code") as EditText).text.isEmpty())
                    find(field("fileSelectionPanel") as View,"Clear all")!!.performClick()
                    val value="1.getcroc.com:9009 · Reachable\n3.getcroc.com:9009 · Unreachable"
                    val colored=MainActivity::class.java.getDeclaredMethod("relayResultColors",CharSequence::class.java).apply{isAccessible=true}.invoke(activity,value) as Spanned
                    val spans=colored.getSpans(0,colored.length,ForegroundColorSpan::class.java)
                    check(spans.size==2 && spans[0].foregroundColor!=spans[1].foregroundColor)
                }
                val s=JSONObject().put("id",id).put("mode","Sendme").put("action","send").put("status","running")
                    .put("phase","Transferring").put("done",70).put("total",100).put("payloadNames",JSONArray().put("generated-test.bin"))
                TransferRoute.senderEvent(s,"a","relay");render(s)
                TransferRoute.senderEvent(s,"a","direct");render(s)
                main{check((field("route") as TextView).text.contains("Direct"))}
                s.put("status","interrupted").put("message","Generated connection failure");render(s)
                main{
                    check((field("bar") as View).visibility==View.VISIBLE)
                    check((field("resend") as View).visibility==View.GONE)
                    check((field("detail") as TextView).text.contains("reselect the exact same files"))
                }
                s.put("status","complete").put("action","receive").put("saveFolder","")
                    .put("published",JSONObject().put("generated-test.bin","content://test/1"))
                render(s)
                main{check((field("openReceived") as View).visibility==View.VISIBLE)}
                s.put("text","generated clipboard text");render(s)
                main{
                    check((field("copyReceivedText") as View).visibility==View.VISIBLE)
                    check((field("receiveTextTicket") as View).visibility==View.GONE)
                    (field("copyReceivedText") as Button).performClick()
                    check((c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip!!.getItemAt(0).text.toString()=="generated clipboard text")
                    check((field("clearFiles") as View).visibility==View.GONE)
                }
                val filter=IntentFilter(Intent.ACTION_VIEW).apply{addDataScheme("content");addDataType(android.provider.DocumentsContract.Document.MIME_TYPE_DIR)}
                val monitor=Instrumentation.ActivityMonitor(filter,Instrumentation.ActivityResult(0,null),true)
                r.addMonitor(monitor)
                try{main{(field("openReceived") as Button).performClick()};check(monitor.hits==1){"Open folder did not launch a file manager"}}
                finally{r.removeMonitor(monitor)}
                result=0
            }
            out.putString("result","PASS mobile cards, controls or exact Croc text")
        }catch(e:Throwable){out.putString("error",e.stackTraceToString())}
        finally{
            if(session.isNotEmpty() && TransferService.isBusy()){
                c.startService(Intent(c,TransferService::class.java).setAction("stop"))
                val deadline=System.currentTimeMillis()+10000;while(TransferService.isBusy() && System.currentTimeMillis()<deadline)Thread.sleep(100)
            }
            main{activity?.finish()};r.waitForIdleSync()
            published.forEach{c.contentResolver.delete(android.net.Uri.parse(it),null,null)}
            if(original==null)prefs.edit().remove("state").commit() else prefs.edit().putString("state",original).commit()
            if(session.isNotEmpty()){
                UUID.fromString(session)
                File(c.filesDir,"transfer-records/$session.json").delete()
                File(c.filesDir,"transfer-records/$session.tmp").delete()
                for(root in listOf("outgoing","received","blobs")){
                    val parent=File(c.filesDir,root).canonicalFile;val child=File(parent,session).canonicalFile
                    check(child.parentFile==parent);child.deleteRecursively()
                }
            }
        }
        r.finish(result,out)
    }
}
