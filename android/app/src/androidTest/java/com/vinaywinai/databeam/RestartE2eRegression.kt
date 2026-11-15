package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.view.View
import android.widget.*
import org.json.*
import java.io.File
import java.io.OutputStream
import java.security.MessageDigest
import java.util.UUID

/** Host-coordinated real receiver tests; all fixtures and control files are per-run. */
@android.annotation.TargetApi(29)
object RestartE2eRegression {
    fun run(runner:Instrumentation,args:JSONObject){
        val c=runner.targetContext
        val id=args.getString("run");UUID.fromString(id)
        val directory=File(c.filesDir,"restart-e2e/$id").apply{mkdirs()}
        val recordFile=File(directory,"record.json")
        var record=if(recordFile.exists())JSONObject(recordFile.readText()) else JSONObject()
        var activity:Activity?=null
        var outcome=1;val result=Bundle()
        fun save(){val temp=File(directory,"record.tmp");temp.writeText(record.toString());check(temp.renameTo(recordFile))}
        fun emit(phase:String,s:JSONObject?=null){
            record.put("phase",phase)
            if(s!=null)record.put(phase,JSONObject(s.toString()).apply{remove("log")})
            save();runner.sendStatus(1,Bundle().apply{putString("restart_e2e_step",phase)})
        }
        fun main(block:()->Unit){
            var failure:Throwable?=null
            runner.runOnMainSync{try{block()}catch(error:Throwable){failure=error}}
            failure?.let{throw it}
        }
        fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        fun waitFor(timeout:Long=45000,ignoreInterruptedId:String="",predicate:(JSONObject)->Boolean):JSONObject {
            val end=System.currentTimeMillis()+timeout
            while(System.currentTimeMillis()<end){
                val s=TransferService.state(c)
                if(predicate(s)){Thread.sleep(650);return TransferService.state(c)}
                if(s.optString("status")=="interrupted" && s.optString("id")!=ignoreInterruptedId && s.optString("id") in record.optJSONArray("sessions").let{a->if(a==null)emptyList() else (0 until a.length()).map{a.getString(it)}})error(s.optString("message"))
                Thread.sleep(100)
            }
            error("Timed out: ${TransferService.state(c).optString("status")} / ${TransferService.state(c).optString("phase")}")
        }
        fun remember(s:JSONObject){
            val sessions=record.optJSONArray("sessions") ?: JSONArray().also{record.put("sessions",it)}
            if((0 until sessions.length()).none{sessions.getString(it)==s.getString("id")})sessions.put(s.getString("id"))
            save()
        }
        fun ready(previous:String=""):JSONObject {
            val s=waitFor(ignoreInterruptedId=previous){it.optString("action")=="send" && it.optString("status")=="running" && it.optString("id").isNotEmpty() && it.optString("id")!=previous &&
                (if(record.getString("mode")=="Croc")it.optString("phase")=="Waiting for receiver" && it.optString("code").isNotEmpty() else it.optString("ticket").isNotEmpty())}
            remember(s)
            // Readiness must survive registration, not just a preparation event.
            Thread.sleep(1500)
            check(TransferService.state(c).optString("status")=="running"){TransferService.state(c).optString("message")}
            return TransferService.state(c)
        }
        fun hash(file:File):String {
            val digest=MessageDigest.getInstance("SHA-256")
            file.inputStream().use{input->val b=ByteArray(65536);while(true){val n=input.read(b);if(n<0)break;digest.update(b,0,n)}}
            return digest.digest().joinToString(""){"%02x".format(it)}
        }
        fun writePayload(out:OutputStream,size:Long):String {
            val random=java.util.Random(id.hashCode().toLong());val digest=MessageDigest.getInstance("SHA-256");val block=ByteArray(65536)
            var remaining=size
            out.use{while(remaining>0){random.nextBytes(block);val count=minOf(remaining,block.size.toLong()).toInt();it.write(block,0,count);digest.update(block,0,count);remaining-=count}}
            return digest.digest().joinToString(""){"%02x".format(it)}
        }
        fun stop(){
            val before=TransferService.state(c)
            main{(field("pause") as Button).performClick()}
            waitFor{it.optString("id")==before.optString("id") && it.optString("status") in listOf("stopped","complete")}
            main{check((field("restartSharing") as View).visibility==View.VISIBLE)}
        }
        fun restart():JSONObject {
            val before=TransferService.state(c)
            main{(field("restartSharing") as Button).performClick()}
            val s=ready(before.getString("id"))
            check(s.optString("code")==before.optString("code")){"Short code unexpectedly changed"}
            if(record.getString("mode")!="Croc"){
                check(s.getString("ticket")!=before.getString("ticket")){"Restart reused the old endpoint ticket"}
                check(NativeBridge.hash(s.getString("ticket"))==NativeBridge.hash(before.getString("ticket"))){"Restart changed the collection hash"}
            }
            return s
        }
        try {
            check(TransferService.state(c).optString("status") !in listOf("running","stopping")){"A user transfer is active"}
            if(args.optString("stage","start")=="start"){
                record=JSONObject().put("run",id).put("mode",args.getString("mode")).put("kind",args.getString("kind")).put("scenario",args.getString("scenario")).put("sessions",JSONArray()).put("uris",JSONArray()).put("files",JSONObject()).put("directories",JSONArray())
                val mode=record.getString("mode");val kind=record.getString("kind");val files=record.getJSONObject("files");val uris=record.getJSONArray("uris")
                val paths=JSONArray();val short=id.take(8)
                if(kind=="text")record.put("text","Restarted DataBeam Croc text $short\nUnicode: αβγ 日本語")
                else if(kind=="flat"){
                    fun document(name:String,bytes:Long,text:String?=null){
                        val uri=c.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{put(MediaStore.Downloads.DISPLAY_NAME,name);put(MediaStore.Downloads.MIME_TYPE,"application/octet-stream");put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeam/")})!!
                        uris.put(uri.toString());save()
                        val digest=if(text==null)writePayload(c.contentResolver.openOutputStream(uri)!!,bytes) else {
                            val data=text.toByteArray();c.contentResolver.openOutputStream(uri)!!.use{it.write(data)};MessageDigest.getInstance("SHA-256").digest(data).joinToString(""){"%02x".format(it)}
                        }
                        files.put(name,JSONObject().put("sha256",digest).put("size",if(text==null)bytes else text.toByteArray().size))
                    }
                    document("restart-$short.bin",args.optLong("mib",8)*1024*1024)
                    document("empty-$short.bin",0)
                    document("notes-$short-α.txt",0,"Generated restart fixture\nUnicode: αβγ\n")
                } else if(kind=="provider") {
                    c.contentResolver.delete(Uri.parse("content://${SourceAccessRegression.AUTHORITY}/reset"),null,null)
                    uris.put(android.provider.DocumentsContract.buildTreeDocumentUri(SourceAccessRegression.AUTHORITY,"root").toString())
                    val data=ByteArray(65536).also{java.util.Random(7).nextBytes(it)}
                    val digest=MessageDigest.getInstance("SHA-256").digest(data).joinToString(""){"%02x".format(it)}
                    for(name in listOf("source-folder/original-name.bin","source-folder/nested/stream.bin","source-folder/nested/bounded.bin"))files.put(name,JSONObject().put("sha256",digest).put("size",data.size))
                    record.getJSONArray("directories").put("source-folder/empty directory")
                } else {
                    val source=File(directory,"source").apply{mkdirs()};val folder=File(source,"folder-$short").apply{mkdirs()}
                    File(folder,"nested").mkdirs();File(folder,"empty directory").mkdirs()
                    val payload=File(folder,"nested/payload.bin");writePayload(payload.outputStream(),args.optLong("mib",8)*1024*1024)
                    File(folder,"zero.bin").writeBytes(byteArrayOf());File(folder,"nested/notes-α.txt").writeText("Generated folder restart fixture αβγ\n")
                    val extra=File(source,"extra-$short.bin");writePayload(extra.outputStream(),128*1024)
                    paths.put(folder.absolutePath).put(extra.absolutePath)
                    for(file in source.walkTopDown().filter{it.isFile})files.put(file.relativeTo(source).invariantSeparatorsPath,JSONObject().put("sha256",hash(file)).put("size",file.length()))
                    record.getJSONArray("directories").put("${folder.name}/empty directory")
                }
                record.put("paths",paths);save()
                AppSettings.setRelay(c,args.getString("relay"))
                activity=runner.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                runner.waitForIdleSync()
                main{(field("mode") as Spinner).setSelection(listOf("EasySendMe","Sendme","Croc").indexOf(mode))}
                runner.waitForIdleSync()
                if(kind=="folder"){
                    val config=JSONObject().put("id",UUID.randomUUID().toString()).put("mode",mode).put("action","send").put("code","").put("text","").put("uris",JSONArray()).put("paths",paths).put("relay",args.getString("relay"))
                    c.startForegroundService(Intent(c,TransferService::class.java).putExtra("config",config.toString()))
                } else {
                    main{
                        if(kind in listOf("flat","provider"))runner.callActivityOnNewIntent(activity,Intent(Intent.ACTION_SEND_MULTIPLE).setType("*/*").putParcelableArrayListExtra(Intent.EXTRA_STREAM,ArrayList((0 until uris.length()).map{Uri.parse(uris.getString(it))})))
                        else{(field("textChoice") as Button).performClick();(field("message") as EditText).setText(record.getString("text"))}
                        (field("sendCode") as EditText).setText("");(field("primary") as Button).performClick()
                    }
                }
                val first=ready()
                if(kind=="flat"){
                    check(first.optInt("referencedFiles")==3 && first.optInt("copiedFiles")==0){"Seekable file selections were copied instead of referenced"}
                    check(TransferCleanup.size(File(c.filesDir,"outgoing/${first.getString("id")}"))==0L){"Outgoing payload copies consumed storage"}
                    check(TransferCleanup.size(File(c.filesDir,"blobs/${first.getString("id")}"))<args.optLong("mib",8)*1024*1024/2){"Sender blob store duplicated the original payload"}
                }
                if(kind=="provider")check(first.optInt("referencedFiles")==0 && first.optInt("copiedFiles")==3){"Expected three safe provider fallbacks"}
                if(mode!="Croc")record.put("first_hash",NativeBridge.hash(first.getString("ticket")))
                emit("prepared",first)
            } else {
                activity=runner.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                runner.waitForIdleSync();Thread.sleep(750)
                val prior=TransferService.state(c)
                check(prior.optString("status")=="interrupted"){"Closed sender did not restore as interrupted"}
                main{check((field("status") as TextView).text.toString()=="Sharing stopped"){"Unexpected restored heading: ${(field("status") as TextView).text}"}}
                val next=restart()
                if(record.getString("mode")!="Croc")check(NativeBridge.hash(next.getString("ticket"))==record.getString("first_hash"))
                emit("restarted",next)
            }
            var lastCommand=-1
            val deadline=System.currentTimeMillis()+360000
            val commandFile=File(directory,"command.json")
            while(System.currentTimeMillis()<deadline){
                val command=runCatching{JSONObject(commandFile.readText())}.getOrNull()
                if(command!=null && command.optInt("sequence")!=lastCommand){
                    lastCommand=command.getInt("sequence")
                    when(command.getString("op")){
                        "restart"->{
                            val direct=TransferService.state(c).optBoolean("referencedSources")
                            stop()
                            if(record.getString("kind") in listOf("flat","provider") && !direct){
                                val uris=record.getJSONArray("uris");for(i in 0 until uris.length())c.contentResolver.delete(Uri.parse(uris.getString(i)),null,null)
                                record.put("originals_deleted",true)
                            }
                            if(direct)record.put("originals_kept_for_reference",true)
                            emit("restarted",restart())
                        }
                        "finish"->{
                            check(command.optBoolean("verified")){"Host did not verify the received payload"}
                            if(record.getString("mode")=="Croc")waitFor{it.optString("status")=="complete"}
                            else{
                                val sent=waitFor{TransferLabels.sentAndSharing(it)}
                                val elapsed=TransferTiming.elapsedText(sent);Thread.sleep(1800)
                                check(TransferTiming.elapsedText(TransferService.state(c))==elapsed){"Completed elapsed time kept counting idle sharing"}
                                val notice=c.getSystemService(NotificationManager::class.java).activeNotifications.firstOrNull{it.id==1}?.notification ?: error("Sharing notification missing")
                                check(notice.extras.getCharSequence(Notification.EXTRA_TEXT).toString().contains("Sent")){"Notification still reports waiting after completion"}
                                main{activity?.finish()}
                                activity=runner.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                                runner.waitForIdleSync()
                                main{
                                    MainActivity::class.java.getDeclaredMethod("render").apply{isAccessible=true}.invoke(activity)
                                    val heading=(field("status") as TextView).text.toString()
                                    check(heading=="Sent"){"Unexpected completed heading: $heading"}
                                    val path=(field("route") as TextView).text.toString()
                                    check(path.startsWith("Connection used") && (path.contains("Direct") || path.contains("Relay"))){"Last transfer path disappeared: $path"}
                                    val count=record.getJSONArray("uris").length().takeIf{it>0} ?: record.getJSONArray("paths").length()
                                    check((field("sources") as List<*>).size==count){"Reopened activity lost the shared file list"}
                                }
                                stop()
                                check(TransferService.state(c).optString("status")=="complete"){"Ending successful sharing erased completion"}
                            }
                            if(record.getString("kind")!="text") {
                                val done=TransferService.state(c)
                                check(done.getJSONObject("timing").getLong("elapsedMs")>0){"Sender elapsed time missing"}
                                check(TransferTiming.average(done)?.let{it.isFinite() && it>0}==true){"Sender average missing"}
                                main{
                                    MainActivity::class.java.getDeclaredMethod("render").apply{isAccessible=true}.invoke(activity)
                                    val shown=(field("detail") as TextView).text
                                    check(shown.contains("Elapsed") && shown.contains("Avg upload")){"Sender timing summary not displayed"}
                                }
                            }
                            record.put("verified",true);emit("passed",TransferService.state(c));outcome=0;break
                        }
                        "abort"->error("Host aborted the test")
                    }
                }
                Thread.sleep(100)
            }
            check(outcome==0){"Host coordination timed out"}
            result.putString("result","PASS ${record.getString("mode")} ${record.getString("scenario")} receiver payload verified")
        }catch(error:Throwable){
            record.put("error",error.stackTraceToString());emit("failed",TransferService.state(c));result.putString("error",error.stackTraceToString())
        }finally{
            val current=TransferService.state(c)
            val own=record.optJSONArray("sessions")?.let{a->(0 until a.length()).any{a.getString(it)==current.optString("id")}} ?: false
            if(own && current.optString("status") in listOf("running","stopping")){
                c.startService(Intent(c,TransferService::class.java).setAction("stop"))
                val until=System.currentTimeMillis()+15000
                while(System.currentTimeMillis()<until && TransferService.state(c).optString("status") in listOf("running","stopping"))Thread.sleep(100)
            }
            main{activity?.finish()}
        }
        runner.finish(outcome,result)
    }
}
