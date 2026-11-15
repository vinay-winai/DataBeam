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
        fun isBusy()=busy.get()
        const val CHANNEL = "transfers"
        fun state(c: Context): JSONObject {
            val wasBusy=busy.get()
            val prefs=c.getSharedPreferences("transfer",0)
            val saved=JSONObject(prefs.getString("state","{}")!!)
            if(!wasBusy && !busy.get() && saved.optString("status") in listOf("running","stopping","stopped","interrupted") && TransferLabels.senderFinished(saved)){
                saved.put("connectionRetained",false).put("status","complete").put("message","Files sent successfully. Sharing has ended. Tap Share again to start a new session.")
                prefs.edit().putString("state",saved.toString()).commit();return saved
            }
            if(!wasBusy && !busy.get() && (saved.optString("status") in listOf("running","stopping") || (saved.optString("status")=="paused" && saved.optBoolean("connectionRetained")))) {
                saved.put("connectionRetained",false).put("status","interrupted").put("message",TransferLabels.interruptedMessage(saved))
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
        RelayRecovery.initializeWordList(assets.open("eff-short-wordlist-1.txt").bufferedReader().use{it.readText()})
        getSystemService(android.net.ConnectivityManager::class.java).registerDefaultNetworkCallback(networkCallback)
    }
    private val main = Handler(Looper.getMainLooper())
    private var process: java.lang.Process? = null
    @Volatile private var crocPid:Int?=null
    private val stopping = AtomicBoolean(false)
    private val sentSuccessfully=AtomicBoolean(false)
    @Volatile private var backgroundTimedOut=false
    private var wake: PowerManager.WakeLock? = null
    private var multicast: android.net.wifi.WifiManager.MulticastLock? = null
    private var state = JSONObject()
    private var job: Thread? = null
    private var lastProgressNotice=0L
    private var timing=TransferTiming()
    private var outgoingSources:OutgoingSources?=null
    private val poll = object : Runnable {
        override fun run() {
            runCatching {
                val events = JSONArray(NativeBridge.poll())
                for (i in 0 until events.length()) {
                    val e=events.getJSONObject(i)
                    val kind=e.getString("kind"); val payload=e.getString("payload")
                    val eventNow=SystemClock.elapsedRealtime()-(System.currentTimeMillis()-e.optLong("atMs",System.currentTimeMillis())).coerceAtLeast(0)
                    if(kind=="receive-route")synchronized(this@TransferService){TransferRoute.receiveEvent(state,payload)}
                    if(kind=="receive-paths")synchronized(this@TransferService){TransferRoute.selected(JSONArray(payload))?.let{TransferRoute.receiveEvent(state,it)}}
                    if(kind=="send-paths")synchronized(this@TransferService){
                        val event=JSONObject(payload);TransferRoute.selected(event.getJSONArray("paths"))?.let{TransferRoute.senderEvent(state,event.getString("connection"),it)}
                    }
                    if(kind=="send-route")synchronized(this@TransferService){
                        val event=JSONObject(payload);TransferRoute.senderEvent(state,event.getString("connection"),event.getString("route"))
                    }
                    if(kind=="ticket") synchronized(this@TransferService) { state.put("ticket",payload) }
                    if(kind in listOf("receive-progress","transfer-progress","preparation-progress")) {
                        ProgressEvents.native(payload)?.let{sample->synchronized(this@TransferService) {
                            if(kind=="preparation-progress")state.put("preparing",true).put("prepDone",sample.done).put("prepTotal",sample.total).put("prepSpeed",sample.speed).put("phase","Preparing files")
                            else {timing.progress(sample.done,eventNow);timing.save(state);if(!state.has("firstPayloadDone"))state.put("firstPayloadDone",sample.done);state.put("preparing",false).put("done",sample.done).put("total",sample.total).put("speed",sample.speed).put("phase","Transferring")}
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
                    if(kind=="transfer-started")synchronized(this@TransferService){timing.beginPayload(eventNow);timing.save(state)}
                    if(kind=="transfer-completed"){
                        val end=synchronized(this@TransferService){timing.finishPayload(eventNow);timing.save(state);TransferLabels.autoEnd(state)}
                        if(end){sentSuccessfully.set(true);NativeBridge.cancel();process?.destroy()}
                    }
                    log("$kind $payload")
                }
                val now=SystemClock.elapsedRealtime()
                if(busy.get() && now-lastProgressNotice>=1000){lastProgressNotice=now;synchronized(this@TransferService){timing.checkpoint(now);timing.save(state);persist()};getSystemService(NotificationManager::class.java).notify(1,notification(""))}
            }
            if(busy.get()) main.postDelayed(this,300)
        }
    }
    override fun onBind(i: Intent?) = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if(intent?.action=="resume" && state.optString("status")=="paused" && process!=null && crocPid!=null) {
            android.system.Os.kill(crocPid!!,android.system.OsConstants.SIGCONT)
            synchronized(this){timing.resume(SystemClock.elapsedRealtime());timing.save(state);state.put("connectionRetained",false)};update("running","Continuing transfer…");return START_NOT_STICKY
        }
        if(intent?.action=="pause" && state.optString("action")=="receive" && state.optString("mode")=="Croc" && process!=null && crocPid!=null) {
            android.system.Os.kill(crocPid!!,android.system.OsConstants.SIGSTOP)
            synchronized(this){timing.pause(SystemClock.elapsedRealtime());timing.save(state);state.put("connectionRetained",true)};update("paused","Paused · connection retained");return START_NOT_STICKY
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
        // Foreground status precedes loading a large text configuration. Large
        // configs are private temporary files, never large Binder extras.
        state=JSONObject()
        val nm=getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL,"Transfers",NotificationManager.IMPORTANCE_LOW))
        if(Build.VERSION.SDK_INT>=29)startForeground(1,notification("Preparing transfer"),ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        else startForeground(1,notification("Preparing transfer"))
        stopping.set(false)
        sentSuccessfully.set(false)
        backgroundTimedOut=false
        NativeBridge.prepare()
        try{
            val name=intent?.getStringExtra("configFile")
            state=if(name!=null){
                require(name.matches(Regex("[0-9a-f-]{36}\\.json"))){"Invalid transfer configuration"}
                val root=File(cacheDir,"start-config").canonicalFile;val config=File(root,name).canonicalFile
                require(config.parentFile==root){"Invalid transfer configuration"}
                try{JSONObject(config.readText(Charsets.UTF_8))}finally{config.delete()}
            }else JSONObject(intent?.getStringExtra("config") ?: "{}")
        }catch(error:Exception){
            state=JSONObject().put("id",intent?.getStringExtra("sessionId").orEmpty()).put("mode",intent?.getStringExtra("mode").orEmpty())
                .put("action",intent?.getStringExtra("action").orEmpty()).put("status","interrupted").put("message","Could not open transfer settings: ${error.message}")
            persist();busy.set(false);stopForeground(STOP_FOREGROUND_REMOVE);stopSelf();return START_NOT_STICKY
        }
        timing=TransferTiming(state.optJSONObject("timing")).apply{resume(SystemClock.elapsedRealtime());save(state)}
        if(!state.has("saveFolder"))state.put("saveFolder",AppSettings.folder(this)).put("saveFolderLabel",AppSettings.folderLabel(this))
        state.remove("route");state.remove("senderRoutes")
        state.remove("timingConcurrent")
        state.put("preparing",false).put("speed",0).put("prepSpeed",0)
        state.remove("firstPayloadDone")
        if(state.optString("mode")=="Croc"){for(key in listOf("overallProgress","done","total","filesDone","filesTotal"))state.remove(key)}
        if(state.optString("action")=="send") state.remove("ticket")
        state.put("connectionRetained",false);state.remove("phase");state.remove("relayNotice");state.remove("suggestSendme");state.remove("troubleshooting");state.remove("codeRevision")
        state.remove("cacheInfo");state.remove("awaitingCacheChoice");state.remove("cachedReceiveBytes")
        wake=(getSystemService(POWER_SERVICE) as PowerManager).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"DataBeam:transfer").apply { acquire(6*60*60*1000L) }
        update("running","Preparing…")
        main.post(poll)
        job=Thread {
            TransferCleanup.lock.lock()
            var terminal="complete"
            var terminalMessage="Complete"
            try {
                val config=synchronized(this) {JSONObject(state.toString())}
                stageSources(config)
                synchronized(this) {
                    state.put("paths",config.optJSONArray("paths"))
                    if(config.optString("action")=="send" && !state.has("payloadNames"))state.put("payloadNames",if(config.optString("text").isNotEmpty())JSONArray().put("Text") else JSONArray((0 until (config.optJSONArray("paths")?.length() ?: 0)).map{File(config.getJSONArray("paths").getString(it)).name}))
                }
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
                            while(native.isAlive && !stopping.get() && !sentSuccessfully.get()) {
                                val ticket=synchronized(this){state.optString("ticket")}
                                if(ticket.isNotEmpty() && !advertised) {
                                    croc(config,"send",ticket);advertised=true
                                    if(!sentSuccessfully.get())update("running","Waiting for receiver")
                                }
                                Thread.sleep(100)
                            }
                        } finally {
                            runCatching { NativeBridge.cancel() };native.join()
                        }
                        nativeError?.let { throw it }
                    } else {
                        // A Croc ticket handoff is one-shot. Resume the saved native
                        // ticket unless the user explicitly requests a new handoff.
                        var ticket=config.optString("ticket")
                        if(EasyReceive.needsLookup(config)) {
                            synchronized(this){state.put("phase","Getting sender ticket")};persist()
                            ticket=croc(config,"receive",null).split(Regex("\\s+")).firstOrNull { NativeBridge.hash(it).isNotEmpty() } ?: error("Sender did not supply a valid Sendme ticket")
                        }
                        EasyReceive.accept(config,ticket,NativeBridge::hash)
                        synchronized(this){state.put("ticket",ticket).put("ticketCode",config.getString("ticketCode"));state.remove("refresh")};persist()
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
                        val receipt=synchronized(this){JSONObject(state.toString())}
                        TransferCleanup.saved(this,receipt)
                        synchronized(this){state.put("workingCleaned",true)};persist()
                        terminalMessage="Saved to ${config.optString("saveFolderLabel","Downloads / DataBeam")}"
                    } else terminalMessage="Text received"
                }
            } catch(e:Throwable) {
                terminal=if(backgroundTimedOut)"interrupted" else if(stopping.get()) if(state.optString("action")=="send")"stopped" else "paused" else "interrupted"
                terminalMessage=if(backgroundTimedOut)"Android reached its background transfer time limit." else if(stopping.get()) if(state.optString("action")=="send")"Sharing ended" else "Paused · received data retained" else (e.message ?: e.javaClass.simpleName)
                if(terminal=="stopped" && synchronized(this){TransferLabels.senderFinished(state)}){
                    terminal="complete";terminalMessage="Files sent successfully. Sharing has ended. Tap Share again to start a new session."
                }
            } finally {
                runCatching{outgoingSources?.close()};outgoingSources=null
                runCatching{
                    TransferCleanup.record(this,synchronized(this){JSONObject(state.toString())},terminal)
                    if(state.optString("action")=="send")TransferCleanup.remove(filesDir,"blobs/${state.getString("id")}")
                    if(state.optString("action")=="send" && (terminal=="complete" || sentSuccessfully.get())){
                        TransferCleanup.remove(filesDir,"outgoing/${state.getString("id")}")
                        TransferCleanup.remove(filesDir,"received/${state.getString("id")}")
                    }
                }
                TransferCleanup.lock.unlock()
                main.post {
                    // Drain final JNI counters before freezing the completion summary.
                    poll.run()
                    main.removeCallbacks(poll)
                    if(sentSuccessfully.get()){terminal="complete";terminalMessage="Files sent successfully. Sharing ended automatically."}
                    synchronized(this){
                        if(terminal=="complete"){
                            timing.finishPayload(SystemClock.elapsedRealtime())
                            if(state.optString("action")=="send"){
                                state.put("text","").put("uris",JSONArray());state.remove("paths")
                            } else state.optJSONObject("published")?.let{published->state.put("payloadNames",JSONArray(published.keys().asSequence().toList()))}
                        }
                        timing.pause(SystemClock.elapsedRealtime());timing.save(state)
                    }
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
        if(old!=null && old.length()>0 && !c.optBoolean("referencedSources")) {
            for(i in 0 until old.length()) require(File(old.getString(i)).exists()) {"Original files are missing. Select them again."}
            return
        }
        val uris=c.optJSONArray("uris") ?: error("Select files or a folder")
        synchronized(this){state.put("phase","Opening selected files").put("preparing",true).put("prepDone",0).put("prepTotal",0)};persist()
        val folder=File(filesDir,"outgoing/${c.getString("id")}").apply{mkdirs()}
        val sources=OutgoingSources(this,folder,{stopping.get()}){synchronized(this){state.put("phase","Copying provider file")};persist()}
        outgoingSources=sources
        val paths=JSONArray()
        for(i in 0 until uris.length()) {
            val file=sources.add(android.net.Uri.parse(uris.getString(i)))
            paths.put(file.absolutePath)
        }
        require(paths.length()>0){"No files selected"};c.put("paths",paths).put("referencedSources",sources.referenced>0).put("sourceRoot",folder.absolutePath)
        synchronized(this){state.put("referencedSources",sources.referenced>0).put("sourceRoot",folder.absolutePath).put("referencedFiles",sources.referenced).put("copiedFiles",sources.copied)};persist()
    }
    private fun runNative(c:JSONObject) {
        check(!stopping.get()){ "Stopped" }
        var cache=File(filesDir,"blobs/${c.getString("id")}")
        if(c.optString("action")=="receive") {
            synchronized(this){state.put("phase","Checking saved data").put("message","")};persist()
            val root=File(filesDir,"blobs")
            ReceiveCache.inspect(root,c.getString("ticket"))?.let{info->
                cache=File(info.getString("base"))
                synchronized(this){state.put("cachedReceiveBytes",info.optLong("bytes")).put("done",info.optLong("bytes")).put("speed",0)};persist()
            }
            synchronized(this){state.put("message","").put("phase",if(c.optString("mode")=="EasySendMe" && c.optString("status") in listOf("paused","interrupted"))"Reconnecting to sender" else "Connecting")};persist()
        }
        c.put("cache",cache.apply{mkdirs()}.absolutePath)
        c.put("output",File(filesDir,"received/${c.getString("id")}").apply{mkdirs()}.absolutePath)
        synchronized(this){state.put("output",c.getString("output")).put("cacheBase",c.getString("cache"))};TransferCleanup.record(this,synchronized(this){JSONObject(state.toString())})
        if(c.optString("action")=="send"){
            val paths=c.getJSONArray("paths")
            c.put("root",PayloadNaming.root((0 until paths.length()).map{File(paths.getString(it))}){stopping.get()})
        }
        val result=JSONObject(NativeBridge.run(c.toString()))
        if(result.optString("status")=="error") error(result.optString("message"))
        synchronized(this) { state.put("output",c.getString("output")) }
    }
    private fun croc(c:JSONObject,action:String,text:String?):String {
        return crocAttempt(c,action,text,true)
    }
    private fun crocAttempt(c:JSONObject,action:String,text:String?,allowRecovery:Boolean):String {
        val code=c.optString("code").ifEmpty {
            RelayRecovery.newCode().also { c.put("code",it);synchronized(this){state.put("code",it)};persist() }
        }
        require(code.length>=7){"Code must have at least 7 characters"}
        val relay=c.optString("relay").ifEmpty { RelayRecovery.relay(code) }
        val output=File(filesDir,"received/${c.getString("id")}").apply{mkdirs()}
        if(action=="receive"){synchronized(this){state.put("output",output.absolutePath)};persist()}
        val sendText=if(action=="send")text ?: c.optString("text").takeIf{it.isNotEmpty()} else null
        val args=mutableListOf(File(applicationInfo.nativeLibraryDir,"libcroc.so").absolutePath,"--yes","--relay",relay)
        if(sendText==null)args.add("--ignore-stdin")
        if(action=="send") {
            args.add("send")
            // Croc reads piped stdin before --text, while --text marks it as
            // text for the receiver. Never place unbounded text in argv (E2BIG).
            if(sendText!=null) args.addAll(listOf("--text","-")) else {
                args.add("--");val paths=c.getJSONArray("paths")
                for(i in 0 until paths.length())args.add(paths.getString(i))
            }
        } else {
            args.addAll(listOf("--out",output.absolutePath))
            if(c.optString("mode")=="Croc") {
                require(c.optString("receiveCode",code)==code){"Use the original Croc code to resume. For a different code, start a new receive."}
                c.put("receiveCode",code);synchronized(this){state.put("receiveCode",code)};persist()
                // This directory is private working storage for this one session.
                // --yes does not answer Croc's separate resume question. Public
                // destination conflicts remain protected by Storage.publishDownloads.
                args.add("--overwrite")
            }
        }
        val command=listOf("/system/bin/sh","-c","echo DATABEAM_CHILD_PID=$$; exec \"$@\"","databeam-croc")+args
        val builder=ProcessBuilder(command).directory(output)
        builder.environment().putAll(mapOf("CROC_SECRET" to code,"HOME" to filesDir.absolutePath,"TMPDIR" to cacheDir.absolutePath,"DATABEAM_CROC_PROGRESS" to "1","CROC_NO_UPDATE_CHECK" to "1"))
        if(action=="send" && c.optBoolean("referencedSources"))builder.environment()["DATABEAM_ANDROID_SOURCE_ROOT"]=c.getString("sourceRoot")
        val p=builder.start();process=p;if(stopping.get())p.destroy()
        val writeFailure=java.util.concurrent.atomic.AtomicReference<Exception?>()
        val writer=if(sendText!=null)Thread{
            try{p.outputStream.use{it.write(sendText.toByteArray(Charsets.UTF_8))}}
            catch(error:Exception){if(!stopping.get())writeFailure.compareAndSet(null,error)}
        }.apply{start()} else null
        if(writer==null)p.outputStream.close()
        val errors=StringBuffer()
        val payloadStarted=AtomicBoolean(false)
        fun observe(line:String) {
            if(c.optString("mode")=="Croc")ProgressEvents.croc(line)?.let{event->
                synchronized(this){
                    event.preparation?.let{sample->state.put("preparing",true).put("phase","Preparing files").put("prepDone",sample.done).put("prepTotal",sample.total).put("prepSpeed",sample.speed)}
                    event.speed?.let{if(!state.optBoolean("overallProgress"))state.put("speed",it)}
                    if(event.verifying)state.put("preparing",false).put("phase","Verifying files").put("speed",0)
                    event.transfer?.let{sample->timing.progress(sample.done,SystemClock.elapsedRealtime());timing.save(state);if(!state.has("firstPayloadDone"))state.put("firstPayloadDone",sample.done);state.put("overallProgress",true).put("preparing",false).put("phase","Transferring").put("done",sample.done).put("total",sample.total).put("speed",sample.speed).put("filesDone",event.filesDone).put("filesTotal",event.filesTotal).put("progress","")}
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
        val readFailure=java.util.concurrent.atomic.AtomicReference<Exception?>()
        val err=Thread {
            try{p.errorStream.bufferedReader().useLines{lines->lines.forEach{observe(it)}}}
            catch(error:Exception){if(!stopping.get()){readFailure.compareAndSet(null,error);p.destroy()}}
        }.apply{start()}
        val result=StringBuilder()
        try{p.inputStream.bufferedReader(Charsets.UTF_8).use{reader->
            val first=reader.readLine()
            if(first?.startsWith("DATABEAM_CHILD_PID=")==true)crocPid=first.substringAfter('=').toInt()
            else if(first!=null)error("Croc did not report its process ID")
            // stdout after the wrapper's PID line is the exact received text.
            // Do not normalize line endings, append newlines or parse user text
            // as progress/error events. Croc telemetry is read from stderr.
            val buffer=CharArray(8192)
            while(true){val count=reader.read(buffer);if(count<0)break
                check(result.length+count<=8*1024*1024){"Text exceeds the display limit. Send it as a text file instead."}
                result.append(buffer,0,count)
            }
        }}
        catch(error:Exception){if(!stopping.get())readFailure.compareAndSet(null,error);p.destroy()}
        val exit=try{p.waitFor()}finally{err.join();writer?.join();process=null;crocPid=null}
        check(!stopping.get()){"Stopped"}
        // Croc prints its terminal error to stdout. Successful stdout is
        // payload text; inspect it as an error only after a failed exit.
        if(exit!=0)synchronized(errors){errors.append('\n').append(result.takeLast(16000))}
        readFailure.get()?.let{throw IllegalStateException("Could not read Croc transfer status: ${it.message}",it)}
        if(exit!=0){
            val failure=RelayRecovery.failureMessage(errors.toString(),exit)
            synchronized(this){state.put("crocExit",exit).put("crocFailure",failure).put("crocStdoutLength",result.length)}
            log("Croc failure: $failure")
        }
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
        writeFailure.get()?.let{throw IllegalStateException("Could not send all text: ${it.message}",it)}
        synchronized(this){state.put("output",output.absolutePath)}
        return result.toString()
    }
    @Synchronized private fun log(line:String) {
        state.put("log",(state.optString("log")+"\n"+line).takeLast(18000));persist()
    }
    @Synchronized private fun update(status:String,message:String) {
        state.put("status",status).put("message",message);persist()
        runCatching{TransferCleanup.record(this,JSONObject(state.toString()))}
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
        val phase=TransferLabels.title(snapshot)
        val speed=snapshot.optDouble(if(preparing)"prepSpeed" else "speed").takeIf{it.isFinite() && it>0} ?: 0.0
        val summary=if(message.isNotEmpty())TransferLabels.message(snapshot).ifEmpty{phase} else listOf(phase,
            if(total>0)"$percentage% · ${ProgressEvents.size(done)} / ${ProgressEvents.size(total)}" else if(snapshot.has("percent"))"$percentage%" else "",
            if(snapshot.optInt("filesTotal")>0)"${snapshot.optInt("filesDone")} / ${snapshot.optInt("filesTotal")} files" else "",
            if(speed>0 && snapshot.optString("status")=="running")"${ProgressEvents.size(speed.toLong())}/s"+(ProgressEvents.eta(done,total,speed)?.let{" · $it left"} ?: "") else "").filter{it.isNotEmpty()}.joinToString(" · ")
        val content=listOf(summary,TransferTiming.elapsedText(snapshot),TransferTiming.averageText(snapshot)).filter{it.isNotEmpty()}.joinToString(" · ")
        return Notification.Builder(this,CHANNEL).setSmallIcon(com.vinaywinai.databeam.R.drawable.ic_databeam).setContentTitle("DataBeam")
            .setSubText("${snapshot.optString("mode")} · ${if(snapshot.optString("action")=="receive")"Receive" else "Send"}")
            .setContentText(content).setStyle(Notification.BigTextStyle().bigText(content))
            .setProgress(100,percentage,total==0L && !snapshot.has("percent") && snapshot.optInt("filesTotal")==0)
            .setOnlyAlertOnce(true).setContentIntent(open).setOngoing(true).addAction(Notification.Action.Builder(null,"Stop",stop).build()).build()
    }
    override fun onTimeout(startId:Int,fgsType:Int) {
        backgroundTimedOut=true
        stopping.set(true);runCatching{NativeBridge.cancel()};process?.destroy()
        update("interrupted","Android reached its background transfer time limit.")
        stopSelf()
    }
    override fun onDestroy() {
        runCatching{getSystemService(android.net.ConnectivityManager::class.java).unregisterNetworkCallback(networkCallback)}
        if(busy.get()){stopping.set(true);runCatching{NativeBridge.cancel()};process?.destroy()}
        super.onDestroy()
    }
}
