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
    private var settingsFolder:TextView?=null
    private var settingsBackground:TextView?=null
    private var batteryButton:Button?=null
    private var notificationsButton:Button?=null
    private lateinit var backgroundNotice:TextView
    private val folderPicker=registerForActivityResult(ActivityResultContracts.OpenDocumentTree()){uri->
        if(uri!=null)runCatching {
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
    private val sources=mutableListOf<Uri>()
    private val handler=Handler(Looper.getMainLooper())
    private var previous=""
    private val ticker=object:Runnable {override fun run(){render();handler.postDelayed(this,500)}}
    private val ink=Color.rgb(232,241,244)
    private val green=Color.rgb(105,231,199)
    private lateinit var sendPanel:LinearLayout
    private lateinit var receivePanel:LinearLayout
    private lateinit var primary:Button
    private lateinit var pause:Button
    private lateinit var resumeButton:Button
    private lateinit var sharing:LinearLayout
    private lateinit var bar:ProgressBar
    private lateinit var detail:TextView
    private lateinit var transferPanel:LinearLayout
    private lateinit var route:TextView
    private lateinit var transferDirection:TextView
    private lateinit var newTransfer:Button
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
        val current=TransferService.state(this)
        renderedSession=current.optString("id")
        tab=savedInstanceState?.getString("tab") ?: current.optString("action","send")
        val scroll=ScrollView(this).apply{setBackgroundColor(Color.rgb(13,19,27));isFillViewport=true}
        val screen=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setBackgroundColor(Color.rgb(13,19,27))}
        val layout=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(20),dp(12),dp(20),dp(24))}
        scroll.addView(layout);screen.addView(scroll,LinearLayout.LayoutParams(-1,0,1f));setContentView(screen)
        screen.setOnApplyWindowInsetsListener{v,insets->
            if(Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(bars.left,bars.top,bars.right,bars.bottom)}
            else v.setPadding(insets.systemWindowInsetLeft,insets.systemWindowInsetTop,insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            insets
        }
        fun label(parent:LinearLayout,text:String,size:Float=14f)=TextView(this).apply{this.text=text;textSize=size;setTextColor(ink);setPadding(0,dp(6),0,dp(6));parent.addView(this)}
        fun row(parent:LinearLayout)=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;parent.addView(this)}
        fun card(parent:LinearLayout)=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;background=rounded(Color.rgb(23,33,45));setPadding(dp(16),dp(10),dp(16),dp(12));parent.addView(this,LinearLayout.LayoutParams(-1,-2).apply{topMargin=dp(12)})}
        fun button(parent:LinearLayout,text:String,accent:Boolean=false,action:()->Unit)=Button(this).apply{
            this.text=text;isAllCaps=false;textSize=14f;minHeight=0;minimumHeight=0
            setTextColor(if(accent)Color.rgb(10,25,25) else ink);background=rounded(if(accent)green else Color.rgb(40,53,69))
            val params=if(parent.orientation==LinearLayout.HORIZONTAL)LinearLayout.LayoutParams(0,dp(46),1f).apply{marginEnd=dp(6)} else LinearLayout.LayoutParams(-1,dp(48)).apply{topMargin=dp(8)}
            parent.addView(this,params);setOnClickListener{runCatching(action).onFailure{alert(it.message ?: "Operation failed")}}
        }
        fun input(parent:LinearLayout,hint:String)=EditText(this).apply{this.hint=hint;setTextColor(ink);setHintTextColor(Color.rgb(143,161,180));textSize=15f;setSingleLine();parent.addView(this,LinearLayout.LayoutParams(-1,dp(52)))}
        val header=row(layout)
        label(header,"⚡ DataBeam",28f).apply{setTextColor(green);layoutParams=LinearLayout.LayoutParams(0,-2,1f)}
        ImageButton(this).apply{
            setImageResource(R.drawable.ic_settings);imageTintList=android.content.res.ColorStateList.valueOf(ink)
            background=rounded(Color.rgb(40,53,69));contentDescription="Settings"
            header.addView(this,LinearLayout.LayoutParams(dp(48),dp(48)));setOnClickListener{showSettings()}
        }
        label(layout,"Secure sharing between your devices").setTextColor(Color.LTGRAY)
        backgroundNotice=label(layout,"",12f).apply{
            setTextColor(Color.rgb(255,205,110));visibility=View.GONE;minHeight=dp(48)
            setOnClickListener{showSettings()}
        }
        val tabs=row(layout)
        sendTab=button(tabs,"Send"){selectTab("send")};receiveTab=button(tabs,"Receive"){selectTab("receive")}
        val engine=card(layout);label(engine,"Transfer engine",12f)
        mode=Spinner(this).apply{adapter=ArrayAdapter(this@MainActivity,android.R.layout.simple_spinner_dropdown_item,listOf("EasySendMe","Sendme","Croc"));engine.addView(this)}
        mode.setSelection(savedInstanceState?.getInt("mode") ?: listOf("EasySendMe","Sendme","Croc").indexOf(current.optString("mode","EasySendMe")).coerceAtLeast(0))
        engineDescription=label(engine,"Short code · resumable transfer",12f).apply{setTextColor(Color.LTGRAY)}
        sendPanel=card(layout)
        label(sendPanel,"1 · Choose what to send",17f)
        selection=label(sendPanel,"No files selected")
        val pick=row(sendPanel)
        button(pick,"Files"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").putExtra(Intent.EXTRA_ALLOW_MULTIPLE,true).addCategory(Intent.CATEGORY_OPENABLE),10)}
        button(pick,"Folder"){startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),11)}
        sendCode=input(sendPanel,"Custom sharing code (optional)")
        sendCode.addTextChangedListener(object:android.text.TextWatcher{
            override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){}
            override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){if(codeMode.isNotEmpty() && codeMode!="Sendme")AppSettings.setCode(this@MainActivity,codeMode,s.toString())}
            override fun afterTextChanged(s:android.text.Editable?){}
        })
        sendCodeActions=row(sendPanel)
        sendRecent=button(sendCodeActions,"Recent codes"){showRecent("send",sendCode)}
        button(sendCodeActions,"Clear"){sendCode.setText("");AppSettings.setCode(this,mode.selectedItem.toString(),"")}
        message=input(sendPanel,"Text to send (Croc only)")
        message.apply{setSingleLine(false);minLines=2;maxLines=5;gravity=Gravity.TOP;layoutParams=LinearLayout.LayoutParams(-1,-2);setText(savedInstanceState?.getString("message") ?: "")}
        receivePanel=card(layout)
        label(receivePanel,"1 · Enter the sender’s code",17f)
        code=input(receivePanel,"Paste a code or Sendme ticket")
        code.setText(savedInstanceState?.getString("code") ?: if(tab=="receive")current.optString(if(current.optString("mode")=="Sendme")"ticket" else "code") else "")
        val receiveActions=row(receivePanel)
        button(receiveActions,"Paste"){val clipboard=getSystemService(CLIPBOARD_SERVICE) as ClipboardManager;code.setText(clipboard.primaryClip?.getItemAt(0)?.coerceToText(this));normalizeInput()}
        button(receiveActions,"Scan QR"){scan()}
        receiveRecent=button(receivePanel,"Recent codes"){showRecent("receive",code)}
        saveLocation=label(receivePanel,"",12f).apply{setTextColor(green)}
        updateSaveLocation()
        primary=button(layout,if(tab=="receive")"Receive" else "Send files",true){launch(tab)}
        val transfer=card(layout);transferPanel=transfer;transfer.visibility=View.GONE
        transferDirection=label(transfer,"",12f).apply{setTextColor(Color.LTGRAY)}
        status=label(transfer,"",20f).apply{setTextIsSelectable(true)}
        route=label(transfer,"",12f).apply{setTextColor(green)}
        detail=label(transfer,"").apply{setTextColor(Color.LTGRAY)}
        bar=ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal).apply{max=100;transfer.addView(this,LinearLayout.LayoutParams(-1,dp(8)))}
        ticket=label(transfer,"").apply{setTextIsSelectable(true);setTextColor(green)}
        relayNotice=label(transfer,"").apply{setTextColor(Color.rgb(255,205,110))}
        sharing=row(transfer)
        button(sharing,"Copy code"){val value=shareValue();(getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("DataBeam",value));Toast.makeText(this,"Copied",Toast.LENGTH_SHORT).show()}
        button(sharing,"Show QR"){showQr()}
        pause=button(transfer,"Pause"){startService(Intent(this,TransferService::class.java).setAction(if(TransferService.state(this).optString("action")=="receive")"pause" else "stop"))}
        resumeButton=button(transfer,"Resume",true){resume(false)}
        endPaused=button(transfer,"End transfer"){startService(Intent(this,TransferService::class.java).setAction("stop"))}
        newTransfer=button(transfer,"Dismiss transfer"){val saved=TransferService.state(this);require(saved.optString("status") !in listOf("running","stopping") && !saved.optBoolean("connectionRetained")){"End the current transfer first"};getSharedPreferences("transfer",0).edit().remove("state").apply();previous="";renderedSession="";code.setText("");render()}
        sendmeFallback=button(transfer,"Use Sendme instead"){mode.setSelection(1);code.setText("");alert("Sendme selected. Ask the sender for its ticket, or select files and press Send.")}
        receivedTextTitle=label(transfer,"Received text",12f)
        receivedText=label(transfer,"").apply{setTextIsSelectable(true)}
        val helpRow=row(screen).apply{gravity=Gravity.END;setPadding(dp(20),dp(4),dp(20),dp(8))}
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
        if(savedInstanceState==null)acceptShare(intent)
        if(sources.isNotEmpty())selection.text="${sources.size} item(s) selected"
        selectTab(tab)
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
        val panel=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(22),dp(8),dp(22),dp(16))}
        fun label(value:String)=TextView(this).apply{text=value;setTextColor(ink);setPadding(0,dp(10),0,dp(8));panel.addView(this)}
        fun button(value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;panel.addView(this);setOnClickListener{runCatching(action).onFailure{alert(it.message ?: "Operation failed")}}}
        label("Background transfers")
        settingsBackground=label("").apply{setTextColor(green)}
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
        settingsFolder=label(AppSettings.folderLabel(this)).apply{setTextColor(green)}
        button("Change folder…"){folderPicker.launch(AppSettings.folder(this).takeIf{it.isNotEmpty()}?.let(Uri::parse))}
        button("Use Downloads / DataBeam"){AppSettings.setFolder(this,null);updateSaveLocation()}
        label("Applies to new transfers. Paused transfers keep their original folder.").textSize=12f
        label("Advanced options")
        val custom=EditText(this).apply{hint="Custom Croc relay (optional)";setSingleLine();setText(AppSettings.relay(this@MainActivity));panel.addView(this)}
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
            .setPositiveButton("Done"){_,_->AppSettings.setRelay(this,custom.text.toString())}.create().also{dialog->
                dialog.setOnDismissListener{AppSettings.setRelay(this,custom.text.toString());settingsFolder=null;settingsBackground=null;batteryButton=null;notificationsButton=null;settingsDialog=null};dialog.show()
            }
    }
    private fun selectTab(value:String){tab=value;updatePanels()}
    private fun updatePanels(){
        val selected=mode.selectedItem.toString()
        if(codeMode!=selected){
            codeMode=selected;sendCode.setText(AppSettings.code(this,selected))
        }
        sendTab.background=rounded(if(tab=="send")green else Color.rgb(40,53,69));sendTab.setTextColor(if(tab=="send")Color.BLACK else ink)
        receiveTab.background=rounded(if(tab=="receive")green else Color.rgb(40,53,69));receiveTab.setTextColor(if(tab=="receive")Color.BLACK else ink)
        engineDescription.text=when(mode.selectedItem){"Croc"->"Short code · files, text and browser sharing";"Sendme"->"Ticket · resumable · multiple receivers";else->"Short code · fast, resumable transfer"}
        sendPanel.visibility=if(tab=="send")View.VISIBLE else View.GONE
        receivePanel.visibility=if(tab=="receive")View.VISIBLE else View.GONE
        sendCode.visibility=if(mode.selectedItem=="Sendme")View.GONE else View.VISIBLE
        sendCodeActions.visibility=sendCode.visibility
        sendRecent.visibility=if(AppSettings.recent(this,selected,"send").isNotEmpty())View.VISIBLE else View.GONE
        receiveRecent.visibility=if(selected!="Sendme" && AppSettings.recent(this,selected,"receive").isNotEmpty())View.VISIBLE else View.GONE
        message.visibility=if(tab=="send" && mode.selectedItem=="Croc")View.VISIBLE else View.GONE
        primary.text=if(tab=="send")"Send files" else "Receive"
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
    override fun onSaveInstanceState(out:Bundle){out.putString("tab",tab);out.putInt("mode",mode.selectedItemPosition);out.putString("message",message.text.toString());out.putString("code",code.text.toString());out.putStringArrayList("sources",ArrayList(sources.map{it.toString()}));super.onSaveInstanceState(out)}
    private fun launch(action:String) {
        val state=TransferService.state(this)
        require(state.optString("status") !in listOf("running","stopping")){"Stop the active transfer first"}
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
        val c=JSONObject().put("id",UUID.randomUUID().toString()).put("mode",mode.selectedItem.toString()).put("action",action).put("code",if(mode.selectedItem=="Sendme") "" else if(action=="send")sendCode.text.toString().trim() else code.text.toString().trim()).put("ticket",if(mode.selectedItem=="Sendme")code.text.toString().trim() else "").put("text",message.text.toString()).put("uris",JSONArray(sources.map{it.toString()})).put("relay",AppSettings.relay(this))
        if(action=="receive") require(code.text.isNotBlank()){"Enter a code or ticket"}
        if(action=="send") require(sources.isNotEmpty() || (mode.selectedItem=="Croc" && message.text.isNotEmpty())){"Select files or enter Croc text"}
        if(mode.selectedItem!="Croc") c.put("text","")
        if(action=="send" && mode.selectedItem!="Sendme")AppSettings.setCode(this,mode.selectedItem.toString(),sendCode.text.toString().trim())
        start(c)
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
                        if(NativeBridge.hash(next).isEmpty() || NativeBridge.hash(next)!=NativeBridge.hash(c.optString("ticket"))) {alert("This ticket is invalid or belongs to different content. Start a new transfer.");return@setPositiveButton}
                        c.put("ticket",next)
                    } else c.put("code",entry.text.toString().trim()).put("refresh",true)
                    start(c)
                }
            }.show()
        } else {
            val entered=code.text.toString().trim()
            if(c.optString("action")=="receive" && entered.isNotEmpty()){
                if(c.optString("mode")=="Sendme" && NativeBridge.hash(entered).isNotEmpty()){
                    require(NativeBridge.hash(entered)==NativeBridge.hash(c.optString("ticket"))){"This ticket is for different files. Choose New transfer."};c.put("ticket",entered)
                } else if(c.optString("mode")=="EasySendMe" && entered!=c.optString("code")){c.put("code",entered).put("refresh",true)}
            }
            start(c)
        }
    }
    private fun start(c:JSONObject){
        val current=TransferService.state(this)
        if(current.optBoolean("connectionRetained") && c.optString("mode")=="Croc" && current.optString("id")==c.optString("id") && current.optString("status")=="paused"){
            startService(Intent(this,TransferService::class.java).setAction("resume"));return
        }
        c.remove("log");c.remove("message");startForegroundService(Intent(this,TransferService::class.java).putExtra("config",c.toString()));status.text="Starting…"}
    private fun render() {
        val s=TransferService.state(this);val value=s.toString();if(value==previous)return;previous=value
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
        status.text=when(s.optString("status")){"running"->s.optString("phase",if(receiving)"Receiving" else "Sharing");"paused"->"Paused";"interrupted"->"Transfer interrupted";"stopping"->if(receiving)"Pausing…" else "Ending sharing…";"stopped"->"Sharing ended";"complete"->if(receiving)"Received" else "Sent";else->"Transfer"}
        val paths=s.optJSONObject("senderRoutes")
        val pathValues=paths?.keys()?.asSequence()?.map{paths.optString(it)}?.toList() ?: emptyList()
        val activeRoute=if(s.optString("mode")!="Croc" && !receiving) {
            if(pathValues.isEmpty())"Waiting for receiver" else TransferRoute.label(pathValues)
        } else TransferRoute.label(listOf(s.optString("route","unknown")))
        route.text="${if(s.optString("status")=="complete")"Connection used" else "Connection"} · $activeRoute";route.visibility=if(running || paused || s.has("route"))View.VISIBLE else View.GONE
        val preparing=s.optBoolean("preparing")
        val total=s.optLong(if(preparing)"prepTotal" else "total");val done=s.optLong(if(preparing)"prepDone" else "done")
        val speed=s.optDouble(if(preparing)"prepSpeed" else "speed").takeIf{it.isFinite() && it>=0} ?: 0.0
        val percent=if(total>0 || s.optInt("filesTotal")>0)ProgressEvents.percent(done,total,s.optInt("filesDone"),s.optInt("filesTotal")) else s.optInt("percent")
        val progress=if(total>0 || s.optInt("filesTotal")>0)"$percent% · ${ProgressEvents.size(done)} of ${ProgressEvents.size(total)}" else if(s.has("percent"))"$percent% · ${s.optString("progress")}" else s.optString("progress")
        val rate=if(running && speed>0)"${ProgressEvents.size(speed.toLong())}/s"+ (ProgressEvents.eta(done,total,speed)?.let{" · $it left"} ?: "") else ""
        val files=if(s.optInt("filesTotal")>0)"${s.optInt("filesDone")} / ${s.optInt("filesTotal")} files" else ""
        detail.text=listOf(s.optString("message").takeUnless{it in listOf("Preparing…","Complete")}.orEmpty(),progress,files,rate).filter{it.isNotEmpty()}.joinToString("\n")
        bar.isIndeterminate=running && total==0L && !s.has("percent") && s.optInt("filesTotal")==0
        bar.progress=percent
        bar.visibility=if(running || paused || stopping)View.VISIBLE else View.GONE
        primary.isEnabled=!running && !stopping;primary.alpha=if(primary.isEnabled)1f else .45f
        pause.visibility=if(running || stopping)View.VISIBLE else View.GONE;pause.isEnabled=!stopping;pause.text=if(receiving)"Pause" else "End sharing"
        resumeButton.visibility=if(paused && receiving)View.VISIBLE else View.GONE
        endPaused.visibility=if(paused && s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        newTransfer.visibility=if(!running && !stopping && !s.optBoolean("connectionRetained"))View.VISIBLE else View.GONE
        sharing.visibility=if(running && !receiving)View.VISIBLE else View.GONE
        if(s.optBoolean("troubleshooting")){qrDialog?.dismiss()}
        relayNotice.text=s.optString("relayNotice");relayNotice.visibility=if(relayNotice.text.isEmpty())View.GONE else View.VISIBLE
        sendmeFallback.visibility=if(s.optBoolean("suggestSendme"))View.VISIBLE else View.GONE
        val replacement=s.optString("codeRevision");if(replacement.isNotEmpty() && replacement!=lastReplacement){qrDialog?.dismiss();sendCode.setText(replacement);lastReplacement=replacement}
        ticket.text=if(running && !receiving && !s.optBoolean("troubleshooting"))if(s.optString("mode")=="Sendme")s.optString("ticket") else s.optString("code") else ""
        receivedText.text=s.optString("text");receivedText.visibility=if(receivedText.text.isEmpty())View.GONE else View.VISIBLE;receivedTextTitle.visibility=receivedText.visibility
    }
    private fun alert(s:String){AlertDialog.Builder(this).setMessage(s).setPositiveButton("OK",null).show()}
    @Deprecated("Legacy result API") override fun onActivityResult(request:Int,result:Int,data:Intent?){
        super.onActivityResult(request,result,data);if(request !in listOf(10,11,12) || result!=RESULT_OK || data==null)return
        if(request==12){val source=File(TransferService.state(this).optString("output"));val uri=data.data ?: return
            Thread {runCatching{Storage.export(this,source,uri)}.onSuccess{runOnUiThread{alert("Saved received files")}}.onFailure{runOnUiThread{alert(it.message ?: "Export failed")}}}.start();return}
        sources.clear();data.clipData?.let{for(i in 0 until it.itemCount)sources.add(it.getItemAt(i).uri)} ?: data.data?.let{sources.add(it)}
        sources.forEach{runCatching{contentResolver.takePersistableUriPermission(it,Intent.FLAG_GRANT_READ_URI_PERMISSION)}}
        selection.text="${sources.size} item(s) selected"
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
        sources.clear();sources.addAll(incoming.distinct())
        if(sources.isEmpty()){
            mode.setSelection(2);message.setText(text);selection.text="Text ready to send"
        } else {
            // Some apps attach a caption too. Sending files must not silently switch to text.
            message.setText("");selection.text="${sources.size} shared item(s) selected"
        }
        selectTab("send")
    }
    override fun onNewIntent(intent:Intent){
        super.onNewIntent(intent);setIntent(intent)
        renderedSession=TransferService.state(this).optString("id")
        acceptShare(intent)
    }
    override fun onResume(){super.onResume();updateBackgroundStatus();handler.post(ticker)}
    override fun onPause(){handler.removeCallbacks(ticker);super.onPause()}
}
