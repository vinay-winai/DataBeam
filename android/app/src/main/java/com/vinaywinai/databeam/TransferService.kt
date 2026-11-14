package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.content.pm.ServiceInfo
import android.os.*
import org.json.*
import java.io.File
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicBoolean

class TransferService : Service() {
    companion object {
        private val busy = AtomicBoolean(false)
        const val CHANNEL = "transfers"
        fun state(c: Context): JSONObject {
            val wasBusy=busy.get()
            val prefs=c.getSharedPreferences("transfer",0)
            val saved=JSONObject(prefs.getString("state","{}")!!)
            if(!wasBusy && !busy.get() && (saved.optString("status") in listOf("running","stopping") || (saved.optString("status")=="paused" && saved.optBoolean("connectionRetained")))) {
                saved.put("connectionRetained",false).put("status","interrupted").put("message","App restarted; saved data retained. Press Resume to reconnect.")
                prefs.edit().putString("state",saved.toString()).commit()
            }
            return saved
        }
    }
    private val networkCallback=object:android.net.ConnectivityManager.NetworkCallback(){
        override fun onAvailable(network:android.net.Network){NativeBridge.networkChanged()}
        override fun onLost(network:android.net.Network){NativeBridge.networkChanged()}
        override fun onLinkPropertiesChanged(network:android.net.Network,properties:android.net.LinkProperties){NativeBridge.networkChanged()}
    }
    override fun onCreate() {
        super.onCreate();NativeBridge.initialize(applicationContext)
        getSystemService(android.net.ConnectivityManager::class.java).registerDefaultNetworkCallback(networkCallback)
    }
    private val main = Handler(Looper.getMainLooper())
    private var process: java.lang.Process? = null
    @Volatile private var crocPid:Int?=null
    private val stopping = AtomicBoolean(false)
    private var wake: PowerManager.WakeLock? = null
    private var multicast: android.net.wifi.WifiManager.MulticastLock? = null
    private var state = JSONObject()
    private var job: Thread? = null
    private var lastProgressNotice=0L
    private val poll = object : Runnable {
        override fun run() {
            runCatching {
                val events = JSONArray(NativeBridge.poll())
                for (i in 0 until events.length()) {
                    val e=events.getJSONObject(i)
                    val kind=e.getString("kind"); val payload=e.getString("payload")
                    if(kind=="receive-route")synchronized(this@TransferService){state.put("route",payload)}
                    if(kind=="send-route")synchronized(this@TransferService){
                        val event=JSONObject(payload);val routes=state.optJSONObject("senderRoutes") ?: JSONObject()
                        if(event.optString("route")=="closed")routes.remove(event.getString("connection")) else routes.put(event.getString("connection"),event.getString("route"))
                        state.put("senderRoutes",routes)
                    }
                    if(kind=="ticket") synchronized(this@TransferService) { state.put("ticket",payload) }
                    if(kind in listOf("receive-progress","transfer-progress","preparation-progress")) {
                        ProgressEvents.native(payload)?.let{sample->synchronized(this@TransferService) {
                            if(kind=="preparation-progress")state.put("preparing",true).put("prepDone",sample.done).put("prepTotal",sample.total).put("prepSpeed",sample.speed).put("phase","Preparing files")
                            else state.put("preparing",false).put("done",sample.done).put("total",sample.total).put("speed",sample.speed).put("phase","Transferring")
                            state.put("progress","")
                        }}
                    }
                    if(kind=="ticket")synchronized(this@TransferService){
                        if(state.optString("phase")!="Transferring")state.put("preparing",false).put("phase","Waiting for receiver")
                    }
                    if(kind=="active-connection-count" && payload=="0")synchronized(this@TransferService){
                        if(state.optString("action")=="send")state.put("phase","Waiting for receiver").put("speed",0)
                    }
                    if(kind=="receive-export-started")synchronized(this@TransferService){state.put("preparing",false).put("phase","Verifying and saving").put("speed",0)}
                    if(kind=="receive-started")synchronized(this@TransferService){state.put("phase","Connecting").put("message","")}
                    if(kind=="receive-progress")synchronized(this@TransferService){state.put("message","")}
                    log("$kind $payload")
                }
                val now=SystemClock.elapsedRealtime()
                if(busy.get() && now-lastProgressNotice>=1000){lastProgressNotice=now;getSystemService(NotificationManager::class.java).notify(1,notification(""))}
            }
            if(busy.get()) main.postDelayed(this,300)
        }
    }
    override fun onBind(i: Intent?) = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if(intent?.action=="resume" && state.optString("status")=="paused" && process!=null && crocPid!=null) {
            android.system.Os.kill(crocPid!!,android.system.OsConstants.SIGCONT)
            synchronized(this){state.put("connectionRetained",false)};update("running","Continuing transfer…");return START_NOT_STICKY
        }
        if(intent?.action=="pause" && state.optString("action")=="receive" && state.optString("mode")=="Croc" && process!=null && crocPid!=null) {
            android.system.Os.kill(crocPid!!,android.system.OsConstants.SIGSTOP)
            synchronized(this){state.put("connectionRetained",true)};update("paused","Paused · connection retained");return START_NOT_STICKY
        }
        if(intent?.action in listOf("stop","pause")) {
            if(!busy.get()){stopSelf();return START_NOT_STICKY}
            stopping.set(true)
            synchronized(this){state.put("connectionRetained",false)}
            runCatching { NativeBridge.cancel() }; crocPid?.let{runCatching{android.system.Os.kill(it,android.system.OsConstants.SIGCONT)}};process?.destroy()
            update("stopping",if(state.optString("action")=="send")"Ending sharing…" else "Pausing · saving received data…")
            return START_NOT_STICKY
        }
        if(!busy.compareAndSet(false,true)) return START_NOT_STICKY
        stopping.set(false)
        NativeBridge.prepare()
        state=JSONObject(intent?.getStringExtra("config") ?: "{}")
        if(!state.has("saveFolder"))state.put("saveFolder",AppSettings.folder(this)).put("saveFolderLabel",AppSettings.folderLabel(this))
        state.remove("route");state.remove("senderRoutes")
        state.put("preparing",false).put("speed",0).put("prepSpeed",0)
        if(state.optString("mode")=="Croc"){for(key in listOf("overallProgress","done","total","filesDone","filesTotal"))state.remove(key)}
        if(state.optString("action")=="send") state.remove("ticket")
        state.put("connectionRetained",false);state.remove("phase");state.remove("relayNotice");state.remove("suggestSendme");state.remove("troubleshooting");state.remove("codeRevision")
        val nm=getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL,"Transfers",NotificationManager.IMPORTANCE_LOW))
        if(Build.VERSION.SDK_INT>=29) startForeground(1, notification("Preparing transfer"), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC) else startForeground(1, notification("Preparing transfer"))
        wake=(getSystemService(POWER_SERVICE) as PowerManager).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"DataBeam:transfer").apply { acquire(6*60*60*1000L) }
        update("running","Preparing…")
        main.post(poll)
        job=Thread {
            var terminal="complete"
            var terminalMessage="Complete"
            try {
                val config=synchronized(this) {JSONObject(state.toString())}
                stageSources(config)
                synchronized(this) { state.put("paths",config.optJSONArray("paths")) }
                persist()
                check(!stopping.get()) {"Stopped"}
                val mode=config.getString("mode")
                if(mode=="Croc") {
                    val out=croc(config,config.getString("action"),null)
                    if(config.getString("action")=="receive") synchronized(this) {state.put("text",out)}
                } else if(mode=="EasySendMe") {
                    if(config.getString("action")=="send") {
                        var advertised=false
                        // Share must run concurrently with the short-code ticket exchange.
                        var nativeError: Throwable? = null
                        val native=Thread { try { runNative(config) } catch(e:Throwable){nativeError=e} }
                        native.start()
                        try {
                            while(native.isAlive && !stopping.get()) {
                                val ticket=synchronized(this){state.optString("ticket")}
                                if(ticket.isNotEmpty() && !advertised) {
                                    croc(config,"send",ticket);advertised=true
                                    update("running","Sharing; receiver can reconnect while this session stays open")
                                }
                                Thread.sleep(100)
                            }
                        } finally {
                            runCatching { NativeBridge.cancel() };native.join()
                        }
                        nativeError?.let { throw it }
                    } else {
                        // Prefer a saved ticket for an intact live sender. On failure the user
                        // can refresh it via the short code without discarding cached chunks.
                        var ticket=config.optString("ticket")
                        if(ticket.isEmpty() || config.optBoolean("refresh")) {
                            val previous=ticket
                            ticket=croc(config,"receive",null).split(Regex("\\s+")).firstOrNull { NativeBridge.hash(it).isNotEmpty() } ?: error("Sender did not supply a valid Sendme ticket")
                            require(previous.isEmpty() || NativeBridge.hash(previous)==NativeBridge.hash(ticket)){"This code now refers to different content. Start a new transfer."}
                            require(ticket.startsWith("blob")) {"Sender did not supply a Sendme ticket"}
                        }
                        config.put("ticket",ticket)
                        synchronized(this){state.put("ticket",ticket)}
                        runNative(config)
                    }
                } else runNative(config)
                check(!stopping.get()){ "Paused" }
                AppSettings.remember(this,config.getString("mode"),config.getString("action"),config.optString("code"))
                if(config.getString("action")=="receive") {
                    val output=synchronized(this){state.optString("output")}
                    if(output.isNotEmpty() && File(output).listFiles()?.isNotEmpty()==true) {
                        synchronized(this){state.put("phase","Saving files")};persist()
                        val published=synchronized(this){state.optJSONObject("published") ?: JSONObject()}
                        val directories=synchronized(this){state.optJSONObject("publishedDirectories") ?: JSONObject()}
                        Storage.publishDownloads(this,File(output),published,{stopping.get()},config.optString("saveFolder"),directories,{_,_->
                            synchronized(this){state.put("publishedDirectories",directories)};persist()
                        }){_,_->
                            synchronized(this){state.put("published",published)};persist()
                        }
                        terminalMessage="Saved to ${config.optString("saveFolderLabel","Downloads / DataBeam")}"
                    } else terminalMessage="Text received"
                }
            } catch(e:Throwable) {
                terminal=if(stopping.get()) if(state.optString("action")=="send")"stopped" else "paused" else "interrupted"
                terminalMessage=if(stopping.get()) if(state.optString("action")=="send")"Sharing ended" else "Paused · received data retained" else (e.message ?: e.javaClass.simpleName)
            } finally {
                main.post {
                    main.removeCallbacks(poll)
                    wake?.let { if(it.isHeld) it.release() };multicast?.let{if(it.isHeld)it.release()}
                    update(terminal,terminalMessage)
                    busy.set(false)
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelfResult(startId)
                }
            }
        }.apply { name="DataBeam transfer";start() }
        return START_NOT_STICKY
    }
    private fun stageSources(c:JSONObject) {
        if(c.getString("action")!="send" || c.optString("text").isNotEmpty()) return
        val old=c.optJSONArray("paths")
        if(old!=null && old.length()>0) {
            for(i in 0 until old.length()) require(File(old.getString(i)).exists()) {"Original files are missing. Select them again."}
            return
        }
        val uris=c.optJSONArray("uris") ?: error("Select files or a folder")
        synchronized(this){state.put("phase","Copying selected files").put("preparing",true).put("prepDone",0).put("prepTotal",0)};persist()
        val folder=File(filesDir,"outgoing/${c.getString("id")}").apply{mkdirs()}
        val paths=JSONArray()
        for(i in 0 until uris.length()) {
            val file=Storage.stage(this,android.net.Uri.parse(uris.getString(i)),folder)
            paths.put(file.absolutePath)
        }
        require(paths.length()>0){"No files selected"};c.put("paths",paths)
    }
    private fun runNative(c:JSONObject) {
        check(!stopping.get()){ "Stopped" }
        c.put("cache",File(filesDir,"blobs/${c.getString("id")}").apply{mkdirs()}.absolutePath)
        c.put("output",File(filesDir,"received/${c.getString("id")}").apply{mkdirs()}.absolutePath)
        synchronized(this){state.put("output",c.getString("output"))}
        c.put("root","DataBeam_databeam")
        val result=JSONObject(NativeBridge.run(c.toString()))
        if(result.optString("status")=="error") error(result.optString("message"))
        synchronized(this) { state.put("output",c.getString("output")) }
    }
    private fun croc(c:JSONObject,action:String,text:String?):String {
        return crocAttempt(c,action,text,true)
    }
    private fun crocAttempt(c:JSONObject,action:String,text:String?,allowRecovery:Boolean):String {
        val code=c.optString("code").ifEmpty {
            val bytes=ByteArray(10);SecureRandom().nextBytes(bytes)
            bytes.joinToString(""){"%02x".format(it)}.also { c.put("code",it);synchronized(this){state.put("code",it)};persist() }
        }
        require(code.length>=7){"Code must have at least 7 characters"}
        val relay=c.optString("relay").ifEmpty { RelayRecovery.relay(code) }
        val output=File(filesDir,"received/${c.getString("id")}").apply{mkdirs()}
        val args=mutableListOf(File(applicationInfo.nativeLibraryDir,"libcroc.so").absolutePath,"--yes","--ignore-stdin","--relay",relay)
        if(action=="send") {
            args.add("send")
            val message=text ?: c.optString("text").takeIf{it.isNotEmpty()}
            if(message!=null) args.addAll(listOf("--text",message)) else {
                args.add("--");val paths=c.getJSONArray("paths")
                for(i in 0 until paths.length())args.add(paths.getString(i))
            }
        } else args.addAll(listOf("--out",output.absolutePath))
        val command=listOf("/system/bin/sh","-c","echo DATABEAM_CHILD_PID=$$; exec \"$@\"","databeam-croc")+args
        val builder=ProcessBuilder(command).directory(output)
        builder.environment().putAll(mapOf("CROC_SECRET" to code,"HOME" to filesDir.absolutePath,"TMPDIR" to cacheDir.absolutePath,"DATABEAM_CROC_PROGRESS" to "1","CROC_NO_UPDATE_CHECK" to "1"))
        val p=builder.start();process=p;if(stopping.get())p.destroy();p.outputStream.close()
        val errors=StringBuffer()
        val payloadStarted=AtomicBoolean(false)
        fun observe(line:String) {
            if(c.optString("mode")=="Croc")ProgressEvents.croc(line)?.let{event->
                synchronized(this){
                    event.preparation?.let{sample->state.put("preparing",true).put("phase","Preparing files").put("prepDone",sample.done).put("prepTotal",sample.total).put("prepSpeed",sample.speed)}
                    event.speed?.let{if(!state.optBoolean("overallProgress"))state.put("speed",it)}
                    if(event.verifying)state.put("preparing",false).put("phase","Verifying files").put("speed",0)
                    event.transfer?.let{sample->state.put("overallProgress",true).put("preparing",false).put("phase","Transferring").put("done",sample.done).put("total",sample.total).put("speed",sample.speed).put("filesDone",event.filesDone).put("filesTotal",event.filesTotal).put("progress","")}
                    if(event.prepared){state.put("preparing",false).put("phase","Waiting for receiver").put("speed",0);state.remove("percent");state.put("progress","")}
                };persist();return
            }
            if(c.optString("mode")=="Croc")TransferRoute.croc(line)?.let{route->synchronized(this){state.put("route",route)};persist()}
            if(c.optString("mode")=="Croc")ProgressEvents.crocPhase(line)?.let{phase->synchronized(this){state.put("phase",phase).put("preparing",false)};persist()}
            val percent=Regex("(\\d{1,3})%.*?\\(([^)]*)\\)").find(line)
            if(percent!=null && c.optString("mode")=="Croc" && !line.trimStart().startsWith("Hashing ")){synchronized(this){if(!state.optBoolean("overallProgress"))state.put("preparing",false).put("phase","Transferring").put("percent",percent.groupValues[1].toInt().coerceIn(0,100)).put("progress",percent.groupValues[2])};persist()}
            if(line.startsWith("Sending (") || line.startsWith("Receiving ("))payloadStarted.set(true)
            synchronized(errors){errors.append(line).append('\n');if(errors.length>16000)errors.delete(0,errors.length-16000)}
            log(line)
        }
        val err=Thread {p.errorStream.bufferedReader().useLines{lines->lines.forEach{observe(it)}}}.apply{start()}
        val result=StringBuilder()
        p.inputStream.bufferedReader().useLines{lines->lines.forEach{
            if(it.startsWith("DATABEAM_CHILD_PID=")){crocPid=it.substringAfter('=').toInt();return@forEach}
            if(result.length+it.length<2*1024*1024)result.append(it).append('\n')
            observe(it)
        }}
        val exit=p.waitFor();err.join();process=null;crocPid=null
        if(exit!=0 && allowRecovery && c.optString("relay").isEmpty() && !payloadStarted.get() && !stopping.get() && RelayRecovery.transportError(errors.toString())) {
            synchronized(this){state.put("relayNotice","Auto troubleshooting Croc public relays…").put("troubleshooting",true)}
            update("running","Auto troubleshooting Croc public relays…")
            val healthy=RelayRecovery.healthy(stopped={stopping.get()})
            check(!stopping.get()){ "Stopped" }
            synchronized(this){state.put("troubleshooting",false)}
            if(healthy.isEmpty()) {
                val notice="Croc public relays could not be reached from this device. Use Sendme instead and share its ticket with the receiver."
                synchronized(this){state.put("relayNotice",notice).put("suggestSendme",true)}
                error(notice)
            }
            if(action!="send") {
                val notice="Croc public relay for this code is temporarily failing. Other relays are reachable. Ask the sender to generate and share a replacement code, or use Sendme."
                synchronized(this){state.put("relayNotice",notice)}
                error(notice)
            }
            val endpoint=healthy.firstOrNull{it!=relay} ?: healthy.first()
            val replacement=RelayRecovery.replacement(code,endpoint)
            val notice="Earlier code $code is temporarily failing due to a Croc public relay connection problem. Generated a replacement for $endpoint. Share the new code with the receiver."
            c.put("code",replacement)
            synchronized(this){state.put("code",replacement).put("codeRevision",replacement).put("relayNotice",notice)}
            log(notice);update("running","Retrying with replacement code…")
            return crocAttempt(c,action,text,false)
        }
        require(exit==0){RelayRecovery.failureMessage(errors.toString(),exit)}
        synchronized(this){state.put("output",output.absolutePath)}
        return result.toString()
    }
    @Synchronized private fun log(line:String) {
        state.put("log",(state.optString("log")+"\n"+line).takeLast(18000));persist()
    }
    @Synchronized private fun update(status:String,message:String) {
        state.put("status",status).put("message",message);persist()
        getSystemService(NotificationManager::class.java).notify(1,notification(message))
    }
    @Synchronized private fun persist() {getSharedPreferences("transfer",0).edit().putString("state",state.toString()).commit()}
    private fun notification(message:String):Notification {
        val open=PendingIntent.getActivity(this,0,Intent(this,MainActivity::class.java),PendingIntent.FLAG_IMMUTABLE)
        val stop=PendingIntent.getService(this,1,Intent(this,TransferService::class.java).setAction("stop"),PendingIntent.FLAG_IMMUTABLE)
        val snapshot=synchronized(this){JSONObject(state.toString())}
        val preparing=snapshot.optBoolean("preparing")
        val total=snapshot.optLong(if(preparing)"prepTotal" else "total");val done=snapshot.optLong(if(preparing)"prepDone" else "done")
        val percentage=if(total>0 || snapshot.optInt("filesTotal")>0)ProgressEvents.percent(done,total,snapshot.optInt("filesDone"),snapshot.optInt("filesTotal")) else snapshot.optInt("percent")
        val phase=snapshot.optString("phase",if(snapshot.optString("action")=="receive")"Receiving" else "Sharing")
        val speed=snapshot.optDouble(if(preparing)"prepSpeed" else "speed").takeIf{it.isFinite() && it>0} ?: 0.0
        val content=if(message.isNotEmpty())message else listOf(phase,
            if(total>0)"$percentage% · ${ProgressEvents.size(done)} / ${ProgressEvents.size(total)}" else if(snapshot.has("percent"))"$percentage%" else "",
            if(snapshot.optInt("filesTotal")>0)"${snapshot.optInt("filesDone")} / ${snapshot.optInt("filesTotal")} files" else "",
            if(speed>0 && snapshot.optString("status")=="running")"${ProgressEvents.size(speed.toLong())}/s"+(ProgressEvents.eta(done,total,speed)?.let{" · $it left"} ?: "") else "").filter{it.isNotEmpty()}.joinToString(" · ")
        return Notification.Builder(this,CHANNEL).setSmallIcon(com.vinaywinai.databeam.R.drawable.ic_databeam).setContentTitle("DataBeam")
            .setSubText("${snapshot.optString("mode")} · ${if(snapshot.optString("action")=="receive")"Receive" else "Send"}")
            .setContentText(content).setStyle(Notification.BigTextStyle().bigText(content))
            .setProgress(100,percentage,total==0L && !snapshot.has("percent") && snapshot.optInt("filesTotal")==0)
            .setOnlyAlertOnce(true).setContentIntent(open).setOngoing(true).addAction(Notification.Action.Builder(null,"Stop",stop).build()).build()
    }
    override fun onTimeout(startId:Int,fgsType:Int) {
        stopping.set(true);runCatching{NativeBridge.cancel()};process?.destroy()
        update("interrupted","Android background time limit reached; reopen DataBeam to resume")
        stopSelf()
    }
    override fun onDestroy() {
        runCatching{getSystemService(android.net.ConnectivityManager::class.java).unregisterNetworkCallback(networkCallback)}
        if(busy.get()){stopping.set(true);runCatching{NativeBridge.cancel()};process?.destroy()}
        super.onDestroy()
    }
}
