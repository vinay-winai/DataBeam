package com.vinaywinai.databeam
import android.app.*
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.doOnPreDraw
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
    private var reconnectDialog:AlertDialog?=null
    private var resumeDialog:AlertDialog?=null
    private var crocHistoryDialog:AlertDialog?=null
    private var crocHistoryInput:EditText?=null
    private var crocHistoryId:String?=null
    private var crocReviewDialog:AlertDialog?=null
    private var shownCrocReview=""
    private var answeredCrocReview=""
    private var cleanupBusy=false
    private var cleanupScanning=false
    private var cleanupScanned=false
    private var settingsFolder:TextView?=null
    private var settingsBackground:TextView?=null
    private var batteryButton:Button?=null
    private var notificationsButton:Button?=null
    private var sendmeEngineToggle:Switch?=null
    private var crocEngineToggle:Switch?=null
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
            acceptScannedCode(raw)
        }.onFailure{alert(it.message ?: "Could not read this QR")}}
    }
    private val crocHistoryScanner=registerForActivityResult(ActivityResultContracts.StartActivityForResult()){result->
        if(result.resultCode==RESULT_OK)result.data?.getStringExtra(QrScannerActivity.RESULT_CODE)?.let{raw->runCatching{
            crocHistoryInput?.setText(crocResumeCode(raw))
        }.onFailure{alert(it.message ?: "Scan the sender’s Croc code")}}
    }
    private lateinit var mode:Spinner
    private lateinit var easyMode:CheckBox
    private lateinit var code:EditText
    private lateinit var receiveInputTitle:TextView
    private lateinit var sendCode:EditText
    private lateinit var sendCodeActions:LinearLayout
    private lateinit var sendRecent:Button
    private lateinit var receiveRecent:Button
    private var codeMode=""
    private var retainedSession=""
    private lateinit var message:EditText
    private lateinit var clearText:Button
    private lateinit var saveLocation:TextView
    private lateinit var status:TextView
    private lateinit var ticket:TextView
    private lateinit var relayNotice:TextView
    private lateinit var sendmeFallback:Button
    private var lastReplacement=""
    private lateinit var receivedText:TextView
    private lateinit var receivedTextTitle:TextView
    private lateinit var selection:TextView
    private lateinit var clearFiles:Button
    private lateinit var copyReceivedText:Button
    private lateinit var receiveTextTicket:Button
    private var detectedTextTicket=""
    private lateinit var screenScroll:ScrollView
    private var scrolledTransfer=""
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
    private val brandAccent=Color.rgb(255,77,0)
    private var accent=brandAccent
    private val accentButtons=mutableListOf<Button>()
    private lateinit var sendPanel:LinearLayout
    private lateinit var receivePanel:LinearLayout
    private lateinit var primary:Button
    private lateinit var pause:Button
    private lateinit var resumeButton:Button
    private lateinit var resend:Button
    private lateinit var openReceived:Button
    private var clearedPayloadSession=""
    private var retainedCodeSession=""
    private var showedRetainedSelection=false
    private val notificationPermission=registerForActivityResult(ActivityResultContracts.RequestPermission()){updateBackgroundStatus()}
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
    private lateinit var historyButton:Button
    private lateinit var engineDescription:TextView
    private lateinit var broadcastChoice:CheckBox
    private lateinit var quickActions:Button
    private var quickMenu:PopupMenu?=null
    private lateinit var browserGuidance:TextView
    private var browserSend=false
    private lateinit var troubleshootButton:Button
    private lateinit var troubleshootBadge:View
    private fun dp(value:Int)=(resources.displayMetrics.density*value).toInt()
    private fun rounded(color:Int)=android.graphics.drawable.GradientDrawable().apply{setColor(color);cornerRadius=dp(14).toFloat()}
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        NativeBridge.initialize(applicationContext)
        RelayRecovery.initializeWordList(assets.open("eff-short-wordlist-1.txt").bufferedReader().use{it.readText()})
        sendKind=savedInstanceState?.getString("sendKind") ?: "files"
        val current=TransferService.state(this)
        if(savedInstanceState==null && current.optString("action")=="send" && current.optString("status") in listOf("interrupted","stopped") && current.optString("mode") in listOf("Croc","EasySendMe") && current.optString("code").isNotEmpty())
            AppSettings.setCode(this,current.optString("mode"),current.optString("code"))
        clearedPayloadSession=savedInstanceState?.getString("clearedPayloadSession") ?: current.optString("id").takeIf{current.optString("status")=="complete"}.orEmpty()
        retainedCodeSession=savedInstanceState?.getString("retainedCodeSession") ?: current.optString("id").takeIf{current.optString("status") in listOf("interrupted","stopped")}.orEmpty()
        browserSend=savedInstanceState?.getBoolean("browserSend") ?: (current.optString("action")=="send" && current.optBoolean("browserSend"))
        renderedSession=current.optString("id")
        tab=savedInstanceState?.getString("tab") ?: current.optString("action","send")
        val scroll=ScrollView(this).apply{setBackgroundColor(Color.BLACK);isFillViewport=true}
        screenScroll=scroll
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
            if(emphasized)accentButtons.add(this)
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
            setOnClickListener{showSettings("background")}
        }
        quickActions=button(layout,"Quick actions ▾"){showQuickActions()}.apply{
            (layoutParams as LinearLayout.LayoutParams).bottomMargin=dp(14)
        }
        browserGuidance=label(layout,"On the receiving device, open https://getcroc.com",14f).apply{
            setTextColor(accent);setLinkTextColor(accent);visibility=View.GONE
            android.text.util.Linkify.addLinks(this,android.text.util.Linkify.WEB_URLS)
            movementMethod=android.text.method.LinkMovementMethod.getInstance()
        }
        val tabs=row(layout)
        sendTab=button(tabs,"Send"){selectTab("send")};receiveTab=button(tabs,"Receive"){selectTab("receive")}
        val engine=card(layout);enginePanel=engine;label(engine,"Transfer engine",12f)
        val engines=EngineOptions.engines(AppSettings.disabledEngine(this))
        val savedMode=savedInstanceState?.getString("modeName")
            ?: current.optString("mode").takeIf{current.optString("status") in listOf("running","stopping") || current.optBoolean("connectionRetained")}
            ?: "Croc"
        val initialMode=savedMode.takeIf{it in AppSettings.modes(this)} ?: engines.first()
        mode=Spinner(this).apply{
            adapter=ArrayAdapter(this@MainActivity,android.R.layout.simple_spinner_dropdown_item,engines)
            engine.addView(this)
        }
        mode.setSelection(engines.indexOf(if(initialMode=="EasySendMe")"Sendme" else initialMode))
        easyMode=CheckBox(this).apply{
            text="EasySendMe · use a short code"
            isChecked=initialMode=="EasySendMe"
            setTextColor(ink)
            engine.addView(this)
        }
        engineDescription=label(engine,"Short code · resumable transfer",12f).apply{setTextColor(Color.LTGRAY)}
        sendPanel=card(layout)
        label(sendPanel,"1 · Choose what to send",17f)
        contentSwitch=row(sendPanel)
        filesChoice=button(contentSwitch,"Files"){sendKind="files";updatePanels()}
        textChoice=button(contentSwitch,"Text"){sendKind="text";browserSend=false;updatePanels()}
        fileSelectionPanel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;sendPanel.addView(this)}
        selection=label(fileSelectionPanel,"No files selected")
        clearFiles=button(fileSelectionPanel,"Clear all"){sources.clear();restartedNames.clear();refreshSelection()}.apply{
            textSize=12f;layoutParams=LinearLayout.LayoutParams(-2,dp(36)).apply{gravity=Gravity.END;bottomMargin=dp(6)}
        }
        selectionList=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;fileSelectionPanel.addView(this)}
        val pick=row(fileSelectionPanel)
        button(pick,"Add files"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").putExtra(Intent.EXTRA_ALLOW_MULTIPLE,true).addCategory(Intent.CATEGORY_OPENABLE),10)}
        button(pick,"Add folder"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),11)}
        message=input(sendPanel,"Text to send")
        message.apply{setSingleLine(false);minLines=2;maxLines=5;gravity=Gravity.TOP;layoutParams=LinearLayout.LayoutParams(-1,-2);setText(savedInstanceState?.getString("message") ?: "")}
        button(sendPanel,"Clear text"){message.setText("")}.also{clearText=it}
        sendCode=input(sendPanel,"Custom sharing code (optional)")
        sendCode.addTextChangedListener(object:android.text.TextWatcher{
            override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){}
            override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){if(codeMode.isNotEmpty() && codeMode!="Sendme")AppSettings.setCode(this@MainActivity,codeMode,s.toString())}
            override fun afterTextChanged(s:android.text.Editable?){}
        })
        sendCodeActions=row(sendPanel)
        sendRecent=button(sendCodeActions,"Recent codes"){showRecent("send",sendCode)}
        button(sendCodeActions,"Clear"){sendCode.setText("");AppSettings.setCode(this,selectedMode(),"")}
        broadcastChoice=CheckBox(this).apply{
            text="Broadcast · keep sharing for multiple receivers";setTextColor(ink)
            buttonTintList=android.content.res.ColorStateList.valueOf(accent)
            isChecked=savedInstanceState?.getBoolean("broadcast") ?: if(current.optString("mode")=="Sendme" && current.optString("action")=="send" && current.optString("status") in listOf("running","stopping"))current.optBoolean("broadcast") else AppSettings.broadcast(this@MainActivity)
            setOnCheckedChangeListener{_,checked->if(!configurationLocked())AppSettings.setBroadcast(this@MainActivity,checked)}
            sendPanel.addView(this)
        }
        receivePanel=card(layout)
        receiveInputTitle=label(receivePanel,"1 · Enter the sender’s code",17f)
        code=input(receivePanel,"Paste a code or Sendme ticket")
        code.setText(if(savedInstanceState?.getString("modeName")==selectedMode())savedInstanceState.getString("code") else if(tab=="receive" && current.optString("mode")==selectedMode())receivedCodeText(current) else "")
        val receiveActions=row(receivePanel)
        button(receiveActions,"Paste"){val clipboard=getSystemService(CLIPBOARD_SERVICE) as ClipboardManager;code.setText(clipboard.primaryClip?.getItemAt(0)?.coerceToText(this));normalizeInput()}
        button(receiveActions,"Scan QR"){scan()}
        button(receiveActions,"Clear"){code.setText("")}
        receiveRecent=button(receivePanel,"Recent codes"){showRecent("receive",code)}
        saveLocation=label(receivePanel,"",12f).apply{setTextColor(accent)}
        historyButton=button(receivePanel,"Croc receive history"){showCrocHistory()}
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
        resumeButton=button(transfer,"Resume",true){showResume()}
        resend=button(transfer,"Resend",true){restoreSendPayload()}
        openReceived=button(transfer,"Open folder",true){openReceivedFolder()}
        endPaused=button(transfer,"End transfer"){startService(Intent(this,TransferService::class.java).setAction("stop"))}
        newTransfer=button(transfer,"Clear transfer card"){val saved=TransferService.state(this);require(saved.optString("status") !in listOf("running","stopping") && !saved.optBoolean("connectionRetained")){"End the current transfer first"};getSharedPreferences("transfer",0).edit().remove("state").apply();previous="";renderedSession="";code.setText("");render()}
        clearTransferNote=label(transfer,"Clears this card. Saved files remain.",12f).apply{setTextColor(Color.LTGRAY)}
        sendmeFallback=button(transfer,"Use Sendme instead"){selectMode("Sendme");code.setText("");alert("Sendme selected. Ask the sender for its ticket, or select files and press Send.")}
        receivedTextTitle=label(transfer,"Received text",12f)
        receivedText=label(transfer,"").apply{setTextIsSelectable(true)}
        copyReceivedText=button(transfer,"Copy text"){
            (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Received text",receivedText.text))
            Toast.makeText(this,"Text copied",Toast.LENGTH_SHORT).show()
        }
        receiveTextTicket=button(transfer,"Receive with Sendme",true){
            require(detectedTextTicket.isNotEmpty()){"No valid Sendme ticket in this text"}
            val value=detectedTextTicket
            selectMode("Sendme");selectTab("receive");code.setText(value);launch("receive")
        }
        val helpRow=row(screen).apply{gravity=Gravity.CENTER_VERTICAL;setPadding(dp(20),dp(4),dp(20),dp(8))}
        @Suppress("DEPRECATION") val version=packageManager.getPackageInfo(packageName,0).versionName.orEmpty()
        label(helpRow,"v$version",12f).apply{contentDescription="Version $version";setTextColor(Color.LTGRAY);layoutParams=LinearLayout.LayoutParams(0,dp(48),1f);gravity=Gravity.CENTER_VERTICAL}
        val help=FrameLayout(this).also{helpRow.addView(it,LinearLayout.LayoutParams(dp(48),dp(48)))}
        troubleshootButton=Button(this).apply{
            stateListAnimator=null;elevation=0f;translationZ=0f
            text="?";textSize=20f;isAllCaps=false;setTextColor(ink);setPadding(0,0,0,0);background=rounded(Color.rgb(28,28,28));contentDescription="Troubleshoot"
            help.addView(this,FrameLayout.LayoutParams(-1,-1))
            setOnClickListener{runCatching{
                startActivity(Intent(Intent.ACTION_VIEW,Uri.parse(TroubleshootNews.GUIDE_URL)))
                TroubleshootNews.markRead(this@MainActivity);updateTroubleshootBadge()
            }.onFailure{alert("Could not open troubleshooting guidance")}}
        }
        troubleshootBadge=View(this).apply{
            background=rounded(accent);visibility=View.GONE;elevation=dp(4).toFloat();importantForAccessibility=View.IMPORTANT_FOR_ACCESSIBILITY_NO
            help.addView(this,FrameLayout.LayoutParams(dp(9),dp(9),Gravity.TOP or Gravity.END).apply{topMargin=dp(5);marginEnd=dp(5)})
        }
        updateTroubleshootBadge()
        savedInstanceState?.getStringArrayList("sources")?.forEach{sources.add(Uri.parse(it))}
        if((savedInstanceState==null || (sources.isEmpty() && current.optString("status") in listOf("running","stopping"))) && current.optString("action")=="send" && current.optString("id").isNotEmpty() && current.optString("status") in listOf("running","stopping")){
            val uris=current.optJSONArray("uris");val paths=current.optJSONArray("paths")
            if(uris!=null && uris.length()>0)for(i in 0 until uris.length()){
                val uri=Uri.parse(uris.getString(i));sources.add(uri)
                if(paths?.length()==uris.length())restartedNames[uri]=File(paths.getString(i)).name
            } else if(paths!=null)for(i in 0 until paths.length())sources.add(Uri.fromFile(File(paths.getString(i))))
            if(current.optString("mode")=="Croc" && current.optString("text").isNotEmpty()){sendKind="text";message.setText(current.getString("text"))}
        }
        easyMode.setOnCheckedChangeListener{_,_->updatePanels()}
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
        savedInstanceState?.getString("crocHistoryId")?.let{id->
            if(!TransferService.isBusy())TransferCleanup.crocReceiveHistory(this).firstOrNull{it.optString("id")==id}?.let{
                showCrocResume(it,savedInstanceState.getString("crocHistoryCode"))
            }
        }
        // Mark before launching so rotation or denial cannot produce a prompt loop.
        val permissions=getSharedPreferences("permissions",0)
        if(Build.VERSION.SDK_INT>=33 && !permissions.getBoolean("notificationsAsked",false)){
            permissions.edit().putBoolean("notificationsAsked",true).apply()
            if(checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)!=android.content.pm.PackageManager.PERMISSION_GRANTED)
                notificationPermission.launch(android.Manifest.permission.POST_NOTIFICATIONS)
        }
    }
    private fun updatePrimaryLabel(){
        if(::primary.isInitialized)primary.text=if(tab=="receive")"Receive" else if(selectedMode()=="Croc" && sendKind=="text")"Send text" else "Send files"
    }
    private fun configurationLocked()=cleanupBusy || pendingStartId.isNotEmpty() || TransferService.state(this).optString("status") in listOf("running","stopping")
    private fun enableTree(view:View,enabled:Boolean){
        view.isEnabled=enabled
        if(view is ViewGroup)for(index in 0 until view.childCount)enableTree(view.getChildAt(index),enabled)
    }
    private fun lockConfiguration(locked:Boolean){
        quickActions.isEnabled=!locked;quickActions.alpha=if(locked).45f else 1f
        for(panel in listOf(enginePanel,sendPanel,receivePanel)){enableTree(panel,!locked);panel.alpha=if(locked).55f else 1f}
        sendTab.isEnabled=!locked;receiveTab.isEnabled=!locked
        primary.isEnabled=!locked;primary.alpha=if(locked).45f else 1f
        settingsConfiguration.forEach{enableTree(it,!locked);it.alpha=if(locked).45f else 1f}
        listOfNotNull(sendmeEngineToggle,crocEngineToggle).forEach{it.isEnabled=!locked && !TransferService.isBusy()}
    }
    private fun sourceName(uri:Uri):String=restartedNames[uri] ?: runCatching{
        if(uri.scheme=="file")File(uri.path.orEmpty()).name else {
            val document=if(android.provider.DocumentsContract.isTreeUri(uri))android.provider.DocumentsContract.buildDocumentUriUsingTree(uri,android.provider.DocumentsContract.getTreeDocumentId(uri)) else uri
            Storage.name(this,document)
        }
    }.getOrElse{uri.lastPathSegment?.substringAfterLast('/') ?: "Selected item"}
    private fun refreshSelection(){
        clearFiles.visibility=if(sources.isEmpty())View.GONE else View.VISIBLE
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
        val selected=selectedMode();val recent=AppSettings.recent(this,selected,action)
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
    private fun showSettings()=showSettings(null)

    private fun showSettings(focusSection:String?){
        settingsConfiguration.clear()
        val panel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(22),dp(8),dp(22),dp(16))}
        val scroll=ScrollView(this).apply{addView(panel)}
        var hasExpandedLowerSection=false
        fun scrollToBottom(){scroll.doOnPreDraw{scroll.scrollTo(0,panel.height)}}
        fun section(id:String,title:String):LinearLayout {
            val header=Button(this).apply{
                isAllCaps=false;gravity=Gravity.START or Gravity.CENTER_VERTICAL
                setTextColor(ink);setPadding(dp(12),0,dp(12),0)
                background=rounded(Color.rgb(25,25,25))
                panel.addView(this,LinearLayout.LayoutParams(-1,dp(46)).apply{topMargin=dp(8)})
            }
            val body=LinearLayout(this).apply{
                orientation=LinearLayout.VERTICAL;setPadding(dp(8),dp(4),dp(8),dp(8))
                panel.addView(this)
            }
            fun expand(expanded:Boolean){
                body.visibility=if(expanded)View.VISIBLE else View.GONE
                header.text="${if(expanded)"▾" else "›"}  $title"
                header.contentDescription="$title, ${if(expanded)"expanded" else "collapsed"}"
            }
            val initiallyExpanded=id==focusSection || AppSettings.settingsSectionExpanded(this,id)
            expand(initiallyExpanded)
            if(id!="transfers" && initiallyExpanded)hasExpandedLowerSection=true
            header.setOnClickListener{
                val expanded=body.visibility!=View.VISIBLE
                expand(expanded)
                AppSettings.setSettingsSectionExpanded(this,id,expanded)
                if(id!="transfers" && expanded)scrollToBottom()
            }
            return body
        }
        fun label(parent:LinearLayout,value:String)=TextView(this).apply{text=value;setTextColor(ink);setPadding(0,dp(8),0,dp(6));parent.addView(this)}
        fun button(parent:LinearLayout,value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;parent.addView(this);setOnClickListener{runCatching(action).onFailure{alert(it.message ?: "Operation failed")}}}
        val transfers=section("transfers","Transfers")
        var updatingEngines=false
        fun toggle(engine:String)=Switch(this).apply{
            text="$engine enabled";isChecked=AppSettings.engineEnabled(this@MainActivity,engine);setTextColor(ink);transfers.addView(this)
            setOnCheckedChangeListener{_,enabled->
                if(!updatingEngines){
                    runCatching{
                        require(!configurationLocked() && !TransferService.isBusy()){"End the current transfer before changing engines"}
                        AppSettings.setEngineEnabled(this@MainActivity,engine,enabled)
                        refreshEngines()
                    }.onFailure{Toast.makeText(this@MainActivity,it.message ?: "Keep at least one engine enabled",Toast.LENGTH_LONG).show()}
                    updatingEngines=true
                    sendmeEngineToggle?.isChecked=AppSettings.engineEnabled(this@MainActivity,"Sendme")
                    crocEngineToggle?.isChecked=AppSettings.engineEnabled(this@MainActivity,"Croc")
                    updatingEngines=false
                }
            }
        }
        sendmeEngineToggle=toggle("Sendme");crocEngineToggle=toggle("Croc")
        label(transfers,"EasySendMe uses both engines.").textSize=12f
        settingsConfiguration.add(Switch(this).apply{
            text="Skip identical files when sending"
            isChecked=AppSettings.skipDuplicateFiles(this@MainActivity)
            setTextColor(ink);transfers.addView(this)
            setOnCheckedChangeListener{_,checked->AppSettings.setSkipDuplicateFiles(this@MainActivity,checked)}
        })
        label(transfers,"Applies to individually selected files. Folder contents are kept.").textSize=12f

        val storage=section("storage","Files and storage")
        button(storage,"Croc receive history"){
            settingsDialog?.dismiss()
            showCrocHistory()
        }
        label(storage,"Download folder")
        val locationRow=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;gravity=Gravity.CENTER_VERTICAL;storage.addView(this)}
        settingsFolder=TextView(this).apply{text=AppSettings.folderLabel(this@MainActivity);setTextColor(accent);setPadding(0,dp(10),dp(8),dp(8));locationRow.addView(this,LinearLayout.LayoutParams(0,-2,1f))}
        Button(this).apply{
            text="Open";isAllCaps=false;contentDescription="Open download folder"
            locationRow.addView(this,LinearLayout.LayoutParams(-2,dp(48)))
            setOnClickListener{runCatching{openDownloadFolder(AppSettings.folder(this@MainActivity))}.onFailure{alert(it.message ?: "Could not open this folder")}}
        }
        settingsConfiguration.add(button(storage,"Change folder…"){folderPicker.launch(AppSettings.folder(this).takeIf{it.isNotEmpty()}?.let(Uri::parse))})
        settingsConfiguration.add(button(storage,"Use Downloads / DataBeam"){AppSettings.setFolder(this,null);updateSaveLocation()})
        label(storage,"Folder changes apply to new transfers.").textSize=12f
        settingsConfiguration.add(Switch(this).apply{
            text="Always unwrap Croc receives"
            isChecked=AppSettings.unwrapCroc(this@MainActivity)
            setTextColor(ink);storage.addView(this)
            setOnCheckedChangeListener{_,checked->AppSettings.setUnwrapCroc(this@MainActivity,checked)}
        })
        label(storage,"Save Croc files and folders directly in the download folder.").textSize=12f
        label(storage,"Auto-clear history and partials").textSize=13f
        val cleanupOptions=listOf(0,7,30)
        settingsConfiguration.add(Spinner(this).apply{
            adapter=ArrayAdapter(this@MainActivity,android.R.layout.simple_spinner_dropdown_item,listOf("Off","After 1 week","After 1 month"))
            setSelection(cleanupOptions.indexOf(AppSettings.cleanupDays(this@MainActivity)))
            onItemSelectedListener=object:android.widget.AdapterView.OnItemSelectedListener{
                override fun onNothingSelected(parent:android.widget.AdapterView<*>?){}
                override fun onItemSelected(parent:android.widget.AdapterView<*>?,view:View?,position:Int,id:Long){
                    if(!configurationLocked())AppSettings.setCleanupDays(this@MainActivity,cleanupOptions[position])
                }
            }
            storage.addView(this)
        })
        label(storage,"Saved files are kept.").textSize=12f
        settingsConfiguration.add(button(storage,"Review and clean transfer storage"){showStorageCleanup(true)})

        val background=section("background","Background transfers")
        settingsBackground=label(background,"").apply{setTextColor(accent)}
        batteryButton=button(background,"Battery settings"){
            // Application details is supported across vendors; Samsung exposes Battery here.
            startActivity(Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,Uri.parse("package:$packageName")))
        }
        if(!unrestricted())label(background,"Open Battery and choose Unrestricted.").textSize=12f
        notificationsButton=button(background,"Notifications"){
            val manager=getSystemService(NotificationManager::class.java)
            val channel=manager.getNotificationChannel(TransferService.CHANNEL)
            val intent=if(manager.areNotificationsEnabled() && channel?.importance==NotificationManager.IMPORTANCE_NONE)
                Intent(android.provider.Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS).putExtra(android.provider.Settings.EXTRA_CHANNEL_ID,TransferService.CHANNEL)
            else Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            startActivity(intent.putExtra(android.provider.Settings.EXTRA_APP_PACKAGE,packageName))
        }
        updateBackgroundStatus()

        val relays=section("relays","Relays")
        val custom=EditText(this).apply{hint="Custom Croc relay (optional)";setSingleLine();setText(AppSettings.relay(this@MainActivity));relays.addView(this)};settingsConfiguration.add(custom)
        label(relays,"Leave empty for automatic public relay selection and recovery.").textSize=12f
        val results=label(relays,"").apply{visibility=View.GONE;textSize=13f;setTextIsSelectable(true)}
        val check=button(relays,"Check Croc public relays"){}
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
                    results.text=relayResultColors(results.text)
                }
            }.start()
        }
        val sendmeResults=label(relays,"").apply{visibility=View.GONE;textSize=13f;setTextIsSelectable(true)}
        val checkSendme=button(relays,"Check Sendme public relay"){}
        checkSendme.setOnClickListener{
            checkSendme.isEnabled=false;sendmeResults.visibility=View.VISIBLE;sendmeResults.text="Connecting to a Sendme public relay…"
            Thread{
                val outcome=runCatching{JSONObject(NativeBridge.checkSendmeRelay())}
                runOnUiThread{
                    if(isDestroyed)return@runOnUiThread
                    checkSendme.isEnabled=true
                    sendmeResults.text=outcome.fold({result->
                        if(result.optBoolean("reachable"))"Reachable · ${result.getJSONArray("relays").let{a->(0 until a.length()).joinToString(", "){a.getString(it)}}}\nIroh relay connection established. Transfers may use Direct or Relay."
                        else "Unreachable · "+result.optString("message","Sendme relay could not be reached.")
                    },{"Check failed: ${it.message}"})
                    sendmeResults.setTextColor(if(outcome.getOrNull()?.optBoolean("reachable")==true)Color.rgb(115,220,130) else Color.rgb(255,205,110))
                }
            }.start()
        }
        settingsDialog=AlertDialog.Builder(this).setTitle("Settings").setView(scroll)
            .setPositiveButton("Done"){_,_->if(!configurationLocked())AppSettings.setRelay(this,custom.text.toString())}.create().also{dialog->
                dialog.setOnDismissListener{if(!configurationLocked())AppSettings.setRelay(this,custom.text.toString());settingsFolder=null;settingsBackground=null;batteryButton=null;notificationsButton=null;sendmeEngineToggle=null;crocEngineToggle=null;settingsDialog=null;settingsConfiguration.clear()};dialog.show();lockConfiguration(configurationLocked())
                if(hasExpandedLowerSection)scrollToBottom()
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
    private fun selectedMode():String {
        val engine=mode.selectedItem?.toString().orEmpty()
        return if(engine=="Sendme" && easyMode.isChecked && AppSettings.engineEnabled(this,"EasySendMe"))"EasySendMe" else engine
    }
    private fun selectMode(value:String){
        require(value in AppSettings.modes(this)){"Enable ${if(value=="EasySendMe")"both engines" else value} in Settings to use this feature"}
        val engines=EngineOptions.engines(AppSettings.disabledEngine(this))
        mode.setSelection(engines.indexOf(if(value=="EasySendMe")"Sendme" else value))
        easyMode.isChecked=value=="EasySendMe"
    }
    private fun refreshEngines(){
        val previousMode=selectedMode()
        val engines=EngineOptions.engines(AppSettings.disabledEngine(this))
        mode.adapter=ArrayAdapter(this,android.R.layout.simple_spinner_dropdown_item,engines)
        selectMode(previousMode.takeIf{it in AppSettings.modes(this)} ?: engines.first())
        if(previousMode !in AppSettings.modes(this))code.setText("")
        quickMenu?.dismiss();updatePanels();previous="";render()
    }
    private fun acceptScannedCode(raw:String){
        val value=raw.trim()
        if(NativeBridge.hash(value).isNotEmpty()){
            selectMode("Sendme");selectTab("receive");code.setText(value);return
        }
        val parsed=QrSharing.parse(raw)
        selectMode(when {
            parsed.easySendMe->"EasySendMe"
            NativeBridge.hash(parsed.value).isNotEmpty()->"Sendme"
            else->"Croc"
        })
        selectTab("receive");code.setText(QrSharing.sharedCode(selectedMode(),parsed.value))
    }
    private fun updateAccent(mode:String){
        val color=when(mode){
            "Croc"->Color.rgb(87,214,36)
            "Sendme"->Color.rgb(11,131,230)
            "EasySendMe"->Color.rgb(214,66,36)
            else->brandAccent
        }
        if(accent==color)return
        accent=color
        val tint=android.content.res.ColorStateList.valueOf(color)
        accentButtons.forEach{it.background=rounded(color)}
        listOf(browserGuidance,saveLocation,route,ticket,settingsFolder,settingsBackground).forEach{it?.setTextColor(color)}
        browserGuidance.setLinkTextColor(color)
        easyMode.buttonTintList=tint
        broadcastChoice.buttonTintList=tint
        bar.progressTintList=tint
        bar.indeterminateTintList=tint
        val highlight=Color.argb(110,Color.red(color),Color.green(color),Color.blue(color))
        listOf(code,sendCode,message).forEach{
            it.backgroundTintList=tint
            it.highlightColor=highlight
        }
        refreshSelection()
    }
    private fun updatePanels(){
        val selected=selectedMode()
        updateAccent(selected)
        historyButton.visibility=if(tab=="receive" && selected=="Croc")View.VISIBLE else View.GONE
        if(selected!="Croc")browserSend=false
        browserGuidance.visibility=if(tab=="send" && selected=="Croc" && browserSend && sendKind=="files")View.VISIBLE else View.GONE
        if(codeMode!=selected){
            codeMode=selected;sendCode.setText(AppSettings.code(this,selected))
        }
        sendTab.background=rounded(if(tab=="send")accent else Color.rgb(28,28,28));sendTab.setTextColor(if(tab=="send")Color.BLACK else ink)
        receiveTab.background=rounded(if(tab=="receive")accent else Color.rgb(28,28,28));receiveTab.setTextColor(if(tab=="receive")Color.BLACK else ink)
        engineDescription.text=when(selected){
            "Croc"->"Fast public relay · Resumable · Text and browser sharing"
            "Sendme"->"Less reliance on relays · Resumable · Optional Broadcast"
            else->"Short code · Less reliance on relays · Resumable"
        }
        receiveInputTitle.text=if(selected=="Sendme")"1 · Enter the sender’s ticket" else "1 · Enter the sender’s code"
        code.hint=if(selected=="Sendme")"Paste a Sendme ticket" else "Paste a $selected code"
        easyMode.visibility=if(mode.selectedItem=="Sendme" && AppSettings.engineEnabled(this,"EasySendMe"))View.VISIBLE else View.GONE
        broadcastChoice.visibility=if(selected=="Sendme")View.VISIBLE else View.GONE
        sendPanel.visibility=if(tab=="send")View.VISIBLE else View.GONE
        receivePanel.visibility=if(tab=="receive")View.VISIBLE else View.GONE
        sendCode.visibility=if(selectedMode()=="Sendme")View.GONE else View.VISIBLE
        sendCodeActions.visibility=sendCode.visibility
        sendRecent.visibility=if(AppSettings.recent(this,selected,"send").isNotEmpty())View.VISIBLE else View.GONE
        receiveRecent.visibility=if(selected!="Sendme" && AppSettings.recent(this,selected,"receive").isNotEmpty())View.VISIBLE else View.GONE
        val croc=selectedMode()=="Croc"
        contentSwitch.visibility=if(croc)View.VISIBLE else View.GONE
        val textMode=croc && sendKind=="text"
        fileSelectionPanel.visibility=if(textMode)View.GONE else View.VISIBLE
        message.visibility=if(textMode)View.VISIBLE else View.GONE
        clearText.visibility=message.visibility
        filesChoice.background=rounded(if(!textMode)accent else Color.rgb(28,28,28));filesChoice.setTextColor(if(!textMode)Color.BLACK else ink)
        textChoice.background=rounded(if(textMode)accent else Color.rgb(28,28,28));textChoice.setTextColor(if(textMode)Color.BLACK else ink)
        updatePrimaryLabel()
    }
    private fun showQuickActions(){
        if(configurationLocked())return
        quickMenu?.dismiss()
        quickMenu=PopupMenu(this,quickActions).apply{
            val allowed=EngineOptions.quickActions(AppSettings.disabledEngine(this@MainActivity))
            if(1 in allowed)menu.add(0,1,0,"Send text")
            if(2 in allowed)menu.add(0,2,1,"Broadcast")
            if(3 in allowed)menu.add(0,3,2,"Send to browser")
            if(4 in allowed)menu.add(0,4,3,"EasySendMe")
            if(EngineOptions.CROC_HISTORY_ACTION in allowed)menu.add(0,EngineOptions.CROC_HISTORY_ACTION,4,"Croc receive history")
            setOnMenuItemClickListener{item->runCatching{quickAction(item.itemId)}.onFailure{alert(it.message ?: "Cannot change the active transfer")};true}
            setOnDismissListener{quickMenu=null}
            show()
        }
    }
    private fun quickAction(action:Int){
        require(!configurationLocked()){"Finish or stop the active transfer first"}
        when(action){
            1->{selectMode("Croc");sendKind="text";browserSend=false}
            2->{selectMode("Sendme");sendKind="files";browserSend=false;broadcastChoice.isChecked=true}
            3->{selectMode("Croc");sendKind="files";browserSend=true}
            4->{selectMode("EasySendMe");sendKind="files";browserSend=false}
            EngineOptions.CROC_HISTORY_ACTION->{showCrocHistory();return}
            else->error("Unknown quick action")
        }
        selectTab("send")
    }
    private fun updateTroubleshootBadge(){
        val unread=TroubleshootNews.hasUnread(this)
        troubleshootBadge.visibility=if(unread)View.VISIBLE else View.GONE
        troubleshootButton.contentDescription=if(unread)"Troubleshoot: new guide or release news" else "Troubleshoot"
    }
    private fun scan(){
        require(TransferService.state(this).optString("status") !in listOf("running","stopping")){"Pause the current transfer before scanning"}
        scanner.launch(Intent(this,QrScannerActivity::class.java))
    }
    private fun normalizeInput(){
        val value=code.text.toString().trim()
        if(NativeBridge.hash(value).isNotEmpty()){selectMode("Sendme");code.setText(value);return}
        QrSharing.easySendMeCode(value)?.let{raw->selectMode("EasySendMe");code.setText(QrSharing.sharedCode("EasySendMe",raw));return}
        if(value.contains("://")){val parsed=QrSharing.parse(value);if(parsed.crocLink)selectMode("Croc");code.setText(parsed.value)}
    }
    private fun receivedCodeText(state:JSONObject):String {
        val mode=state.optString("mode")
        return QrSharing.sharedCode(mode,state.optString(if(mode=="Sendme")"ticket" else "code"))
    }
    private fun shareValue():String {
        val saved=TransferService.state(this)
        require(saved.optString("action")=="send" && saved.optString("status")=="running" && !saved.optBoolean("troubleshooting")){"Start sharing first"}
        val mode=saved.optString("mode")
        val value=if(mode=="Sendme")saved.optString("ticket") else saved.optString("code")
        require(value.isNotEmpty()){"Preparing sharing code…"}
        return QrSharing.sharedCode(mode,value)
    }
    private fun showQr(){
        val value=shareValue();val matrix=QrSharing.matrix(value)
        val pixels=IntArray(matrix.width*matrix.height){i->if(matrix[i%matrix.width,i/matrix.width])Color.BLACK else Color.WHITE}
        val image=ImageView(this).apply{setImageBitmap(Bitmap.createBitmap(pixels,matrix.width,matrix.height,Bitmap.Config.ARGB_8888));adjustViewBounds=true;setPadding(dp(12),dp(12),dp(12),dp(12));setBackgroundColor(Color.WHITE)}
        qrDialog?.dismiss();qrDialog=AlertDialog.Builder(this).setTitle("${TransferService.state(this).optString("mode")} sharing QR").setView(image).setPositiveButton("Close",null).show()
    }
    override fun onSaveInstanceState(out:Bundle){out.putString("clearedPayloadSession",clearedPayloadSession);out.putString("retainedCodeSession",retainedCodeSession);out.putBoolean("browserSend",browserSend);out.putBoolean("broadcast",broadcastChoice.isChecked);out.putString("sendKind",sendKind);out.putString("tab",tab);out.putString("modeName",selectedMode());out.putString("message",message.text.toString());out.putString("code",code.text.toString());out.putStringArrayList("sources",ArrayList(sources.map{it.toString()}));out.putString("crocHistoryId",crocHistoryId);out.putString("crocHistoryCode",crocHistoryInput?.text?.toString());super.onSaveInstanceState(out)}
    private fun launch(action:String) {
        val state=TransferService.state(this)
        require(!configurationLocked()){"Stop or pause the active transfer first"}
        var entered=""
        if(action=="receive") {
            normalizeInput()
            if(Build.VERSION.SDK_INT<=28 && AppSettings.folder(this).isEmpty() && checkSelfPermission(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)!=android.content.pm.PackageManager.PERMISSION_GRANTED){requestPermissions(arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE),22);return}
            entered=code.text.toString().trim()
            if(selectedMode()=="EasySendMe")entered=QrSharing.easySendMeCode(entered) ?: entered
            if(state.optString("action")=="receive" && state.optString("mode")==selectedMode() && state.optString("status") in listOf("paused","interrupted")) {
                val old=if(selectedMode()=="Sendme")state.optString("ticket") else state.optString("code")
                val same=entered==old || (selectedMode()=="Sendme" && NativeBridge.hash(entered).isNotEmpty() && NativeBridge.hash(entered)==NativeBridge.hash(old))
                if(same){if(selectedMode()=="Sendme")state.put("ticket",entered);start(state);return}
            }
        }
        val sendingText=selectedMode()=="Croc" && sendKind=="text"
        val outgoingCode=sendCode.text.toString().trim()
        if(action=="send" && selectedMode()=="Croc")require(!outgoingCode.endsWith("esm",true)){"Choose a Croc code that ends with something other than esm."}
        val c=JSONObject().put("id",UUID.randomUUID().toString()).put("mode",selectedMode()).put("action",action).put("code",if(selectedMode()=="Sendme") "" else if(action=="send")outgoingCode else entered).put("ticket",if(selectedMode()=="Sendme")entered else "").put("text",if(sendingText)message.text.toString() else "").put("uris",JSONArray(if(sendingText)emptyList<String>() else sources.map{it.toString()})).put("payloadNames",JSONArray(if(sendingText)listOf("Text") else sources.map{sourceName(it)})).put("relay",AppSettings.relay(this)).put("broadcast",action=="send" && selectedMode()=="Sendme" && broadcastChoice.isChecked)
        if(action=="receive") require(code.text.isNotBlank()){"Enter a code or ticket"}
        if(action=="receive")c.remove("payloadNames")
        if(action=="send" && !sendingText)c.put("sourceNames",c.getJSONArray("payloadNames"))
        if(action=="send")c.put("skipDuplicateFiles",AppSettings.skipDuplicateFiles(this))
        if(action=="send") require(if(sendingText)message.text.isNotBlank() else sources.isNotEmpty()){if(sendingText)"Enter text to send" else "Select files or a folder"}
        if(selectedMode()!="Croc") c.put("text","")
        if(action=="send" && selectedMode()!="Sendme")AppSettings.setCode(this,selectedMode(),sendCode.text.toString().trim())
        start(c)
    }
    private fun canReadSendSource(uri:Uri):Boolean = runCatching {
        when(uri.scheme){
            "file"->File(requireNotNull(uri.path)).let{it.canRead() && (it.isFile || it.isDirectory)}
            "content"->if(android.provider.DocumentsContract.isTreeUri(uri)){
                val doc=android.provider.DocumentsContract.buildDocumentUriUsingTree(uri,android.provider.DocumentsContract.getTreeDocumentId(uri))
                contentResolver.query(doc,arrayOf(android.provider.DocumentsContract.Document.COLUMN_DOCUMENT_ID),null,null,null)
                    ?.use{it.moveToFirst()}==true
            } else {
                contentResolver.openAssetFileDescriptor(uri,"r")?.use{true}==true
            }
            else->false
        }
    }.getOrDefault(false)

    private fun restoreSendPayload(){
        require(!configurationLocked()){"Stop the active transfer first"}
        val saved=TransferService.state(this)
        require(TransferLabels.canResend(saved)){"There is no interrupted send to restore."}
        val selected=saved.optString("mode")
        require(selected in AppSettings.modes(this)){"Enable this transfer’s engine in Settings to restore it."}
        val text=if(selected=="Croc")saved.optString("text") else ""
        val previousSources=saved.optJSONArray("uris") ?: JSONArray()
        val previousNames=saved.optJSONArray("sourceNames") ?: saved.optJSONArray("payloadNames")
        val available=mutableListOf<Uri>()
        val unavailable=mutableListOf<String>()
        for(index in 0 until previousSources.length()){
            val uri=Uri.parse(previousSources.getString(index))
            if(canReadSendSource(uri))available.add(uri)
            else unavailable.add(previousNames?.optString(index).orEmpty().ifBlank{"Item ${index+1}"})
        }
        if(text.isEmpty() && available.isEmpty()){
            alert("Select these files or folders again to resend them:\n${unavailable.joinToString("\n")}")
            return
        }
        selectMode(selected)
        browserSend=selected=="Croc" && saved.optBoolean("browserSend")
        sendKind=if(text.isNotEmpty())"text" else "files"
        sources.clear();sources.addAll(available);restartedNames.clear()
        message.setText(text)
        broadcastChoice.isChecked=selected=="Sendme" && saved.optBoolean("broadcast")
        selectTab("send")
        sendCode.setText(saved.optString("code"))
        refreshSelection()
        if(unavailable.isEmpty()){
            launch("send")
        } else {
            screenScroll.post{screenScroll.smoothScrollTo(0,0)}
            alert("Available items were restored. Select these items again before sending:\n${unavailable.joinToString("\n")}")
        }
    }
    private fun showResume(){
        val saved=TransferService.state(this)
        if(saved.optString("mode")=="Croc"){
            if(saved.optBoolean("connectionRetained"))resume(false) else showCrocResume(saved)
            return
        }
        val ticketMode=saved.optString("mode")=="Sendme"
        resumeDialog=AlertDialog.Builder(this).setTitle("Resume ${saved.optString("mode")}")
            .setMessage("Pausing a normal desktop send ends its sharing session. Ask the sender to share the exact same files again, then choose ${if(ticketMode)"New ticket" else "New code"}. Matching saved data continues automatically.\n\nIf the sender is still sharing the original session, such as Sendme Broadcast, choose Sender still sharing to use its current ticket.")
            .setPositiveButton(if(ticketMode)"New ticket" else "New code"){_,_->resume(true)}
            .setNeutralButton("Sender still sharing"){_,_->resume(false)}
            .setNegativeButton("Cancel",null).create().also{dialog->dialog.setOnDismissListener{resumeDialog=null};dialog.show()}
    }
    private fun showCrocHistory(){
        val entries=TransferCleanup.crocReceiveHistory(this)
        if(entries.isEmpty()){alert("Croc file receives will appear here.");return}
        AlertDialog.Builder(this).setTitle("Croc receive history")
            .setItems(entries.map { entry ->
                val names=entry.optJSONArray("payloadNames")
                val title=entry.optString("crocWrapper").ifBlank{names?.optString(0).orEmpty().ifBlank{"Croc receive"}}
                val state=TransferLabels.title(entry)
                val total=entry.optLong("total")
                val progress=if(total>0)" · ${ProgressEvents.size(entry.optLong("done"))} of ${ProgressEvents.size(total)}" else ""
                val details=if(entry.optString("status")=="complete")crocHistoryDetails(entry) else emptyList()
                listOf(title,"$state$progress",details.joinToString(" · ")).filter{it.isNotBlank()}.joinToString("\n")
            }.toTypedArray()){_,index->showCrocHistoryEntry(entries[index])}
            .setNegativeButton("Close",null).show()
    }
    private fun crocHistoryDetails(entry:JSONObject):List<String> = buildList {
        entry.optLong("completedAt").takeIf{it>0L}?.let{
            add(java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.MEDIUM,java.text.DateFormat.SHORT).format(java.util.Date(it)))
        }
        TransferTiming.elapsedText(entry).takeIf(String::isNotBlank)?.let(::add)
        entry.optString("route").takeIf{it in listOf("direct","relay")}?.let{add(TransferRoute.label(listOf(it)))}
    }

    private fun showCrocHistoryEntry(entry:JSONObject){
        val complete=entry.optString("status")=="complete"
        val active=entry.optString("status") in listOf("running","stopping") || entry.optBoolean("connectionRetained")
        val wrapper=entry.optString("crocWrapper")
        val availableFolder=wrapper.takeUnless{entry.optBoolean("crocMoved")}.orEmpty()
        val summary=listOf(TransferLabels.title(entry),TransferLabels.message(entry))+crocHistoryDetails(entry)
        val dialog=AlertDialog.Builder(this).setTitle(wrapper.ifBlank{"Croc receive"})
            .setMessage(summary.filter(String::isNotBlank).distinct().joinToString("\n"))
            .setNegativeButton("Close",null)
        if(!active && (complete || AppSettings.engineEnabled(this,"Croc")))dialog.setPositiveButton(if(complete)"Open folder" else "Resume"){_,_->
            runCatching{if(complete)openDownloadFolder(entry.optString("saveFolder"),availableFolder) else showCrocResume(entry)}
                .onFailure{alert(it.message ?: "Could not open transfer")}
        }
        if(!active && !TransferService.isBusy())dialog.setNeutralButton(if(complete)"Actions" else "Discard partial files"){_,_->
            if(complete){
                val move=availableFolder.isNotEmpty() && entry.optString("saveFolder").isEmpty() && android.os.Build.VERSION.SDK_INT>=29
                val options=if(move)arrayOf("Move files out","Remove from history") else arrayOf("Remove from history")
                AlertDialog.Builder(this).setTitle(wrapper.ifBlank{"Croc receive"}).setItems(options){_,index->
                    if(move && index==0){
                        Thread{
                            val result=runCatching{TransferCleanup.moveCrocReceive(this,entry)}
                            runOnUiThread{if(!isDestroyed)result.onSuccess{alert("Files moved to Downloads / DataBeam")}.onFailure{alert(it.message ?: "Could not move files")}}
                        }.start()
                    }else confirmRemoveCrocHistory(entry,true)
                }.setNegativeButton("Close",null).show()
            }else confirmRemoveCrocHistory(entry,false)
        }
        dialog.show()
    }

    private fun confirmRemoveCrocHistory(entry:JSONObject,complete:Boolean){
            AlertDialog.Builder(this).setMessage(if(complete)"Remove this history entry? Saved files will stay." else "Delete the partial data for this transfer? Files already saved will stay.")
                .setNegativeButton("Cancel",null).setPositiveButton("Remove"){_,_->
                    Thread{
                        val outcome=runCatching{TransferCleanup.discardCrocReceive(this,entry.getString("id"))}
                        runOnUiThread{if(!isDestroyed)outcome.onFailure{alert(it.message ?: "Could not remove transfer")};render()}
                    }.start()
                }.show()
    }
    private fun crocResumeCode(raw:String):String {
        val parsed=QrSharing.parse(raw)
        require(!parsed.easySendMe && NativeBridge.hash(parsed.value).isEmpty()){"Enter a Croc code for this partial receive"}
        return parsed.value
    }

    private fun showCrocResume(entry:JSONObject,draft:String?=null){
        require(!TransferService.isBusy() && !configurationLocked()){"Finish or end the active transfer first"}
        if(entry.optBoolean("crocReadyToSave")){start(entry);return}
        val receipt=File(filesDir,"croc-resume/${entry.getString("id")}.json")
        val canChange=runCatching{JSONObject(receipt.readText()).optBoolean("strong")}.getOrDefault(false)
        val panel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(20),0,dp(20),0)}
        val input=EditText(this).apply{setSingleLine();setText(draft ?: entry.optString("code"));isEnabled=canChange;panel.addView(this)}
        if(canChange)Button(this).apply{
            text="Scan QR";isAllCaps=false
            panel.addView(this)
            setOnClickListener{crocHistoryScanner.launch(Intent(this@MainActivity,QrScannerActivity::class.java))}
        }
        crocHistoryId=entry.getString("id");crocHistoryInput=input
        crocHistoryDialog=AlertDialog.Builder(this).setTitle("Resume Croc receive")
            .setMessage(if(canChange)"Enter the sender’s current code. You’ll review any changes to the file list." else "Ask the sender to share the original files with the original code.")
            .setView(panel).setNegativeButton("Cancel",null).setPositiveButton("Resume"){_,_->
                runCatching{
                    val entered=crocResumeCode(input.text.toString())
                    start(JSONObject(entry.toString()).put("code",entered).put("reviewCrocChanges",canChange))
                }.onFailure{alert(it.message ?: "Could not resume")}
            }.create().also{dialog->
                dialog.setOnDismissListener{crocHistoryDialog=null;crocHistoryId=null;crocHistoryInput=null}
                dialog.show()
            }
    }

    private fun renderCrocReview(state:JSONObject){
        val token=state.optString("crocReview")
        if(token.isEmpty() || state.optString("status")!="running" || !TransferService.isBusy()){
            crocReviewDialog?.dismiss();crocReviewDialog=null;shownCrocReview=""
            return
        }
        if(token==shownCrocReview || token==answeredCrocReview)return
        val transferId=state.optString("id")
        val review=CrocBatchReview.read(this,transferId,token) ?: return
        val message=buildString{
            append("Matching files keep their progress. Removed files are discarded from this unfinished receive.")
            for((key,label) in listOf("replaced" to "Replace","added" to "Add","removed" to "Remove")){
                val files=review.getJSONArray(key)
                if(files.length()>0){
                    append("\n\n").append(label)
                    for(index in 0 until files.length())append('\n').append(files.getString(index))
                }
            }
        }
        fun answer(apply:Boolean){
            answeredCrocReview=token
            runCatching{CrocBatchReview.respond(this,transferId,token,apply)}
                .onFailure{alert(it.message ?: "Could not send the review decision")}
        }
        crocReviewDialog?.dismiss()
        shownCrocReview=token
        crocReviewDialog=AlertDialog.Builder(this).setTitle("Review updated Croc files").setMessage(message)
            .setPositiveButton("Apply changes and resume"){_,_->answer(true)}
            .setNegativeButton("Keep partial files"){_,_->answer(false)}
            .setOnCancelListener{answer(false)}
            .show()
    }
    private fun resume(replace:Boolean) {
        val c=TransferService.state(this)
        require(c.has("id")){"No interrupted transfer"}
        require(c.optString("status") !in listOf("running","stopping")){"Transfer is still running"}
        if(replace) {
            val ticketMode=c.optString("mode")=="Sendme"
            val entry=EditText(this).apply{hint=if(ticketMode)"Paste the sender’s new ticket" else "Sender’s current EasySendMe code";if(!ticketMode)setText(c.optString("code"))}
            reconnectDialog=AlertDialog.Builder(this).setTitle(if(ticketMode)"New Sendme ticket" else "Current EasySendMe code").setMessage("Ask the sender to share the exact same files again, then enter their ${if(ticketMode)"new ticket" else "current code (it can be unchanged)"}. Matching saved data continues automatically.").setView(entry).setNegativeButton("Cancel",null).setPositiveButton("Receive"){_,_->
                if(entry.text.isNotBlank()) {
                    val next=entry.text.toString().trim()
                    if(ticketMode){
                        if(NativeBridge.hash(next).isEmpty() || NativeBridge.hash(next)!=NativeBridge.hash(c.optString("ticket"))){alert("This ticket is invalid or belongs to different files. Start a new receive for different files.");return@setPositiveButton}
                        c.put("ticket",next)
                    }else c.put("code",QrSharing.easySendMeCode(next) ?: next).put("refresh",true)
                    if(selectedMode()==c.optString("mode"))code.setText(receivedCodeText(c))
                    start(c)
                }
            }.create().also{dialog->dialog.setOnDismissListener{reconnectDialog=null};dialog.show()}
        } else {
            val entered=code.text.toString().trim()
            if(c.optString("action")=="receive" && entered.isNotEmpty()){
                if(c.optString("mode")=="Croc" && entered!=c.optString("code")){
                    error("Use the original Croc code to resume. For a different code, tap Receive to start a new transfer.")
                }
            }
            start(c)
        }
    }
    private fun start(c:JSONObject){
        require(c.optString("mode") in AppSettings.modes(this)){"Enable this transfer’s engines in Settings to continue"}
        if(c.optString("action")=="send" && c.optString("mode")=="Croc")c.put("browserSend",browserSend)
        val current=TransferService.state(this)
        if(current.optBoolean("connectionRetained") && c.optString("mode")=="Croc" && current.optString("id")==c.optString("id") && current.optString("status")=="paused"){
            require(current.optString("code")==c.optString("code")){"Tap End transfer to close the paused connection, then tap Resume with the new code."}
            startService(Intent(this,TransferService::class.java).setAction("resume"));return
        }
        c.remove("log");c.remove("message");pendingStartId=c.optString("id");lockConfiguration(true)
        var handoff:File?=null
        try{
            val service=Intent(this,TransferService::class.java).putExtra("sessionId",c.optString("id")).putExtra("mode",c.optString("mode")).putExtra("action",c.optString("action"))
            if(c.optString("text").length>8192){
                handoff=File(File(cacheDir,"start-config").apply{mkdirs()},"${UUID.randomUUID()}.json").apply{writeText(c.toString(),Charsets.UTF_8)}
                service.putExtra("configFile",handoff.name)
            }else service.putExtra("config",c.toString())
            startForegroundService(service);status.text="Starting…"
        }catch(error:Exception){handoff?.delete();pendingStartId="";lockConfiguration(configurationLocked());throw error}
    }
    private fun render() {
        val s=TransferService.state(this);renderCrocReview(s);val value=s.toString()
        if(value==previous && showedRetainedSelection==sources.isNotEmpty())return
        previous=value
        if(s.optString("id")==pendingStartId)pendingStartId=""
        val inCrocHistory=s.optString("mode")=="Croc" && s.optString("action")=="receive" &&
            s.optString("status") !in listOf("running","stopping") && !s.optBoolean("connectionRetained") &&
            (s.optString("crocWrapper").isNotEmpty() || s.optJSONArray("payloadNames")?.length()?.let{it>0}==true)
        transferPanel.visibility=if(!inCrocHistory && s.optString("id").isNotEmpty() && s.optString("mode") in AppSettings.modes(this))View.VISIBLE else View.GONE
        if(transferPanel.visibility==View.VISIBLE && scrolledTransfer!=s.optString("id")){
            scrolledTransfer=s.optString("id");screenScroll.postDelayed({screenScroll.fullScroll(View.FOCUS_DOWN)},150)
        }
        if(s.optString("id").isNotEmpty() && s.optString("id")!=renderedSession){
            renderedSession=s.optString("id");tab=s.optString("action","send")
            if(s.optString("mode") in AppSettings.modes(this)){
                selectMode(s.optString("mode"))
                if(tab=="receive")code.setText(receivedCodeText(s))
            }
            updatePanels()
        }
        val running=s.optString("status")=="running";val stopping=s.optString("status")=="stopping"
        val paused=s.optString("status") in listOf("paused","interrupted")
        val receiving=s.optString("action")=="receive"
        if(!receiving && s.optString("status")=="complete" && clearedPayloadSession!=s.optString("id")){
            clearedPayloadSession=s.optString("id")
            sources.clear();restartedNames.clear();message.setText("")
            refreshSelection()
        }
        if(!receiving && s.optString("status") in listOf("interrupted","stopped") && retainedCodeSession!=s.optString("id")){
            retainedCodeSession=s.optString("id")
            restartedNames.clear()
            if(s.optString("mode")!="Sendme" && selectedMode()==s.optString("mode") && s.optString("code").isNotEmpty())sendCode.setText(s.optString("code"))
        }
        showedRetainedSelection=sources.isNotEmpty()
        transferDirection.text="${s.optString("mode")} · ${if(receiving)"Receive on this phone" else "Send from this phone"}"
        if(!receiving && s.optString("mode")=="Croc" && s.optString("status")=="complete" && retainedSession!=s.optString("id")){
            retainedSession=s.optString("id");if(selectedMode()=="Croc")sendCode.setText(AppSettings.code(this,"Croc"))
        }
        sendCode.isEnabled=!running && !stopping;sendCodeActions.isEnabled=!running && !stopping
        for(index in 0 until sendCodeActions.childCount)sendCodeActions.getChildAt(index).isEnabled=!running && !stopping
        sendRecent.visibility=if(AppSettings.recent(this,selectedMode(),"send").isNotEmpty())View.VISIBLE else View.GONE
        receiveRecent.visibility=if(selectedMode()!="Sendme" && AppSettings.recent(this,selectedMode(),"receive").isNotEmpty())View.VISIBLE else View.GONE
        status.text=TransferLabels.title(s)
        val completedSender=TransferLabels.senderFinished(s)
        val paths=if(completedSender || (!running && !stopping))s.optJSONObject("lastSenderRoutes") ?: s.optJSONObject("senderRoutes") else s.optJSONObject("senderRoutes")
        val pathValues=paths?.keys()?.asSequence()?.map{paths.optString(it).takeIf{v->v in listOf("direct","relay")} ?: s.optJSONObject("lastSenderRoutes")?.optString(it,"unknown") ?: "unknown"}?.toList() ?: emptyList()
        val activeRoute=if(s.optString("mode")!="Croc" && !receiving) {
            if(pathValues.isEmpty())"Waiting for receiver" else TransferRoute.label(pathValues)
        } else TransferRoute.label(listOf(s.optString("route","unknown")))
        route.text="${if(completedSender || (!running && !stopping))"Connection used" else "Connection"} · $activeRoute";route.visibility=if(running || stopping || (activeRoute !in listOf("Waiting for receiver","Connecting…","Unknown") && (s.has("route") || pathValues.isNotEmpty())))View.VISIBLE else View.GONE
        val preparing=s.optBoolean("preparing")
        val total=s.optLong(if(preparing)"prepTotal" else "total");val done=s.optLong(if(preparing)"prepDone" else "done")
        val speed=s.optDouble(if(preparing)"prepSpeed" else "speed").takeIf{it.isFinite() && it>=0} ?: 0.0
        val percent=if(total>0 || s.optInt("filesTotal")>0)ProgressEvents.percent(done,total,s.optInt("filesDone"),s.optInt("filesTotal")) else s.optInt("percent")
        val progress=if(total>0 || s.optInt("filesTotal")>0)"$percent% · ${ProgressEvents.size(done)} of ${ProgressEvents.size(total)}" else if(s.has("percent"))"$percent% · ${s.optString("progress")}" else s.optString("progress")
        val rate=if(running && speed>0)"${ProgressEvents.size(speed.toLong())}/s"+ (ProgressEvents.eta(done,total,speed)?.let{" · $it left"} ?: "") else ""
        val files=if(s.optInt("filesTotal")>0)"${s.optInt("filesDone")} / ${s.optInt("filesTotal")} files" else ""
        val names=s.optJSONArray("payloadNames")?.let{a->(0 until a.length()).joinToString("\n"){a.optString(it)}}.orEmpty()
        val skipped=s.optInt("skippedDuplicateFiles").takeIf{it>0}?.let{if(it==1)"Skipped 1 identical file" else "Skipped $it identical files"}.orEmpty()
        detail.text=if(s.optString("status")=="complete")listOf(names,skipped,TransferLabels.message(s),TransferTiming.elapsedText(s)).filter{it.isNotEmpty()}.joinToString("\n")
            else listOf(names,skipped,TransferLabels.message(s,showedRetainedSelection),progress,files,rate,TransferTiming.elapsedText(s),TransferTiming.averageText(s)).filter{it.isNotEmpty()}.joinToString("\n")
        bar.isIndeterminate=running && total==0L && !s.has("percent") && s.optInt("filesTotal")==0
        bar.progress=percent
        bar.visibility=if(running || paused || stopping || s.optString("status")=="stopped")View.VISIBLE else View.GONE
        lockConfiguration(configurationLocked())
        pause.visibility=if(running || stopping)View.VISIBLE else View.GONE;pause.isEnabled=!stopping;pause.text=if(receiving)"Pause" else "Stop sharing"
        resumeButton.visibility=if(paused && receiving)View.VISIBLE else View.GONE
        resend.visibility=if(TransferLabels.canResend(s))View.VISIBLE else View.GONE
        endPaused.visibility=if(paused && s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        newTransfer.visibility=if(!running && !stopping && !s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        clearTransferNote.visibility=newTransfer.visibility
        clearTransferNote.text=if(paused && receiving)"Clears this card and its Resume entry. Saved files remain." else "Clears this card. Saved files remain."
        sharing.visibility=if(running && !receiving)View.VISIBLE else View.GONE
        if(s.optBoolean("troubleshooting")){qrDialog?.dismiss()}
        relayNotice.text=s.optString("relayNotice");relayNotice.visibility=if(relayNotice.text.isEmpty())View.GONE else View.VISIBLE
        sendmeFallback.visibility=if(s.optBoolean("suggestSendme") && AppSettings.engineEnabled(this,"Sendme"))View.VISIBLE else View.GONE;sendmeFallback.isEnabled=!configurationLocked()
        val replacement=s.optString("codeRevision");if(replacement.isNotEmpty() && replacement!=lastReplacement){qrDialog?.dismiss();sendCode.setText(replacement);lastReplacement=replacement}
        ticket.text=if(running && !receiving && !s.optBoolean("troubleshooting"))
            QrSharing.sharedCode(s.optString("mode"),if(s.optString("mode")=="Sendme")s.optString("ticket") else s.optString("code")) else ""
        openReceived.visibility=if(receiving && s.optString("status")=="complete" && ((s.optJSONObject("published")?.length() ?: 0)>0 || (s.optJSONObject("publishedDirectories")?.length() ?: 0)>0))View.VISIBLE else View.GONE
        receivedText.text=if(receiving)s.optString("text") else "";receivedText.visibility=if(receivedText.text.isEmpty())View.GONE else View.VISIBLE;receivedTextTitle.visibility=receivedText.visibility
        copyReceivedText.visibility=receivedText.visibility
        detectedTextTicket=if(receiving)s.optString("text").splitToSequence(Regex("\\s+")).map{it.trim('"','\'','`')}.firstOrNull{it.startsWith("blob") && NativeBridge.hash(it).isNotEmpty()}.orEmpty() else ""
        receiveTextTicket.visibility=if(detectedTextTicket.isNotEmpty() && AppSettings.engineEnabled(this,"Sendme") && !running && !stopping)View.VISIBLE else View.GONE
    }
    private fun relayResultColors(value:CharSequence):CharSequence {
        val colored=android.text.SpannableString(value)
        Regex("(?m)^.*(?:Reachable|Unreachable|Check failed|No public relay).*$").findAll(value).forEach{match->
            val good=match.value.contains("Reachable") && !match.value.contains("Unreachable")
            colored.setSpan(android.text.style.ForegroundColorSpan(if(good)Color.rgb(115,220,130) else Color.rgb(255,205,110)),match.range.first,match.range.last+1,android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
        return colored
    }
    private fun openReceivedFolder(){
        openDownloadFolder(TransferService.state(this).optString("saveFolder"))
    }
    private fun openDownloadFolder(folder:String,wrapper:String=""){
        val uri=if(folder.isNotEmpty())Storage.folderDocument(this,folder,wrapper)
            else android.provider.DocumentsContract.buildDocumentUri("com.android.externalstorage.documents","primary:Download/DataBeam"+if(wrapper.isEmpty())"" else "/$wrapper")
        val reuse=Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
        val grant=if(folder.isNotEmpty())Intent.FLAG_GRANT_READ_URI_PERMISSION else 0
        try{startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri,android.provider.DocumentsContract.Document.MIME_TYPE_DIR).addFlags(reuse or grant))}
        catch(_:ActivityNotFoundException){startActivity(Intent(android.app.DownloadManager.ACTION_VIEW_DOWNLOADS).addFlags(reuse))}
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
        if(incoming.isEmpty() && !AppSettings.engineEnabled(this,"Croc")){alert("Enable Croc in Settings to send text");return}
        sources.clear();sources.addAll(incoming.distinct())
        if(i.flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION!=0){
            sources.forEach{runCatching{contentResolver.takePersistableUriPermission(it,Intent.FLAG_GRANT_READ_URI_PERMISSION)}}
        }
        restartedNames.clear()
        if(sources.isEmpty()){
            sendKind="text";selectMode("Croc");message.setText(text)
        } else {
            // Some apps attach a caption too. Sending files must not silently switch to text.
            sendKind="files";message.setText("")
        }
        refreshSelection();selectTab("send")
    }
    override fun onNewIntent(intent:Intent){
        super.onNewIntent(intent);setIntent(intent)
        renderedSession=TransferService.state(this).optString("id")
        render()
        acceptShare(intent)
    }
    override fun onResume(){super.onResume();Thread{runCatching{TransferCleanup.clearInactiveSenders(applicationContext);TransferCleanup.clearAgedTransfers(applicationContext)}}.start();updateBackgroundStatus();handler.post(ticker);updateTroubleshootBadge();TroubleshootNews.check(applicationContext,force=true){runOnUiThread{if(!isDestroyed)updateTroubleshootBadge()}};if(!cleanupScanned){cleanupScanned=true;showStorageCleanup(false)}}
    override fun onPause(){handler.removeCallbacks(ticker);super.onPause()}
    override fun onDestroy(){quickMenu?.dismiss();quickMenu=null;resumeDialog?.dismiss();resumeDialog=null;reconnectDialog?.dismiss();reconnectDialog=null;crocHistoryDialog?.dismiss();crocReviewDialog?.dismiss();super.onDestroy()}
}
