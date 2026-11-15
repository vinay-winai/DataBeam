package com.vinaywinai.databeam
import android.app.*
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import android.graphics.Bitmap
import android.content.*
import android.graphics.Color
import android.net.Uri
import android.os.*
import android.view.*
import android.widget.*
import org.json.*
import java.io.File
import java.util.UUID

class MainActivity:ComponentActivity() {
    private var qrDialog:AlertDialog?=null
    private var settingsDialog:AlertDialog?=null
    private var cacheDialog:AlertDialog?=null
    private var cleanupBusy=false
    private var cleanupScanning=false
    private var cleanupScanned=false
    private var settingsFolder:TextView?=null
    private var settingsBackground:TextView?=null
    private var batteryButton:Button?=null
    private var notificationsButton:Button?=null
    private lateinit var backgroundNotice:TextView
    private val folderPicker=registerForActivityResult(ActivityResultContracts.OpenDocumentTree()){uri->
        if(uri!=null && !configurationLocked())runCatching {
            contentResolver.takePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            val doc=android.provider.DocumentsContract.buildDocumentUriUsingTree(uri,android.provider.DocumentsContract.getTreeDocumentId(uri))
            val label=Storage.name(this,doc)
            AppSettings.setFolder(this,uri,label)
            updateSaveLocation()
        }.onFailure{alert(it.message ?: "Could not use this folder")}
    }
    private val scanner=registerForActivityResult(ActivityResultContracts.StartActivityForResult()){result->
        if(result.resultCode==RESULT_OK)result.data?.getStringExtra(QrScannerActivity.RESULT_CODE)?.let{raw->runCatching{
            val parsed=QrSharing.parse(raw)
            selectTab("receive");code.setText(parsed.value)
            if(NativeBridge.hash(parsed.value).isNotEmpty()){mode.setSelection(1)}
            else if(parsed.crocLink){mode.setSelection(2)}
            else AlertDialog.Builder(this).setTitle("Sender’s transfer mode").setItems(arrayOf("EasySendMe","Croc")){_,which->mode.setSelection(if(which==0)0 else 2)}.setNegativeButton("Cancel",null).show()
        }.onFailure{alert(it.message ?: "Could not read this QR")}}
    }
    private lateinit var mode:Spinner
    private lateinit var code:EditText
    private lateinit var sendCode:EditText
    private lateinit var sendCodeActions:LinearLayout
    private lateinit var sendRecent:Button
    private lateinit var receiveRecent:Button
    private var codeMode=""
    private var retainedSession=""
    private lateinit var message:EditText
    private lateinit var saveLocation:TextView
    private lateinit var status:TextView
    private lateinit var ticket:TextView
    private lateinit var relayNotice:TextView
    private lateinit var sendmeFallback:Button
    private var lastReplacement=""
    private lateinit var receivedText:TextView
    private lateinit var receivedTextTitle:TextView
    private lateinit var selection:TextView
    private lateinit var selectionList:LinearLayout
    private lateinit var enginePanel:LinearLayout
    private lateinit var fileSelectionPanel:LinearLayout
    private lateinit var contentSwitch:LinearLayout
    private lateinit var filesChoice:Button
    private lateinit var textChoice:Button
    private var sendKind="files"
    private var pendingStartId=""
    private val settingsConfiguration=mutableListOf<View>()
    private val sources=mutableListOf<Uri>()
    private val restartedNames=mutableMapOf<Uri,String>()
    private val handler=Handler(Looper.getMainLooper())
    private var previous=""
    private val ticker=object:Runnable {override fun run(){render();handler.postDelayed(this,500)}}
    private val ink=Color.rgb(245,245,245)
    private val accent=Color.rgb(255,77,0)
    private lateinit var sendPanel:LinearLayout
    private lateinit var receivePanel:LinearLayout
    private lateinit var primary:Button
    private lateinit var pause:Button
    private lateinit var resumeButton:Button
    private lateinit var restartSharing:Button
    private lateinit var sharing:LinearLayout
    private lateinit var bar:ProgressBar
    private lateinit var detail:TextView
    private lateinit var transferPanel:LinearLayout
    private lateinit var route:TextView
    private lateinit var transferDirection:TextView
    private lateinit var newTransfer:Button
    private lateinit var clearTransferNote:TextView
    private lateinit var endPaused:Button
    private var tab="send"
    private var renderedSession=""
    private lateinit var sendTab:Button
    private lateinit var receiveTab:Button
    private lateinit var engineDescription:TextView
    private fun dp(value:Int)=(resources.displayMetrics.density*value).toInt()
    private fun rounded(color:Int)=android.graphics.drawable.GradientDrawable().apply{setColor(color);cornerRadius=dp(14).toFloat()}
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        NativeBridge.initialize(applicationContext)
        RelayRecovery.initializeWordList(assets.open("eff-short-wordlist-1.txt").bufferedReader().use{it.readText()})
        sendKind=savedInstanceState?.getString("sendKind") ?: "files"
        val current=TransferService.state(this)
        renderedSession=current.optString("id")
        tab=savedInstanceState?.getString("tab") ?: current.optString("action","send")
        val scroll=ScrollView(this).apply{setBackgroundColor(Color.BLACK);isFillViewport=true}
        val screen=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setBackgroundColor(Color.BLACK)}
        val layout=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(20),dp(12),dp(20),dp(24))}
        scroll.addView(layout);screen.addView(scroll,LinearLayout.LayoutParams(-1,0,1f));setContentView(screen)
        screen.setOnApplyWindowInsetsListener{v,insets->
            if(Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(bars.left,bars.top,bars.right,bars.bottom)}
            else v.setPadding(insets.systemWindowInsetLeft,insets.systemWindowInsetTop,insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            insets
        }
        fun label(parent:LinearLayout,text:String,size:Float=14f)=TextView(this).apply{this.text=text;textSize=size;setTextColor(ink);setPadding(0,dp(6),0,dp(6));parent.addView(this)}
        fun row(parent:LinearLayout)=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;parent.addView(this)}
        fun card(parent:LinearLayout)=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;background=rounded(Color.rgb(10,10,10));setPadding(dp(16),dp(10),dp(16),dp(12));parent.addView(this,LinearLayout.LayoutParams(-1,-2).apply{topMargin=dp(12)})}
        fun button(parent:LinearLayout,text:String,emphasized:Boolean=false,action:()->Unit)=Button(this).apply{
            this.text=text;isAllCaps=false;textSize=14f;minHeight=0;minimumHeight=0
            setTextColor(if(emphasized)Color.BLACK else ink);background=rounded(if(emphasized)accent else Color.rgb(28,28,28))
            val params=if(parent.orientation==LinearLayout.HORIZONTAL)LinearLayout.LayoutParams(0,dp(46),1f).apply{marginEnd=dp(6)} else LinearLayout.LayoutParams(-1,dp(48)).apply{topMargin=dp(8)}
            parent.addView(this,params);setOnClickListener{runCatching(action).onFailure{alert(it.message ?: "Operation failed")}}
        }
        fun input(parent:LinearLayout,hint:String)=EditText(this).apply{this.hint=hint;setTextColor(ink);setHintTextColor(Color.rgb(150,150,150));textSize=15f;setSingleLine();parent.addView(this,LinearLayout.LayoutParams(-1,dp(52)))}
        val header=row(layout)
        header.addView(ImageView(this).apply{setImageResource(R.drawable.databeam_logo);contentDescription="DataBeam logo"},LinearLayout.LayoutParams(dp(44),dp(48)).apply{marginEnd=dp(10)})
        label(header,"DataBeam",28f).apply{setTextColor(accent);layoutParams=LinearLayout.LayoutParams(0,-2,1f)}
        ImageButton(this).apply{
            setImageResource(R.drawable.ic_settings);imageTintList=android.content.res.ColorStateList.valueOf(ink)
            background=rounded(Color.rgb(28,28,28));contentDescription="Settings"
            header.addView(this,LinearLayout.LayoutParams(dp(48),dp(48)));setOnClickListener{showSettings()}
        }
        label(layout,"Secure sharing between your devices").setTextColor(Color.LTGRAY)
        backgroundNotice=label(layout,"",12f).apply{
            setTextColor(Color.rgb(255,205,110));visibility=View.GONE;minHeight=dp(48)
            setOnClickListener{showSettings()}
        }
        val tabs=row(layout)
        sendTab=button(tabs,"Send"){selectTab("send")};receiveTab=button(tabs,"Receive"){selectTab("receive")}
        val engine=card(layout);enginePanel=engine;label(engine,"Transfer engine",12f)
        mode=Spinner(this).apply{adapter=ArrayAdapter(this@MainActivity,android.R.layout.simple_spinner_dropdown_item,listOf("EasySendMe","Sendme","Croc"));engine.addView(this)}
        mode.setSelection(savedInstanceState?.getInt("mode") ?: listOf("EasySendMe","Sendme","Croc").indexOf(current.optString("mode","EasySendMe")).coerceAtLeast(0))
        engineDescription=label(engine,"Short code · resumable transfer",12f).apply{setTextColor(Color.LTGRAY)}
        sendPanel=card(layout)
        label(sendPanel,"1 · Choose what to send",17f)
        contentSwitch=row(sendPanel)
        filesChoice=button(contentSwitch,"Files"){sendKind="files";updatePanels()}
        textChoice=button(contentSwitch,"Text"){sendKind="text";updatePanels()}
        fileSelectionPanel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;sendPanel.addView(this)}
        selection=label(fileSelectionPanel,"No files selected")
        selectionList=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;fileSelectionPanel.addView(this)}
        val pick=row(fileSelectionPanel)
        button(pick,"Add files"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").putExtra(Intent.EXTRA_ALLOW_MULTIPLE,true).addCategory(Intent.CATEGORY_OPENABLE),10)}
        button(pick,"Add folder"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),11)}
        message=input(sendPanel,"Text to send")
        message.apply{setSingleLine(false);minLines=2;maxLines=5;gravity=Gravity.TOP;layoutParams=LinearLayout.LayoutParams(-1,-2);setText(savedInstanceState?.getString("message") ?: "")}
        sendCode=input(sendPanel,"Custom sharing code (optional)")
        sendCode.addTextChangedListener(object:android.text.TextWatcher{
            override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){}
            override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){if(codeMode.isNotEmpty() && codeMode!="Sendme")AppSettings.setCode(this@MainActivity,codeMode,s.toString())}
            override fun afterTextChanged(s:android.text.Editable?){}
        })
        sendCodeActions=row(sendPanel)
        sendRecent=button(sendCodeActions,"Recent codes"){showRecent("send",sendCode)}
        button(sendCodeActions,"Clear"){sendCode.setText("");AppSettings.setCode(this,mode.selectedItem.toString(),"")}
        receivePanel=card(layout)
        label(receivePanel,"1 · Enter the sender’s code",17f)
        code=input(receivePanel,"Paste a code or Sendme ticket")
        code.setText(savedInstanceState?.getString("code") ?: if(tab=="receive")current.optString(if(current.optString("mode")=="Sendme")"ticket" else "code") else "")
        val receiveActions=row(receivePanel)
        button(receiveActions,"Paste"){val clipboard=getSystemService(CLIPBOARD_SERVICE) as ClipboardManager;code.setText(clipboard.primaryClip?.getItemAt(0)?.coerceToText(this));normalizeInput()}
        button(receiveActions,"Scan QR"){scan()}
        receiveRecent=button(receivePanel,"Recent codes"){showRecent("receive",code)}
        saveLocation=label(receivePanel,"",12f).apply{setTextColor(accent)}
        updateSaveLocation()
        primary=button(layout,if(tab=="receive")"Receive" else "Send files",true){launch(tab)}
        val transfer=card(layout);transferPanel=transfer;transfer.visibility=View.GONE
        transferDirection=label(transfer,"",12f).apply{setTextColor(Color.LTGRAY)}
        status=label(transfer,"",20f).apply{setTextIsSelectable(true)}
        route=label(transfer,"",12f).apply{setTextColor(accent)}
        detail=label(transfer,"").apply{setTextColor(Color.LTGRAY)}
        bar=ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal).apply{max=100;transfer.addView(this,LinearLayout.LayoutParams(-1,dp(8)))}
        ticket=label(transfer,"").apply{setTextIsSelectable(true);setTextColor(accent)}
        relayNotice=label(transfer,"").apply{setTextColor(Color.rgb(255,205,110))}
        sharing=row(transfer)
        button(sharing,"Copy code"){val value=shareValue();(getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("DataBeam",value));Toast.makeText(this,"Copied",Toast.LENGTH_SHORT).show()}
        button(sharing,"Show QR"){showQr()}
        pause=button(transfer,"Pause"){startService(Intent(this,TransferService::class.java).setAction(if(TransferService.state(this).optString("action")=="receive")"pause" else "stop"))}
        resumeButton=button(transfer,"Resume",true){resume(false)}
        restartSharing=button(transfer,"Restart sharing",true){restartSender()}
        endPaused=button(transfer,"End transfer"){startService(Intent(this,TransferService::class.java).setAction("stop"))}
        newTransfer=button(transfer,"Clear transfer card"){val saved=TransferService.state(this);require(saved.optString("status") !in listOf("running","stopping") && !saved.optBoolean("connectionRetained")){"End the current transfer first"};getSharedPreferences("transfer",0).edit().remove("state").apply();previous="";renderedSession="";code.setText("");render()}
        clearTransferNote=label(transfer,"Clears this card. Saved files remain.",12f).apply{setTextColor(Color.LTGRAY)}
        sendmeFallback=button(transfer,"Use Sendme instead"){mode.setSelection(1);code.setText("");alert("Sendme selected. Ask the sender for its ticket, or select files and press Send.")}
        receivedTextTitle=label(transfer,"Received text",12f)
        receivedText=label(transfer,"").apply{setTextIsSelectable(true)}
        val helpRow=row(screen).apply{gravity=Gravity.CENTER_VERTICAL;setPadding(dp(20),dp(4),dp(20),dp(8))}
        @Suppress("DEPRECATION") val version=packageManager.getPackageInfo(packageName,0).versionName.orEmpty()
        label(helpRow,"v$version",12f).apply{contentDescription="Version $version";setTextColor(Color.LTGRAY);layoutParams=LinearLayout.LayoutParams(0,dp(48),1f);gravity=Gravity.CENTER_VERTICAL}
        button(helpRow,"?"){
            startActivity(Intent(Intent.ACTION_VIEW,Uri.parse("https://github.com/vinay-winai/DataBeam/blob/main/troubleshoot.md")))
        }.apply{
            contentDescription="Troubleshoot";textSize=20f
            layoutParams=LinearLayout.LayoutParams(dp(48),dp(48))
        }
        savedInstanceState?.getStringArrayList("sources")?.forEach{sources.add(Uri.parse(it))}
        mode.onItemSelectedListener=object:AdapterView.OnItemSelectedListener{
            override fun onNothingSelected(p:AdapterView<*>?){}
            override fun onItemSelected(p:AdapterView<*>?,v:View?,position:Int,id:Long){updatePanels()}
        }
        message.addTextChangedListener(object:android.text.TextWatcher{
            override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){}
            override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){updatePrimaryLabel()}
            override fun afterTextChanged(s:android.text.Editable?){}
        })
        if(savedInstanceState==null)acceptShare(intent)
        refreshSelection()
        selectTab(tab)
        lockConfiguration(configurationLocked())
    }
    private fun updatePrimaryLabel(){
        if(::primary.isInitialized)primary.text=if(tab=="receive")"Receive" else if(mode.selectedItem=="Croc" && sendKind=="text")"Send text" else "Send files"
    }
    private fun configurationLocked()=cleanupBusy || pendingStartId.isNotEmpty() || TransferService.state(this).optString("status") in listOf("running","stopping")
    private fun enableTree(view:View,enabled:Boolean){
        view.isEnabled=enabled
        if(view is ViewGroup)for(index in 0 until view.childCount)enableTree(view.getChildAt(index),enabled)
    }
    private fun lockConfiguration(locked:Boolean){
        for(panel in listOf(enginePanel,sendPanel,receivePanel)){enableTree(panel,!locked);panel.alpha=if(locked).55f else 1f}
        sendTab.isEnabled=!locked;receiveTab.isEnabled=!locked
        primary.isEnabled=!locked;primary.alpha=if(locked).45f else 1f
        settingsConfiguration.forEach{enableTree(it,!locked);it.alpha=if(locked).45f else 1f}
    }
    private fun sourceName(uri:Uri):String=restartedNames[uri] ?: runCatching{
        if(uri.scheme=="file")File(uri.path.orEmpty()).name else {
            val document=if(android.provider.DocumentsContract.isTreeUri(uri))android.provider.DocumentsContract.buildDocumentUriUsingTree(uri,android.provider.DocumentsContract.getTreeDocumentId(uri)) else uri
            Storage.name(this,document)
        }
    }.getOrElse{uri.lastPathSegment?.substringAfterLast('/') ?: "Selected item"}
    private fun refreshSelection(){
        selection.text=if(sources.isEmpty())"No files selected" else "${sources.size} item(s) selected"
        selectionList.removeAllViews()
        sources.forEachIndexed{index,uri->
            val name=sourceName(uri)
            val row=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;gravity=Gravity.CENTER_VERTICAL}
            row.addView(TextView(this).apply{text=name;setTextColor(ink);textSize=14f;maxLines=3;setPadding(0,dp(6),dp(8),dp(6))},LinearLayout.LayoutParams(0,-2,1f))
            row.addView(Button(this).apply{text="×";textSize=22f;isAllCaps=false;minWidth=0;minimumWidth=0;setPadding(0,0,0,0);setTextColor(accent);background=rounded(Color.rgb(28,28,28));contentDescription="Remove $name";setOnClickListener{if(!configurationLocked()){sources.removeAt(index);refreshSelection()}}},LinearLayout.LayoutParams(dp(48),dp(48)))
            selectionList.addView(row,LinearLayout.LayoutParams(-1,-2).apply{bottomMargin=dp(6)})
        }
        enableTree(selectionList,!configurationLocked())
    }
    private fun showRecent(action:String,target:EditText){
        val selected=mode.selectedItem.toString();val recent=AppSettings.recent(this,selected,action)
        AlertDialog.Builder(this).setTitle("Recent $selected codes")
            .setItems(recent.toTypedArray()){_,index->target.setText(recent[index])}
            .setNegativeButton("Cancel",null).setNeutralButton("Clear history"){_,_->AppSettings.clearRecent(this,selected,action);updatePanels()}.show()
    }
    private fun unrestricted()=(getSystemService(POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(packageName)
    private fun notificationsEnabled():Boolean {
        val manager=getSystemService(NotificationManager::class.java)
        return manager.areNotificationsEnabled() && manager.getNotificationChannel(TransferService.CHANNEL)?.importance!=NotificationManager.IMPORTANCE_NONE
    }
    private fun updateBackgroundStatus(){
        val battery=unrestricted();val notifications=notificationsEnabled()
        settingsBackground?.text="Battery · ${if(battery)"Unrestricted enabled" else "Restricted or optimized"}\nNotifications · ${if(notifications)"Enabled" else "Disabled"}"
        batteryButton?.text=if(battery)"Manage battery settings" else "Allow unrestricted battery use"
        notificationsButton?.text=if(notifications)"Manage notifications" else "Enable transfer notifications"
        if(::backgroundNotice.isInitialized){
            backgroundNotice.visibility=if(battery && notifications)View.GONE else View.VISIBLE
            backgroundNotice.text="For better background transfers: "+listOfNotNull(if(!battery)"allow unrestricted battery use" else null,if(!notifications)"enable notifications" else null).joinToString(" and ")+". Tap to open Settings."
        }
    }
    private fun updateSaveLocation(){
        if(::saveLocation.isInitialized)saveLocation.text="Files save automatically to ${AppSettings.folderLabel(this)}"
        settingsFolder?.text=AppSettings.folderLabel(this)
    }
    private fun showSettings(){
        settingsConfiguration.clear()
        val panel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(22),dp(8),dp(22),dp(16))}
        fun label(value:String)=TextView(this).apply{text=value;setTextColor(ink);setPadding(0,dp(10),0,dp(8));panel.addView(this)}
        fun button(value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;panel.addView(this);setOnClickListener{runCatching(action).onFailure{alert(it.message ?: "Operation failed")}}}
        label("Background transfers")
        settingsBackground=label("").apply{setTextColor(accent)}
        batteryButton=button("Battery settings"){
            // Application details is supported across vendors; Samsung exposes Battery here.
            startActivity(Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,Uri.parse("package:$packageName")))
        }
        label("In Android app settings, choose Battery → Unrestricted. Android may still limit very long background transfers.").textSize=12f
        notificationsButton=button("Notifications"){
            val manager=getSystemService(NotificationManager::class.java)
            val channel=manager.getNotificationChannel(TransferService.CHANNEL)
            val intent=if(manager.areNotificationsEnabled() && channel?.importance==NotificationManager.IMPORTANCE_NONE)
                Intent(android.provider.Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS).putExtra(android.provider.Settings.EXTRA_CHANNEL_ID,TransferService.CHANNEL)
            else Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            startActivity(intent.putExtra(android.provider.Settings.EXTRA_APP_PACKAGE,packageName))
        }
        updateBackgroundStatus()
        label("Download save location")
        settingsFolder=label(AppSettings.folderLabel(this)).apply{setTextColor(accent)}
        settingsConfiguration.add(button("Change folder…"){folderPicker.launch(AppSettings.folder(this).takeIf{it.isNotEmpty()}?.let(Uri::parse))})
        settingsConfiguration.add(button("Use Downloads / DataBeam"){AppSettings.setFolder(this,null);updateSaveLocation()})
        label("Applies to new transfers. Paused transfers keep their original folder.").textSize=12f
        label("Transfer storage")
        label("Saved working copies are verified and removed automatically. Known inactive caches expire after seven days when checked; current resume/restart data is kept.").textSize=12f
        settingsConfiguration.add(button("Review and clean transfer storage"){showStorageCleanup(true)})
        label("Advanced options")
        val custom=EditText(this).apply{hint="Custom Croc relay (optional)";setSingleLine();setText(AppSettings.relay(this@MainActivity));panel.addView(this)};settingsConfiguration.add(custom)
        label("Leave empty to use Croc public relays. Relay recovery stays enabled.").textSize=12f
        val results=label("").apply{visibility=View.GONE;textSize=13f;setTextIsSelectable(true)}
        val check=button("Check Croc public relays"){}
        check.setOnClickListener{
            check.isEnabled=false;results.visibility=View.VISIBLE;results.text="Checking Croc public relays…"
            Thread{
                val outcome=runCatching{RelayRecovery.healthy(stopped={isDestroyed})}
                runOnUiThread{
                    if(isDestroyed)return@runOnUiThread
                    check.isEnabled=true
                    results.text=outcome.fold({healthy->
                        RelayRecovery.relays.joinToString("\n"){"$it · ${if(it in healthy)"Reachable" else "Unreachable"}"}+"\n\n"+
                        if(healthy.isEmpty())"No public relay reachable. Use Sendme instead." else "Reachability from this phone; a transfer may still fail later."
                    },{"Check failed: ${it.message}"})
                }
            }.start()
        }
        val sendmeResults=label("").apply{visibility=View.GONE;textSize=13f;setTextIsSelectable(true)}
        val checkSendme=button("Check Sendme public relay"){}
        checkSendme.setOnClickListener{
            checkSendme.isEnabled=false;sendmeResults.visibility=View.VISIBLE;sendmeResults.text="Connecting to a Sendme public relay…"
            Thread{
                val outcome=runCatching{JSONObject(NativeBridge.checkSendmeRelay())}
                runOnUiThread{
                    if(isDestroyed)return@runOnUiThread
                    checkSendme.isEnabled=true
                    sendmeResults.text=outcome.fold({result->
                        if(result.optBoolean("reachable"))"Reachable · ${result.getJSONArray("relays").join(", ")}\nIroh relay connection established. Transfers may use Direct or Relay."
                        else result.optString("message","Sendme relay could not be reached.")
                    },{"Check failed: ${it.message}"})
                }
            }.start()
        }
        settingsDialog=AlertDialog.Builder(this).setTitle("Settings").setView(ScrollView(this).apply{addView(panel)})
            .setPositiveButton("Done"){_,_->if(!configurationLocked())AppSettings.setRelay(this,custom.text.toString())}.create().also{dialog->
                dialog.setOnDismissListener{if(!configurationLocked())AppSettings.setRelay(this,custom.text.toString());settingsFolder=null;settingsBackground=null;batteryButton=null;notificationsButton=null;settingsDialog=null;settingsConfiguration.clear()};dialog.show();lockConfiguration(configurationLocked())
            }
    }
    private fun showStorageCleanup(manual:Boolean){
        if(configurationLocked() || TransferService.isBusy()){
            if(manual)alert("End the active or paused connection before cleaning transfer storage.");return
        }
        if(cleanupScanning)return
        val extra=sources.filter{it.scheme=="file"}.mapNotNull{it.path}
        cleanupScanning=true
        if(manual)Toast.makeText(this,"Checking transfer storage…",Toast.LENGTH_SHORT).show()
        Thread{
            val result=runCatching{TransferCleanup.scan(this,extra)}
            runOnUiThread{
                cleanupScanning=false;if(isDestroyed || configurationLocked() || TransferService.isBusy())return@runOnUiThread
                result.onFailure{if(manual)alert(it.message ?: "Could not check transfer storage")}.onSuccess{report->
                    val candidates=if(manual)report.candidates else report.candidates.filter{it.bytes>1024*1024}
                    val bytes=candidates.sumOf{it.bytes}
                    if(!manual && candidates.isEmpty()){
                        if(report.reclaimed>0)Toast.makeText(this,"Freed ${ProgressEvents.size(report.reclaimed)} of saved working copies",Toast.LENGTH_LONG).show()
                        return@onSuccess
                    }
                    val message="Freed ${ProgressEvents.size(report.reclaimed)} automatically.\nUnused transfer data: ${ProgressEvents.size(bytes)}.\nKept for the current resume/restart: ${ProgressEvents.size(report.protectedBytes)}.\n\nDeleting unused data removes older transfer caches and temporary copies. Those older transfers will need their sources again. Files saved to Downloads or your chosen folder stay intact.\n\nTo remove the current transfer's saved resume data too, clear its transfer card first, then check storage again."
                    val dialog=AlertDialog.Builder(this).setTitle("Transfer storage").setMessage(message).setNegativeButton("Keep",null)
                    if(candidates.isNotEmpty())dialog.setPositiveButton("Delete unused data"){_,_->
                        cleanupBusy=true;lockConfiguration(true)
                        Thread{
                            val removed=runCatching{TransferCleanup.deleteUnused(this,candidates,extra)}
                            runOnUiThread{cleanupBusy=false;if(!isDestroyed){lockConfiguration(configurationLocked());removed.onSuccess{alert("Freed ${ProgressEvents.size(it)}. Saved files and current resume/restart data were kept.")}.onFailure{alert(it.message ?: "Cleanup could not finish")}}}
                        }.start()
                    }
                    dialog.show()
                }
            }
        }.start()
    }
    private fun selectTab(value:String){tab=value;updatePanels()}
    private fun updatePanels(){
        val selected=mode.selectedItem.toString()
        if(codeMode!=selected){
            codeMode=selected;sendCode.setText(AppSettings.code(this,selected))
        }
        sendTab.background=rounded(if(tab=="send")accent else Color.rgb(28,28,28));sendTab.setTextColor(if(tab=="send")Color.BLACK else ink)
        receiveTab.background=rounded(if(tab=="receive")accent else Color.rgb(28,28,28));receiveTab.setTextColor(if(tab=="receive")Color.BLACK else ink)
        engineDescription.text=when(mode.selectedItem){"Croc"->"Short code · files, text and browser sharing";"Sendme"->"Ticket · resumable · multiple receivers";else->"Short code · fast, resumable transfer"}
        sendPanel.visibility=if(tab=="send")View.VISIBLE else View.GONE
        receivePanel.visibility=if(tab=="receive")View.VISIBLE else View.GONE
        sendCode.visibility=if(mode.selectedItem=="Sendme")View.GONE else View.VISIBLE
        sendCodeActions.visibility=sendCode.visibility
        sendRecent.visibility=if(AppSettings.recent(this,selected,"send").isNotEmpty())View.VISIBLE else View.GONE
        receiveRecent.visibility=if(selected!="Sendme" && AppSettings.recent(this,selected,"receive").isNotEmpty())View.VISIBLE else View.GONE
        val croc=mode.selectedItem=="Croc"
        contentSwitch.visibility=if(croc)View.VISIBLE else View.GONE
        val textMode=croc && sendKind=="text"
        fileSelectionPanel.visibility=if(textMode)View.GONE else View.VISIBLE
        message.visibility=if(textMode)View.VISIBLE else View.GONE
        filesChoice.background=rounded(if(!textMode)accent else Color.rgb(28,28,28));filesChoice.setTextColor(if(!textMode)Color.BLACK else ink)
        textChoice.background=rounded(if(textMode)accent else Color.rgb(28,28,28));textChoice.setTextColor(if(textMode)Color.BLACK else ink)
        updatePrimaryLabel()
    }
    private fun scan(){
        require(TransferService.state(this).optString("status") !in listOf("running","stopping")){"Pause the current transfer before scanning"}
        scanner.launch(Intent(this,QrScannerActivity::class.java))
    }
    private fun normalizeInput(){
        val value=code.text.toString().trim()
        if(NativeBridge.hash(value).isNotEmpty()){mode.setSelection(1);code.setText(value)}
        else if(value.contains("://")){val parsed=QrSharing.parse(value);code.setText(parsed.value);if(parsed.crocLink)mode.setSelection(2)}
    }
    private fun shareValue():String {
        val saved=TransferService.state(this)
        require(saved.optString("action")=="send" && saved.optString("status")=="running" && !saved.optBoolean("troubleshooting")){"Start sharing first"}
        return (if(saved.optString("mode")=="Sendme")saved.optString("ticket") else saved.optString("code")).also{require(it.isNotEmpty()){"Preparing sharing code…"}}
    }
    private fun showQr(){
        val value=shareValue();val matrix=QrSharing.matrix(value)
        val pixels=IntArray(matrix.width*matrix.height){i->if(matrix[i%matrix.width,i/matrix.width])Color.BLACK else Color.WHITE}
        val image=ImageView(this).apply{setImageBitmap(Bitmap.createBitmap(pixels,matrix.width,matrix.height,Bitmap.Config.ARGB_8888));adjustViewBounds=true;setPadding(dp(12),dp(12),dp(12),dp(12));setBackgroundColor(Color.WHITE)}
        qrDialog?.dismiss();qrDialog=AlertDialog.Builder(this).setTitle("${TransferService.state(this).optString("mode")} sharing QR").setView(image).setPositiveButton("Close",null).show()
    }
    override fun onSaveInstanceState(out:Bundle){out.putString("sendKind",sendKind);out.putString("tab",tab);out.putInt("mode",mode.selectedItemPosition);out.putString("message",message.text.toString());out.putString("code",code.text.toString());out.putStringArrayList("sources",ArrayList(sources.map{it.toString()}));super.onSaveInstanceState(out)}
    private fun launch(action:String) {
        val state=TransferService.state(this)
        require(!configurationLocked()){"Stop or pause the active transfer first"}
        if(action=="receive") {
            normalizeInput()
            if(Build.VERSION.SDK_INT<=28 && AppSettings.folder(this).isEmpty() && checkSelfPermission(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)!=android.content.pm.PackageManager.PERMISSION_GRANTED){requestPermissions(arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE),22);return}
            val entered=code.text.toString().trim()
            if(state.optString("action")=="receive" && state.optString("mode")==mode.selectedItem.toString() && state.optString("status") in listOf("paused","interrupted")) {
                val old=if(mode.selectedItem=="Sendme")state.optString("ticket") else state.optString("code")
                val same=entered==old || (mode.selectedItem=="Sendme" && NativeBridge.hash(entered).isNotEmpty() && NativeBridge.hash(entered)==NativeBridge.hash(old))
                if(same){if(mode.selectedItem=="Sendme")state.put("ticket",entered);start(state);return}
            }
        }
        val sendingText=mode.selectedItem=="Croc" && sendKind=="text"
        val c=JSONObject().put("id",UUID.randomUUID().toString()).put("mode",mode.selectedItem.toString()).put("action",action).put("code",if(mode.selectedItem=="Sendme") "" else if(action=="send")sendCode.text.toString().trim() else code.text.toString().trim()).put("ticket",if(mode.selectedItem=="Sendme")code.text.toString().trim() else "").put("text",if(sendingText)message.text.toString() else "").put("uris",JSONArray(if(sendingText)emptyList<String>() else sources.map{it.toString()})).put("relay",AppSettings.relay(this))
        if(action=="receive") require(code.text.isNotBlank()){"Enter a code or ticket"}
        if(action=="send") require(if(sendingText)message.text.isNotBlank() else sources.isNotEmpty()){if(sendingText)"Enter text to send" else "Select files or a folder"}
        if(mode.selectedItem!="Croc") c.put("text","")
        if(action=="send" && mode.selectedItem!="Sendme")AppSettings.setCode(this,mode.selectedItem.toString(),sendCode.text.toString().trim())
        start(c)
    }
    private fun restartSender(){
        require(!configurationLocked()){"Stop the active transfer first"}
        val saved=TransferService.state(this)
        require(saved.optString("action")=="send"){"This is a receive transfer; use Resume"}
        val selected=saved.optString("mode")
        val text=if(selected=="Croc")saved.optString("text") else ""
        val uris=saved.optJSONArray("uris") ?: JSONArray()
        val cached=saved.optJSONArray("paths")
        val reusable=!saved.optBoolean("referencedSources") && cached!=null && cached.length()>0 && (0 until cached.length()).all{File(cached.getString(it)).exists()}
        if(uris.length()==0 && text.isBlank())saved.optJSONArray("paths")?.let{paths->
            for(index in 0 until paths.length()){
                val file=File(paths.getString(index))
                if(file.exists())uris.put(Uri.fromFile(file).toString())
            }
        }
        require(uris.length()>0 || text.isNotBlank()){"The previous files are no longer available. Select them again to start sharing."}
        val next=JSONObject().put("id",UUID.randomUUID().toString()).put("action","send").put("mode",selected)
            .put("code",if(selected=="Sendme")"" else saved.optString("code"))
            .put("text",text).put("uris",uris).put("relay",AppSettings.relay(this))
        if(reusable && text.isBlank())next.put("paths",cached)
        sources.clear();for(index in 0 until uris.length())sources.add(Uri.parse(uris.getString(index)))
        restartedNames.clear()
        if(reusable && cached!!.length()==sources.size)sources.forEachIndexed{index,uri->restartedNames[uri]=File(cached.getString(index)).name}
        sendKind=if(text.isNotBlank())"text" else "files"
        mode.setSelection(listOf("EasySendMe","Sendme","Croc").indexOf(selected).coerceAtLeast(0))
        message.setText(text);sendCode.setText(next.optString("code"));refreshSelection();selectTab("send")
        start(next)
    }
    private fun resume(replace:Boolean) {
        val c=TransferService.state(this)
        require(c.has("id")){"No interrupted transfer"}
        require(c.optString("status") !in listOf("running","stopping")){"Transfer is still running"}
        if(replace) {
            val entry=EditText(this).apply{hint=if(c.optString("mode")=="Sendme")"New ticket" else "New short code";setText(code.text)}
            AlertDialog.Builder(this).setTitle("Reconnect saved transfer").setMessage("Use a new ticket only for the same original files. A different transfer keeps a separate cache.").setView(entry).setNegativeButton("Cancel",null).setPositiveButton("Reconnect"){_,_->
                if(entry.text.isNotBlank()) {
                    if(c.optString("mode")=="Sendme") {
                        val next=entry.text.toString().trim()
                        if(NativeBridge.hash(next).isEmpty() || NativeBridge.hash(next)!=NativeBridge.hash(c.optString("ticket"))) {alert("This ticket is invalid or belongs to different files. Clear the transfer card and start a new receive.");return@setPositiveButton}
                        c.put("ticket",next)
                    } else c.put("code",entry.text.toString().trim()).put("refresh",true)
                    start(c)
                }
            }.show()
        } else {
            val entered=code.text.toString().trim()
            if(c.optString("action")=="receive" && entered.isNotEmpty()){
                if(c.optString("mode")=="Sendme" && NativeBridge.hash(entered).isNotEmpty()){
                    require(NativeBridge.hash(entered)==NativeBridge.hash(c.optString("ticket"))){"This ticket is for different files. Clear the transfer card and start a new receive."};c.put("ticket",entered)
                } else if(c.optString("mode") in listOf("EasySendMe","Croc") && entered!=c.optString("code")){
                    require(c.optString("mode")!="Croc"){"Use the original Croc code to resume. For a different code, tap Receive to start a new transfer."}
                    c.put("code",entered)
                    if(c.optString("mode")=="EasySendMe")c.put("refresh",true)
                }
            }
            start(c)
        }
    }
    private fun start(c:JSONObject){
        val current=TransferService.state(this)
        if(current.optBoolean("connectionRetained") && c.optString("mode")=="Croc" && current.optString("id")==c.optString("id") && current.optString("status")=="paused"){
            require(current.optString("code")==c.optString("code")){"Tap End transfer to close the paused connection, then tap Resume with the new code."}
            startService(Intent(this,TransferService::class.java).setAction("resume"));return
        }
        c.remove("log");c.remove("message");pendingStartId=c.optString("id");lockConfiguration(true)
        try{startForegroundService(Intent(this,TransferService::class.java).putExtra("config",c.toString()));status.text="Starting…"}
        catch(error:Exception){pendingStartId="";lockConfiguration(configurationLocked());throw error}
    }
    private fun render() {
        val s=TransferService.state(this);val value=s.toString();if(value==previous)return;previous=value
        if(s.optBoolean("awaitingCacheChoice") && s.optString("status")=="running") {
            if(cacheDialog==null) {
                val info=s.getJSONObject("cacheInfo")
                val names=info.optJSONArray("names")?.let{a->(0 until a.length()).map{a.getString(it)}.joinToString("\n")}?.takeIf{it.isNotBlank()} ?: "Transfer ${NativeBridge.hash(s.optString("ticket")).take(12)}"
                val complete=info.optBoolean("complete")
                fun choose(action:String){startService(Intent(this,TransferService::class.java).setAction(action).putExtra("id",s.optString("id")))}
                cacheDialog=AlertDialog.Builder(this).setTitle(if(complete)"Complete transfer data available" else "Partial transfer data available")
                    .setMessage("$names\n\n${ProgressEvents.size(info.optLong("bytes"))} saved on this phone. "+(if(complete)"Continue to verify and save these files without downloading them again." else "Continue to download only the missing data. Keep the sender sharing.")+"\n\nDeleting saved data starts the download over. Files already saved to Downloads or your chosen folder are kept.")
                    .setPositiveButton("Continue"){_,_->choose("continue-cache")}
                    .setNeutralButton("Delete and start over"){_,_->choose("delete-cache")}
                    .setNegativeButton("Cancel"){_,_->startService(Intent(this,TransferService::class.java).setAction("pause"))}
                    .setCancelable(false).create().also{dialog->dialog.setOnDismissListener{cacheDialog=null};dialog.show()}
            }
        } else {cacheDialog?.dismiss();cacheDialog=null}
        if(s.optString("id")==pendingStartId)pendingStartId=""
        transferPanel.visibility=if(s.optString("id").isNotEmpty())View.VISIBLE else View.GONE
        if(s.optString("id").isNotEmpty() && s.optString("id")!=renderedSession){
            renderedSession=s.optString("id");tab=s.optString("action","send")
            mode.setSelection(listOf("EasySendMe","Sendme","Croc").indexOf(s.optString("mode")).coerceAtLeast(0))
            if(tab=="receive")code.setText(s.optString(if(s.optString("mode")=="Sendme")"ticket" else "code"))
            updatePanels()
        }
        val running=s.optString("status")=="running";val stopping=s.optString("status")=="stopping"
        val paused=s.optString("status") in listOf("paused","interrupted")
        val receiving=s.optString("action")=="receive"
        transferDirection.text="${s.optString("mode")} · ${if(receiving)"Receive on this phone" else "Send from this phone"}"
        if(!receiving && s.optString("mode")=="Croc" && s.optString("status")=="complete" && retainedSession!=s.optString("id")){
            retainedSession=s.optString("id");if(mode.selectedItem=="Croc")sendCode.setText(AppSettings.code(this,"Croc"))
        }
        sendCode.isEnabled=!running && !stopping;sendCodeActions.isEnabled=!running && !stopping
        for(index in 0 until sendCodeActions.childCount)sendCodeActions.getChildAt(index).isEnabled=!running && !stopping
        sendRecent.visibility=if(AppSettings.recent(this,mode.selectedItem.toString(),"send").isNotEmpty())View.VISIBLE else View.GONE
        receiveRecent.visibility=if(mode.selectedItem!="Sendme" && AppSettings.recent(this,mode.selectedItem.toString(),"receive").isNotEmpty())View.VISIBLE else View.GONE
        status.text=TransferLabels.title(s)
        val paths=s.optJSONObject("senderRoutes")
        val pathValues=paths?.keys()?.asSequence()?.map{paths.optString(it)}?.toList() ?: emptyList()
        val activeRoute=if(s.optString("mode")!="Croc" && !receiving) {
            if(pathValues.isEmpty())"Waiting for receiver" else TransferRoute.label(pathValues)
        } else TransferRoute.label(listOf(s.optString("route","unknown")))
        route.text="${if(!running && !stopping)"Connection used" else "Connection"} · $activeRoute";route.visibility=if(running || stopping || (activeRoute !in listOf("Waiting for receiver","Connecting…","Unknown") && (s.has("route") || pathValues.isNotEmpty())))View.VISIBLE else View.GONE
        val preparing=s.optBoolean("preparing")
        val total=s.optLong(if(preparing)"prepTotal" else "total");val done=s.optLong(if(preparing)"prepDone" else "done")
        val speed=s.optDouble(if(preparing)"prepSpeed" else "speed").takeIf{it.isFinite() && it>=0} ?: 0.0
        val percent=if(total>0 || s.optInt("filesTotal")>0)ProgressEvents.percent(done,total,s.optInt("filesDone"),s.optInt("filesTotal")) else s.optInt("percent")
        val progress=if(total>0 || s.optInt("filesTotal")>0)"$percent% · ${ProgressEvents.size(done)} of ${ProgressEvents.size(total)}" else if(s.has("percent"))"$percent% · ${s.optString("progress")}" else s.optString("progress")
        val rate=if(running && speed>0)"${ProgressEvents.size(speed.toLong())}/s"+ (ProgressEvents.eta(done,total,speed)?.let{" · $it left"} ?: "") else ""
        val files=if(s.optInt("filesTotal")>0)"${s.optInt("filesDone")} / ${s.optInt("filesTotal")} files" else ""
        detail.text=listOf(TransferLabels.message(s),progress,files,rate,TransferTiming.elapsedText(s),TransferTiming.averageText(s)).filter{it.isNotEmpty()}.joinToString("\n")
        bar.isIndeterminate=running && total==0L && !s.has("percent") && s.optInt("filesTotal")==0
        bar.progress=percent
        bar.visibility=if(running || (paused && receiving) || stopping)View.VISIBLE else View.GONE
        lockConfiguration(configurationLocked())
        pause.visibility=if(running || stopping)View.VISIBLE else View.GONE;pause.isEnabled=!stopping;pause.text=if(receiving)"Pause" else "Stop sharing"
        resumeButton.visibility=if(paused && receiving)View.VISIBLE else View.GONE
        restartSharing.visibility=if(!receiving && s.optString("status") in listOf("interrupted","stopped","complete"))View.VISIBLE else View.GONE
        restartSharing.text=if(s.optString("status")=="complete")"Share again" else "Restart sharing"
        endPaused.visibility=if(paused && s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        newTransfer.visibility=if(!running && !stopping && !s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        clearTransferNote.visibility=newTransfer.visibility
        clearTransferNote.text=if(paused && receiving)"Clears this card and its Resume entry. Saved files remain." else "Clears this card. Saved files remain."
        sharing.visibility=if(running && !receiving)View.VISIBLE else View.GONE
        if(s.optBoolean("troubleshooting")){qrDialog?.dismiss()}
        relayNotice.text=s.optString("relayNotice");relayNotice.visibility=if(relayNotice.text.isEmpty())View.GONE else View.VISIBLE
        sendmeFallback.visibility=if(s.optBoolean("suggestSendme"))View.VISIBLE else View.GONE;sendmeFallback.isEnabled=!configurationLocked()
        val replacement=s.optString("codeRevision");if(replacement.isNotEmpty() && replacement!=lastReplacement){qrDialog?.dismiss();sendCode.setText(replacement);lastReplacement=replacement}
        ticket.text=if(running && !receiving && !s.optBoolean("troubleshooting"))if(s.optString("mode")=="Sendme")s.optString("ticket") else s.optString("code") else ""
        receivedText.text=s.optString("text");receivedText.visibility=if(receivedText.text.isEmpty())View.GONE else View.VISIBLE;receivedTextTitle.visibility=receivedText.visibility
    }
    private fun alert(s:String){AlertDialog.Builder(this).setMessage(s).setPositiveButton("OK",null).show()}
    @Deprecated("Legacy result API") override fun onActivityResult(request:Int,result:Int,data:Intent?){
        super.onActivityResult(request,result,data);if(request !in listOf(10,11,12) || result!=RESULT_OK || data==null)return
        if(request==12){val source=File(TransferService.state(this).optString("output"));val uri=data.data ?: return
            Thread {runCatching{Storage.export(this,source,uri)}.onSuccess{runOnUiThread{alert("Saved received files")}}.onFailure{runOnUiThread{alert(it.message ?: "Export failed")}}}.start();return}
        if(configurationLocked()){Toast.makeText(this,"Finish or pause the transfer before changing files",Toast.LENGTH_SHORT).show();return}
        val incoming=mutableListOf<Uri>()
        data.clipData?.let{for(i in 0 until it.itemCount)incoming.add(it.getItemAt(i).uri)} ?: data.data?.let{incoming.add(it)}
        sources.addAll(incoming.distinct().filter{it !in sources})
        incoming.forEach{restartedNames.remove(it)}
        sources.forEach{runCatching{contentResolver.takePersistableUriPermission(it,Intent.FLAG_GRANT_READ_URI_PERMISSION)}}
        refreshSelection()
    }
    @Suppress("DEPRECATION") private fun acceptShare(i:Intent){
        if(i.action !in listOf(Intent.ACTION_SEND,Intent.ACTION_SEND_MULTIPLE,Intent.ACTION_PROCESS_TEXT))return
        val incoming=mutableListOf<Uri>()
        if(i.action==Intent.ACTION_SEND_MULTIPLE)incoming.addAll(i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM) ?: arrayListOf())
        else i.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let{incoming.add(it)}
        if(incoming.isEmpty())i.clipData?.let{clip->for(index in 0 until clip.itemCount)clip.getItemAt(index).uri?.let{incoming.add(it)}}
        val text=i.getCharSequenceExtra(if(i.action==Intent.ACTION_PROCESS_TEXT)Intent.EXTRA_PROCESS_TEXT else Intent.EXTRA_TEXT)?.toString()
            ?: if(incoming.isEmpty())i.clipData?.getItemAt(0)?.text?.toString().orEmpty() else ""
        if(incoming.isEmpty() && text.isBlank())return
        if(configurationLocked()){Toast.makeText(this,"Finish or pause the transfer before changing files",Toast.LENGTH_LONG).show();return}
        sources.clear();sources.addAll(incoming.distinct())
        restartedNames.clear()
        if(sources.isEmpty()){
            sendKind="text";mode.setSelection(2);message.setText(text)
        } else {
            // Some apps attach a caption too. Sending files must not silently switch to text.
            sendKind="files";message.setText("")
        }
        refreshSelection();selectTab("send")
    }
    override fun onNewIntent(intent:Intent){
        super.onNewIntent(intent);setIntent(intent)
        renderedSession=TransferService.state(this).optString("id")
        acceptShare(intent)
    }
    override fun onResume(){super.onResume();updateBackgroundStatus();handler.post(ticker);if(!cleanupScanned){cleanupScanned=true;showStorageCleanup(false)}}
    override fun onPause(){handler.removeCallbacks(ticker);super.onPause()}
    override fun onDestroy(){cacheDialog?.dismiss();cacheDialog=null;super.onDestroy()}
}
