package com.vinaywinai.databeam

import android.app.*
import android.content.*
import android.os.Bundle
import android.view.View
import android.widget.Button

object GuideBadgeRegression {
    fun run(runner:Instrumentation){
        val c=runner.targetContext;var activity:Activity?=null;var monitor:Instrumentation.ActivityMonitor?=null
        val prefs=c.getSharedPreferences("troubleshoot-news",0);var snapshot:Map<String,*>?=null
        var code=1;val result=Bundle()
        try {
            check(!TransferService.isBusy())
            activity=runner.startActivitySync(Intent(c,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            runner.waitForIdleSync()
            val checking=TroubleshootNews::class.java.getDeclaredField("checking").apply{isAccessible=true}.get(TroubleshootNews) as java.util.concurrent.atomic.AtomicBoolean
            val end=System.currentTimeMillis()+20000
            while(checking.get() && System.currentTimeMillis()<end)Thread.sleep(100)
            check(!checking.get());snapshot=prefs.all.toMap()
            TroubleshootNews.observe(c,GuideNews.revision("Generated unread test guidance"))
            fun field(name:String)=MainActivity::class.java.getDeclaredField(name).apply{isAccessible=true}.get(activity)
            val filter=IntentFilter(Intent.ACTION_VIEW).apply{addDataScheme("https")}
            monitor=runner.addMonitor(filter,Instrumentation.ActivityResult(Activity.RESULT_CANCELED,null),true)
            var failure:Throwable?=null
            runner.runOnMainSync{try {
                MainActivity::class.java.getDeclaredMethod("updateTroubleshootBadge").apply{isAccessible=true}.invoke(activity)
                check((field("troubleshootBadge") as View).visibility==View.VISIBLE)
                check((field("troubleshootBadge") as View).z>(field("troubleshootButton") as View).z){"Badge is behind the native button"}
                check((field("troubleshootButton") as Button).stateListAnimator==null){"Button animation can cover the badge"}
            }catch(e:Throwable){failure=e}}
            failure?.let{throw it}
            runner.waitForIdleSync();Thread.sleep(200)
            val location=IntArray(2);var radius=0
            runner.runOnMainSync{val badge=field("troubleshootBadge") as View;badge.getLocationOnScreen(location);radius=badge.width/2}
            val screenshot=runner.uiAutomation.takeScreenshot() ?: error("Badge screenshot unavailable")
            try{
                val pixel=screenshot.getPixel(location[0]+radius,location[1]+radius)
                check(android.graphics.Color.red(pixel)>200 && android.graphics.Color.green(pixel) in 40..130 && android.graphics.Color.blue(pixel)<70){"Badge pixels are obscured despite VISIBLE state"}
            }finally{screenshot.recycle()}
            runner.runOnMainSync{
                (field("troubleshootButton") as Button).performClick()
                check((field("troubleshootBadge") as View).visibility==View.GONE)
            }
            check(monitor.hits==1 && !TroubleshootNews.hasUnread(c))
            result.putString("result","PASS unread troubleshooting badge, guide opening and read acknowledgement");code=0
        }catch(e:Throwable){result.putString("error",e.stackTraceToString())}
        finally {
            monitor?.let{runner.removeMonitor(it)}
            snapshot?.let{saved->val editor=prefs.edit().clear();saved.forEach{(key,value)->if(value is String)editor.putString(key,value)};editor.commit()}
            runner.runOnMainSync{activity?.finish()}
        }
        runner.finish(code,result)
    }
}
