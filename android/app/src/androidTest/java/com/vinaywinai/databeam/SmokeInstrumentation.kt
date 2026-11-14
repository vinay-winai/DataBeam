package com.vinaywinai.databeam
import android.app.Instrumentation
import android.os.Bundle
import android.content.Intent
import android.util.Base64
import org.json.JSONObject
import org.json.JSONArray
import java.io.File
import java.security.MessageDigest
class SmokeInstrumentation:Instrumentation(){
 private var args=Bundle()
 override fun onCreate(arguments:Bundle?){super.onCreate(arguments);args=arguments ?: Bundle();start()}
 override fun onStart(){
  try {
   if(args.getString("storage_test")=="true"){StorageRegression.run(this);return}
   if(args.getString("share_test")=="true"){ShareEntryRegression.run(this);return}
   if(args.getString("camera_decoder_test")=="true"){CameraQrRegression.run(this);return}
   if(args.getString("relay_test")=="true"){
    NativeBridge.initialize(targetContext)
    val healthy=RelayRecovery.healthy({false})
    val sendme=JSONObject(NativeBridge.checkSendmeRelay())
    finish(0,Bundle().apply{putString("croc",healthy.joinToString());putString("sendme",sendme.toString())});return
   }
   val c=targetContext
   val config=JSONObject(String(Base64.decode(args.getString("config"),Base64.DEFAULT)))
   val activity=startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
   if(config.optBoolean("folderFixture")) {
    val folder=File(c.filesDir,"smoke-folder-${config.getString("id")}").apply{mkdirs()}
    File(folder,"nested/empty directory").mkdirs()
    File(folder,"zero.bin").writeBytes(byteArrayOf())
    File(folder,"databeam_hash.txt").writeText("user-provided file")
    File(folder,"tiny").mkdirs()
    repeat(16){index->File(folder,"tiny/small-$index.bin").writeBytes(ByteArray(4096){7})}
    val block=ByteArray(32768){i->(i%251).toByte()}
    File(folder,"nested/large.bin").outputStream().use{out->repeat(32*128){out.write(block)}}
    val extra=File(c.filesDir,"smoke-extra-${config.getString("id")}.txt").apply{writeText("second selection")}
    config.put("paths",JSONArray().put(folder.absolutePath).put(extra.absolutePath))
   } else if(config.optBoolean("fixture")) {
    val f=File(c.filesDir,"smoke-fixture.bin")
    val block=ByteArray(32768){i->(i%251).toByte()}
    f.outputStream().use{out->repeat(32*config.optInt("fixtureMiB",1).coerceIn(1,1024)){out.write(block)}}
    config.put("paths",JSONArray().put(f.absolutePath))
   }
   c.startForegroundService(Intent(c,TransferService::class.java).putExtra("config",config.toString()))
   val end=System.currentTimeMillis()+1000L*config.optInt("testTimeoutSeconds",120).coerceIn(30,600)
   var didPause=false
   var resumedFrom=-1L
   var last=""
   var lastRevision=""
   var lastNotice=""
   var backgroundAt=0L
   var backgroundBytes=0L
   while(System.currentTimeMillis()<end){
    Thread.sleep(300)
    val s=TransferService.state(c)
    if(s.optString("id")!=config.getString("id"))continue
    val currentPercent=if(s.has("total"))ProgressEvents.percent(s.optLong("done"),s.optLong("total"),s.optInt("filesDone"),s.optInt("filesTotal")) else s.optInt("percent")
    if(config.optBoolean("backgroundTest") && backgroundAt==0L && (s.optLong("done")>=config.optLong("backgroundThreshold",8*1024*1024L) || s.optInt("percent")>=5)){
        backgroundAt=System.currentTimeMillis();backgroundBytes=s.optLong("done")
        for(command in listOf("input keyevent 3","input keyevent 223")) {
            android.os.ParcelFileDescriptor.AutoCloseInputStream(uiAutomation.executeShellCommand(command)).use{it.readBytes()}
        }
        sendStatus(1,Bundle().apply{putString("background_started_bytes",backgroundBytes.toString());putString("background_started_percent",s.optInt("percent").toString())})
    }
    if(config.optBoolean("pauseTest") && !didPause && ((config.optString("mode")=="Croc" && currentPercent in 5..80) || (s.optLong("done")>=8*1024*1024 && s.optLong("done")<s.optLong("total")))) {
        c.startService(Intent(c,TransferService::class.java).setAction("pause"))
        val deadline=System.currentTimeMillis()+20000
        while(TransferService.state(c).optString("status")!="paused" && System.currentTimeMillis()<deadline)Thread.sleep(50)
        val saved=TransferService.state(c);check(saved.optString("status")=="paused"){"Pause did not finish"}
        val cache=File(c.filesDir,if(config.optString("mode")=="Croc")"received/${saved.getString("id")}" else "blobs/${saved.getString("id")}").walkTopDown().filter{it.isFile}.sumOf{it.length()}
        check(cache>0){"Pause discarded the partial cache"}
        sendStatus(1,Bundle().apply{putString("paused_cache_bytes",cache.toString())})
        saved.remove("log")
        if(config.optBoolean("replaceTicket")){
            val replacement=File(c.filesDir,"replacement-${saved.getString("id")}.txt")
            val until=System.currentTimeMillis()+40000
            while(!replacement.exists() && System.currentTimeMillis()<until)Thread.sleep(50)
            check(replacement.exists()){"Replacement ticket was not supplied"}
            val ticket=replacement.readText().trim()
            check(NativeBridge.hash(ticket)==NativeBridge.hash(saved.getString("ticket"))){"Replacement has different content"}
            check(ticket!=saved.getString("ticket")){"Test did not change the ticket"}
            saved.put("ticket",ticket)
        }
        Thread.sleep(1500)
        if(config.optString("mode")=="Croc"){
            resumedFrom=currentPercent.toLong();c.startService(Intent(c,TransferService::class.java).setAction("resume"))
        } else c.startForegroundService(Intent(c,TransferService::class.java).putExtra("config",saved.toString()))
        didPause=true
        continue
    }
    if((didPause || config.optBoolean("requireCachedResume")) && resumedFrom<0){
        val first=s.optString("log").lineSequence().firstOrNull{it.startsWith("receive-progress ")}
        if(first!=null)resumedFrom=first.substringAfter("receive-progress ").substringBefore(':').toLong()
    }
    val revision=s.optString("codeRevision")
    if(revision.isNotEmpty() && revision!=lastRevision){lastRevision=revision;sendStatus(1,Bundle().apply{putString("replacement_code",revision)})}
    val notice=s.optString("relayNotice")
    if(notice.isNotEmpty() && notice!=lastNotice){lastNotice=notice;sendStatus(1,Bundle().apply{putString("relay_notice",notice)})}
    val ticket=s.optString("ticket")
    if(ticket.isNotEmpty() && ticket!=last){last=ticket;sendStatus(1,Bundle().apply{putString("ticket",ticket)})}
    if(s.optString("status") in listOf("complete","interrupted")) {
     if(config.optBoolean("pauseTest")){check(didPause){"Transfer finished before test pause"};check(resumedFrom>0){"Resume downloaded from zero: $resumedFrom"}}
     if(config.optBoolean("requireCachedResume"))check(resumedFrom>0){"Restart did not reuse cached data: $resumedFrom"}
     val out=Bundle();out.putString("state",s.toString());out.putString("resumed_from",resumedFrom.toString())
     if(config.optBoolean("backgroundTest") && s.optString("status")=="complete"){
        check(backgroundAt>0){"Transfer completed before it could enter background"}
        out.putString("background_elapsed_ms",(System.currentTimeMillis()-backgroundAt).toString())
        val power=c.getSystemService(android.content.Context.POWER_SERVICE) as android.os.PowerManager
        check(!power.isInteractive){"Screen did not remain off"}
        out.putString("screen_off","true")
     }
     if(config.getString("action")=="receive" && s.optString("status")=="complete" && s.optString("output").isNotEmpty()) {
        val saved=s.optJSONObject("published") ?: error("Automatic Downloads publication missing")
        check(saved.length()>0 || (s.optJSONObject("publishedDirectories")?.length() ?: 0)>0){"No files or directories saved automatically"}
        val keys=saved.keys()
        while(keys.hasNext()){
            val name=keys.next();val hash=MessageDigest.getInstance("SHA-256")
            c.contentResolver.openInputStream(android.net.Uri.parse(saved.getString(name)))!!.use{input->val buffer=ByteArray(65536);while(true){val n=input.read(buffer);if(n<0)break;hash.update(buffer,0,n)}}
            out.putString("download_hash_${name.substringAfterLast('/')}",hash.digest().joinToString(""){"%02x".format(it)})
        }
     }
     val path=s.optString("output")
     if(path.isNotEmpty()) File(path).walkTopDown().filter{it.isFile}.forEach {f->
      val digest=MessageDigest.getInstance("SHA-256");f.inputStream().use{input->val bytes=ByteArray(65536);while(true){val n=input.read(bytes);if(n<0)break;digest.update(bytes,0,n)}}
      out.putString("hash_${f.name}",digest.digest().joinToString(""){"%02x".format(it)})
     }
     finish(if(s.optString("status")=="complete")0 else 1,out);return
    }
   }
   c.startService(Intent(c,TransferService::class.java).setAction("stop"))
   finish(2,Bundle().apply{putString("error","Timeout");putString("state",TransferService.state(c).toString())})
  }catch(e:Throwable){finish(3,Bundle().apply{putString("error",e.stackTraceToString())})}
 }
}
