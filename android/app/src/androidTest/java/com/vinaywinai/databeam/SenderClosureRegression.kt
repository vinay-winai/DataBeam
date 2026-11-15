package com.vinaywinai.databeam

import android.app.Instrumentation
import android.content.Context
import android.content.ContextWrapper
import android.os.Bundle
import org.json.JSONObject
import java.util.UUID

/** Isolated preferences: never overwrites the user's actual completed card. */
object SenderClosureRegression {
    fun run(runner:Instrumentation){
        val c=runner.targetContext;val preference="sender-closure-${UUID.randomUUID()}"
        val fake=object:ContextWrapper(c){override fun getSharedPreferences(name:String,mode:Int)=c.getSharedPreferences(preference,mode)}
        var code=1;val out=Bundle()
        try {
            check(!TransferService.isBusy()){"A transfer is active"}
            for(status in listOf("running","stopping","stopped","interrupted")){
                val s=JSONObject().put("id","closure-fixture").put("action","send").put("mode","Sendme").put("status",status)
                    .put("phase","Waiting for receiver").put("timing",JSONObject().put("complete",true))
                fake.getSharedPreferences("transfer",0).edit().putString("state",s.toString()).commit()
                val restored=TransferService.state(fake)
                check(restored.optString("status")=="complete"){"Acknowledged $status sender became interrupted"}
                check(TransferLabels.title(restored)=="Sent" && TransferLabels.message(restored)=="Sent successfully.")
            }
            val pending=JSONObject().put("action","send").put("mode","Sendme").put("status","running").put("phase","Transferring")
                .put("done",100).put("total",100).put("timing",JSONObject().put("complete",false))
            fake.getSharedPreferences("transfer",0).edit().putString("state",pending.toString()).commit()
            check(TransferService.state(fake).optString("status")=="interrupted"){"100% without acknowledgement was mistaken for success"}
            out.putString("result","PASS completed sender closure/stop recovery and unacknowledged 100% rejection");code=0
        }catch(error:Throwable){out.putString("error",error.stackTraceToString())}
        finally{c.deleteSharedPreferences(preference)}
        runner.finish(code,out)
    }
}
