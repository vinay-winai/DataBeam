package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.os.Bundle
import android.view.View
import android.widget.*
import org.json.JSONObject
import java.io.File
import java.util.UUID

/** Actual settings widgets, native resume choices, QR gates and external folder reuse. */
object SettingsFlowRegression {
    fun run(r:Instrumentation,args:Bundle){
        val c=r.targetContext;val settings=c.getSharedPreferences("settings",0);val saved=settings.all.toMap()
        val transfer=c.getSharedPreferences("transfer",0);val original=transfer.getString("state",null)
        var activity:MainActivity?=null;var exit=1;val out=Bundle()
        fun main(block:()->Unit){var error:Throwable?=null;r.runOnMainSync{try{block()}catch(e:Throwable){error=e}};error?.let{throw it}}
        fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
        fun invoke(name:String){MainActivity::class.java.getDeclaredMethod(name).apply{isAccessible=true}.invoke(activity)}
        fun modeNames():List<String>{val spinner=field("mode") as Spinner;return (0 until spinner.adapter.count).map{spinner.adapter.getItem(it).toString()}}
        fun settingsOpen(){main{invoke("showSettings")}}
        fun closeSettings(){main{(field("settingsDialog") as? AlertDialog)?.dismiss()}}
        fun menuIds():List<Int>{var ids=emptyList<Int>();main{invoke("showQuickActions");val menu=(field("quickMenu") as PopupMenu).menu;ids=(0 until menu.size()).map{menu.getItem(it).itemId};(field("quickMenu") as PopupMenu).dismiss()};return ids}
        fun render(s:JSONObject){transfer.edit().putString("state",s.toString()).commit();main{invoke("render")}}
        fun screenshot(name:String){r.waitForIdleSync();r.uiAutomation.takeScreenshot()?.let{image->File(c.cacheDir,name).outputStream().use{image.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};image.recycle()}}
        fun shell(command:String)=android.os.ParcelFileDescriptor.AutoCloseInputStream(r.uiAutomation.executeShellCommand(command)).bufferedReader().use{it.readText()}
        fun resumed():Triple<String,String,String>{
            val dump=shell("dumpsys activity activities")
            val line=dump.lineSequence().firstOrNull{it.contains("topResumedActivity=ActivityRecord")}
                ?:dump.lineSequence().first{it.contains("mResumedActivity: ActivityRecord") || it.contains("mResumedActivity=ActivityRecord")}
            val m=Regex("ActivityRecord\\{(\\S+) u\\d+ (\\S+) t(\\d+)").find(line) ?: error(line)
            return Triple(m.groupValues[1],m.groupValues[2],m.groupValues[3])
        }
        try{
            check(!TransferService.isBusy())
            settings.edit().remove("disabledEngine").commit()
            activity=r.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            r.waitForIdleSync()
            settingsOpen();main{(field("crocEngineToggle") as Switch).isChecked=false}
            check(modeNames()==listOf("Sendme"));main{
                (field("sendmeEngineToggle") as Switch).isChecked=false
                check((field("sendmeEngineToggle") as Switch).isChecked)
            }
            check(AppSettings.disabledEngine(c)=="Croc");closeSettings();check(menuIds()==listOf(2))
            main{check((field("contentSwitch") as View).visibility==View.GONE);check((field("sendCode") as View).visibility==View.GONE)}
            main{
                val method=MainActivity::class.java.getDeclaredMethod("acceptScannedCode",String::class.java).apply{isAccessible=true}
                try{method.invoke(activity,"generated-short-code");error("Disabled Croc code was accepted")}catch(e:java.lang.reflect.InvocationTargetException){check(e.cause is IllegalArgumentException)}
            }
            settingsOpen();main{(field("crocEngineToggle") as Switch).isChecked=true;(field("sendmeEngineToggle") as Switch).isChecked=false}
            check(modeNames()==listOf("Croc"));screenshot("settings-engines-test.png");closeSettings();check(menuIds()==listOf(1,3))
            main{check((field("broadcastChoice") as View).visibility==View.GONE)}
            val ticket=args.getString("ticket")!!;check(NativeBridge.hash(ticket).isNotEmpty())
            render(JSONObject().put("id",UUID.randomUUID().toString()).put("mode","Croc").put("action","receive").put("status","complete").put("text",ticket))
            main{check((field("receiveTextTicket") as View).visibility==View.GONE)}
            render(JSONObject().put("id",UUID.randomUUID().toString()).put("mode","Sendme").put("action","receive").put("status","paused").put("ticket",ticket))
            main{check((field("transferPanel") as View).visibility==View.GONE)}
            settingsOpen();main{(field("sendmeEngineToggle") as Switch).isChecked=true};closeSettings()
            check(modeNames()==listOf("EasySendMe","Sendme","Croc"));check(menuIds()==listOf(1,2,3))
            for(mode in listOf("Sendme","EasySendMe")){
                render(JSONObject().put("id",UUID.randomUUID().toString()).put("mode",mode).put("action","receive").put("status","paused").put("ticket",ticket).put("code","same-short-code"))
                main{
                    (field("resumeButton") as Button).performClick()
                    val dialog=field("resumeDialog") as AlertDialog
                    check(dialog.findViewById<TextView>(android.R.id.message).text.contains("Pausing a normal desktop send ends"))
                    check(dialog.getButton(DialogInterface.BUTTON_POSITIVE).text.toString()==if(mode=="Sendme")"New ticket" else "New code")
                }
                if(mode=="EasySendMe")screenshot("easy-resume-prompt-test.png")
                main{
                    (field("resumeDialog") as AlertDialog).getButton(DialogInterface.BUTTON_POSITIVE).performClick()
                }
                r.waitForIdleSync()
                main{
                    (field("reconnectDialog") as AlertDialog).getButton(DialogInterface.BUTTON_NEGATIVE).performClick()
                }
                check(!TransferService.isBusy())
            }
            // Both Settings Open and completed-card Open call this same method.
            val method=MainActivity::class.java.getDeclaredMethod("openDownloadFolder",String::class.java).apply{isAccessible=true}
            main{method.invoke(activity,AppSettings.folder(c))};Thread.sleep(1200)
            val first=resumed();check(!first.second.startsWith(c.packageName+"/"))
            main{(c.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).appTasks.first{it.taskInfo.taskId==activity!!.taskId}.moveToFront()}
            Thread.sleep(600);main{method.invoke(activity,AppSettings.folder(c))};Thread.sleep(1000)
            val second=resumed();check(first.second==second.second)
            if(first==second)out.putString("folder_reused",second.second+" task "+second.third)
            else {
                // Host inspects the installed APK manifest; package visibility
                // deliberately prevents the production app querying other apps.
                check(second.second==args.getString("folder_handler") && args.getString("folder_document_mode")=="2"){
                    "Unexpected duplicate file-manager activity: $first -> $second"
                }
                out.putString("folder_reuse_limitation",second.second+" declares documentLaunchMode=always; tasks "+first.third+" and "+second.third)
            }
            out.putString("result","PASS engine hiding/last-engine guard, native Resume choices and folder navigation with verified file-manager window policy");exit=0
        }catch(e:Throwable){out.putString("error",e.stackTraceToString())}
        finally{
            main{(field("settingsDialog") as? AlertDialog)?.dismiss();(field("resumeDialog") as? AlertDialog)?.dismiss();(field("reconnectDialog") as? AlertDialog)?.dismiss();activity?.finish()}
            val edit=settings.edit().clear();saved.forEach{(key,value)->when(value){is String->edit.putString(key,value);is Boolean->edit.putBoolean(key,value);is Int->edit.putInt(key,value);is Long->edit.putLong(key,value);is Float->edit.putFloat(key,value);is Set<*>->edit.putStringSet(key,value.filterIsInstance<String>().toSet())}};edit.commit()
            if(original==null)transfer.edit().remove("state").commit() else transfer.edit().putString("state",original).commit()
        }
        r.finish(exit,out)
    }
}
