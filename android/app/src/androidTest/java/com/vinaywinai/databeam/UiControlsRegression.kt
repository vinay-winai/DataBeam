package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.view.*
import android.widget.*
import org.json.JSONObject
import java.io.File
import java.util.UUID

object UiControlsRegression {
    fun run(runner:Instrumentation){
        val c=runner.targetContext
        val transfer=c.getSharedPreferences("transfer",0);val original=transfer.getString("state",null)
        val settings=c.getSharedPreferences("settings",0);val savedSettings=settings.all.toMap()
        val created=mutableListOf<Uri>();var activity:Activity?=null
        val sessions=mutableListOf<String>()
        val nonce=UUID.randomUUID().toString();val text="Generated DataBeam UI test $nonce"
        val folder=File(c.cacheDir,"ui-folder-$nonce")
        var code=0;val result=Bundle()
        fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        fun invoke(name:String){MainActivity::class.java.getDeclaredMethod(name).apply{isAccessible=true}.invoke(activity)}
        fun main(block:()->Unit){
            var failure:Throwable?=null
            runner.runOnMainSync{try{block()}catch(error:Throwable){failure=error}}
            failure?.let{throw it}
        }
        fun waitFor(predicate:(JSONObject)->Boolean):JSONObject {
            val deadline=System.currentTimeMillis()+25000
            while(System.currentTimeMillis()<deadline){val s=TransferService.state(c);if(predicate(s)){Thread.sleep(600);return s};Thread.sleep(100)}
            error("Timed out waiting for UI test state: ${TransferService.state(c).optString("status")}")
        }
        fun find(view:View,test:(View)->Boolean):View? {
            if(test(view))return view
            if(view is ViewGroup)for(i in 0 until view.childCount)find(view.getChildAt(i),test)?.let{return it}
            return null
        }
        fun screenshot(name:String){runner.waitForIdleSync();runner.uiAutomation.takeScreenshot()?.let{bitmap->File(c.cacheDir,name).outputStream().use{bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()}}
        try {
            check(TransferService.state(c).optString("status") !in listOf("running","stopping")){"A user transfer is active"}
            fun document(name:String):Uri {
                val uri=c.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply{
                    put(MediaStore.Downloads.DISPLAY_NAME,name);put(MediaStore.Downloads.MIME_TYPE,"text/plain");put(MediaStore.Downloads.RELATIVE_PATH,"Download/DataBeam/")
                })!!;created.add(uri);c.contentResolver.openOutputStream(uri)!!.use{it.write("generated fixture".toByteArray())};return uri
            }
            val firstName="ui-first-$nonce.txt";val secondName="ui-second-$nonce.txt"
            val first=document(firstName);val second=document(secondName)
            activity=runner.startActivitySync(Intent(c,MainActivity::class.java).setAction(Intent.ACTION_SEND_MULTIPLE).setType("text/plain").putParcelableArrayListExtra(Intent.EXTRA_STREAM,arrayListOf(first,second)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            runner.waitForIdleSync()
            main {
                fun quick(id:Int){
                    (field("quickActions") as Button).performClick()
                    val popup=field("quickMenu") as PopupMenu
                    check(popup.menu.findItem(EngineOptions.CROC_HISTORY_ACTION).title=="Croc receive history")
                    check(popup.menu.size()==5);check(popup.menu.performIdentifierAction(id,0));popup.dismiss()
                }
                quick(1)
                check((field("mode") as Spinner).selectedItem=="Croc" && (field("message") as View).visibility==View.VISIBLE)
                check((field("primary") as Button).background.let{it as android.graphics.drawable.GradientDrawable}.color?.defaultColor==android.graphics.Color.rgb(87,214,36))
                check((field("historyButton") as View).visibility==View.GONE)
                quick(2)
                check((field("mode") as Spinner).selectedItem=="Sendme" && (field("broadcastChoice") as CheckBox).isChecked)
                check((field("primary") as Button).background.let{it as android.graphics.drawable.GradientDrawable}.color?.defaultColor==android.graphics.Color.rgb(11,131,230))
                (field("receiveTab") as Button).performClick()
                check((field("historyButton") as View).visibility==View.GONE)
                quick(3)
                check((field("mode") as Spinner).selectedItem=="Croc" && (field("browserGuidance") as View).visibility==View.VISIBLE)
                check((field("browserGuidance") as TextView).text.contains("https://getcroc.com"))
                quick(4)
                val selectedMode=MainActivity::class.java.getDeclaredMethod("selectedMode").apply{isAccessible=true}
                check(selectedMode.invoke(activity)=="EasySendMe")
                check((field("primary") as Button).background.let{it as android.graphics.drawable.GradientDrawable}.color?.defaultColor==android.graphics.Color.rgb(214,66,36))
                quick(1)
                check((field("browserGuidance") as View).visibility==View.GONE)
                (field("receiveTab") as Button).performClick()
                check((field("historyButton") as View).visibility==View.VISIBLE)
                (field("sendTab") as Button).performClick()
                (field("filesChoice") as Button).performClick()
            }
            main {
                check((field("selectionList") as LinearLayout).childCount==2)
                val root=activity!!.window.decorView
                check(find(root){it is TextView && it.text.toString()==firstName}!=null){"Selected name missing"}
                check(find(root){it.contentDescription=="Version 0.8.8"}!=null){"Version footer missing"}
                check(find(root){it.contentDescription=="DataBeam logo"}!=null){"Arrow branding missing"}
            }
            screenshot("ui-controls-files.png")
            fun settingsRoot()=(field("settingsDialog") as AlertDialog).window!!.decorView
            fun header(description:String)=find(settingsRoot()){it.contentDescription==description}
            fun settingsScroll():ScrollView {
                val transfers=find(settingsRoot()){it.contentDescription?.startsWith("Transfers, ")==true}!!
                return (transfers.parent.parent as ScrollView)
            }
            main {
                for((section,expanded) in listOf("transfers" to true,"storage" to false,"background" to false,"relays" to false)){
                    AppSettings.setSettingsSectionExpanded(c,section,expanded)
                }
                invoke("showSettings")
            }
            Thread.sleep(250)
            main {
                check(settingsScroll().scrollY==0){"Settings scrolled with only Transfers expanded"}
                check(header("Transfers, expanded")!!.performClick())
                check(header("Files and storage, collapsed")!!.performClick())
                check(header("Background transfers, collapsed")!!.performClick())
                check(header("Relays, collapsed")!!.performClick())
            }
            Thread.sleep(250)
            main {
                check(settingsScroll().scrollY>0 && !settingsScroll().canScrollVertically(1)){
                    "Expanding a lower Settings section did not scroll to the bottom"
                }
                (field("settingsDialog") as AlertDialog).dismiss()
            }
            runner.waitForIdleSync()
            main{invoke("showSettings")}
            Thread.sleep(250)
            main {
                val root=(field("settingsDialog") as AlertDialog).window!!.decorView
                for(description in listOf("Transfers, collapsed","Files and storage, expanded","Background transfers, expanded","Relays, expanded")){
                    check(find(root){it.contentDescription==description}!=null){"Settings forgot $description"}
                }
                check(settingsScroll().scrollY>0 && !settingsScroll().canScrollVertically(1)){
                    "Settings did not reopen at the bottom with lower sections expanded"
                }
                (field("settingsDialog") as AlertDialog).dismiss()
            }
            main{
                check(find(activity!!.window.decorView){it is Button && it.text=="Clear all"}!!.performClick())
                check((field("sources") as List<*>).isEmpty())
                runner.callActivityOnNewIntent(activity,Intent(Intent.ACTION_SEND_MULTIPLE).setType("text/plain").putParcelableArrayListExtra(Intent.EXTRA_STREAM,arrayListOf(first,second)))
            }
            main {
                check(find(activity!!.window.decorView){it.contentDescription=="Remove $firstName"}!!.performClick())
                check((field("sources") as List<*>).size==1)
                folder.mkdir()
                MainActivity::class.java.getDeclaredMethod("onActivityResult",Int::class.javaPrimitiveType,Int::class.javaPrimitiveType,Intent::class.java).apply{isAccessible=true}.invoke(activity,11,Activity.RESULT_OK,Intent().setData(Uri.fromFile(folder)))
                check((field("sources") as List<*>).size==2){"Adding a folder replaced the selection"}
                check(find(activity!!.window.decorView){it.contentDescription=="Remove ${folder.name}"}!!.performClick())
                check(find(activity!!.window.decorView){it.contentDescription=="Remove $secondName"}!!.performClick())
                check((field("sources") as List<*>).isEmpty())
                (field("mode") as Spinner).setSelection(0)
            }
            runner.waitForIdleSync()
            main {
                (field("textChoice") as Button).performClick()
                (field("message") as EditText).setText(text)
                (field("sendCode") as EditText).setText("")
                check((field("fileSelectionPanel") as View).visibility==View.GONE)
                check((field("message") as View).visibility==View.VISIBLE)
                check((field("primary") as Button).text=="Send text")
                (field("filesChoice") as Button).performClick()
                check((field("message") as View).visibility==View.GONE)
                check((field("primary") as Button).text=="Send files")
                (field("textChoice") as Button).performClick()
                AppSettings.setRelay(c,"127.0.0.1:19009")
            }
            screenshot("ui-controls-text.png")
            main{(field("primary") as Button).performClick()}
            val firstSession=waitFor{it.optString("status")=="running" && it.optString("phase")=="Waiting for receiver" && it.optString("text")==text}
            sessions.add(firstSession.getString("id"))
            val words=c.assets.open("eff-short-wordlist-1.txt").bufferedReader().use{it.readLines().toSet()}
            fun phrase(value:String,count:Int):Boolean=if(count==1)value in words else
                value.indices.any{index->value[index]=='-' && value.take(index) in words && phrase(value.substring(index+1),count-1)}
            check(phrase(firstSession.optString("code"),3)){"Generated code did not use the desktop word list"}
            main {
                check(!(field("mode") as View).isEnabled)
                check(!(field("quickActions") as View).isEnabled)
                check(!(field("message") as View).isEnabled)
                check(!(field("sendCode") as View).isEnabled)
                check(!(field("receiveTab") as View).isEnabled)
                check((field("pause") as View).isEnabled)
                invoke("showSettings")
                val controls=field("settingsConfiguration") as List<*>
                check(controls.any{it is Button && it.text=="Review and clean transfer storage"}){"Storage cleanup setting missing"}
                check(controls.all{!(it as View).isEnabled}){"Transfer settings could change while active"}
                (field("settingsDialog") as AlertDialog).dismiss()
            }
            val notification=c.getSystemService(NotificationManager::class.java).activeNotifications.firstOrNull{it.id==1}?.notification ?: error("Active transfer notification missing")
            check(notification.extras.getInt(Notification.EXTRA_PROGRESS_MAX)==100){"Notification progress bar missing"}
            check(notification.flags and Notification.FLAG_ONGOING_EVENT!=0){"Transfer notification was not ongoing"}
            check(notification.extras.getCharSequence(Notification.EXTRA_TEXT)?.contains("Elapsed")==true){"Notification elapsed time missing"}
            screenshot("ui-controls-active.png")
            main{(field("pause") as Button).performClick()}
            waitFor{it.optString("status")=="stopped" && it.optString("id")==firstSession.optString("id")}
            main {
                check((field("resend") as View).visibility==View.VISIBLE)
                check((field("resumeButton") as View).visibility==View.GONE)
                check(!(field("detail") as TextView).text.contains("Press Resume"))
                check((field("route") as View).visibility==View.GONE){"Stopped share still said waiting for receiver"}
                check((field("newTransfer") as Button).text=="Clear transfer card")
                (field("message") as EditText).setText("")
                (field("resend") as Button).performClick()
                check((field("message") as EditText).text.toString()==text){"Resend did not restore the text"}
            }
            val restarted=waitFor{it.optString("status")=="running" && it.optString("phase")=="Waiting for receiver" && it.optString("id")!=firstSession.optString("id")}
            sessions.add(restarted.getString("id"))
            check(restarted.optString("text")==text){"Restart lost the previous text"}
            check(restarted.optString("code")==firstSession.optString("code")){"Restart unexpectedly changed the Croc code"}
            main{(field("pause") as Button).performClick()}
            waitFor{it.optString("status")=="stopped" && it.optString("id")==restarted.optString("id")}
            main {
                runner.callActivityOnNewIntent(activity,Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_STREAM,first))
                (field("primary") as Button).performClick()
            }
            val fileSession=waitFor{it.optString("status")=="running" && it.optString("phase")=="Waiting for receiver" && it.optString("text").isEmpty()}
            sessions.add(fileSession.getString("id"))
            check(fileSession.optJSONArray("paths")?.length()==1 && fileSession.optInt("referencedFiles")==1 && fileSession.optInt("copiedFiles")==0){"Seekable file was not referenced"}
            main{(field("pause") as Button).performClick()}
            waitFor{it.optString("status")=="stopped" && it.optString("id")==fileSession.optString("id")}
            main{
                check((field("resend") as View).visibility==View.VISIBLE){"Resend missing on stopped file transfer"}
                check((field("sources") as List<*>)==listOf(first)){"Stopping cleared the file selection"}
                check((field("detail") as TextView).text.contains("Tap Resend to share the same files"))
                (field("clearFiles") as Button).performClick()
                (field("resend") as Button).performClick()
                check((field("sources") as List<*>)==listOf(first)){"Resend did not restore the original URI"}
            }
            val fileRestart=waitFor{it.optString("status")=="running" && it.optString("phase")=="Waiting for receiver" && it.optString("id")!=fileSession.optString("id")}
            sessions.add(fileRestart.getString("id"))
            check(fileRestart.optInt("referencedFiles")==1 && fileRestart.optInt("copiedFiles")==0){"Restart did not reopen the original URI"}
            check(fileRestart.getJSONArray("paths").toString()!=fileSession.getJSONArray("paths").toString()){"Restart reused closed descriptor aliases"}
            main{(field("pause") as Button).performClick()}
            val stopped=waitFor{it.optString("status")=="stopped" && it.optString("id")==fileRestart.optString("id")}
            stopped.getJSONArray("uris").put(Uri.fromFile(File(c.cacheDir,"missing-$nonce.bin")).toString())
            stopped.put("sourceNames",org.json.JSONArray().put(firstName).put("missing-$nonce.bin"))
            transfer.edit().putString("state",stopped.toString()).commit()
            main {
                invoke("render")
                (field("resend") as Button).performClick()
                check((field("sources") as List<*>)==listOf(first)){"Resend lost the available part of the selection"}
            }
            runner.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
            check(TransferService.state(c).optString("id")==fileRestart.optString("id") && !TransferService.isBusy()){
                "Resend started with an incomplete selection"
            }
            main {
                val clear=field("clearFiles") as Button
                check(clear.visibility==View.VISIBLE)
                clear.performClick()
                check((field("sources") as List<*>).isEmpty()){"Clear all retained the selected file"}
            }
            result.putString("result","PASS selection controls, engine accents, Settings section memory and bottom scroll, immediate file and text resend, missing-file guard, transfer lock and ongoing notification progress")
        }catch(error:Throwable){code=1;result.putString("error",error.stackTraceToString())}
        finally {
            if(TransferService.state(c).optString("id") in sessions && TransferService.state(c).optString("status") in listOf("running","stopping")){
                c.startService(Intent(c,TransferService::class.java).setAction("stop"));runCatching{waitFor{it.optString("status") !in listOf("running","stopping")}}
            }
            main{activity?.finish()};runner.waitForIdleSync()
            if(original==null)transfer.edit().remove("state").commit() else transfer.edit().putString("state",original).commit()
            val editor=settings.edit().clear()
            savedSettings.forEach{(key,value)->when(value){is String->editor.putString(key,value);is Boolean->editor.putBoolean(key,value);is Int->editor.putInt(key,value);is Long->editor.putLong(key,value);is Float->editor.putFloat(key,value);is Set<*>->editor.putStringSet(key,value.filterIsInstance<String>().toSet())}}
            editor.commit();created.forEach{runCatching{c.contentResolver.delete(it,null,null)}};folder.delete()
            sessions.forEach{id->
                UUID.fromString(id)
                File(c.filesDir,"transfer-records/$id.json").delete()
                File(c.filesDir,"transfer-records/$id.tmp").delete()
                for(root in listOf("outgoing","received","blobs")){
                    val parent=File(c.filesDir,root).canonicalFile
                    val child=File(parent,id).canonicalFile
                    check(child.parentFile==parent)
                    child.deleteRecursively()
                }
            }
        }
        runner.finish(code,result)
    }
}
