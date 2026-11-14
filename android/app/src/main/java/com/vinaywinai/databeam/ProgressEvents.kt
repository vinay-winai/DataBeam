package com.vinaywinai.databeam

import org.json.JSONObject
import kotlin.math.roundToLong

object ProgressEvents {
    data class Sample(val done:Long,val total:Long,val speed:Double)
    data class Telemetry(val preparation:Sample?=null,val speed:Double?=null,val prepared:Boolean=false,val transfer:Sample?=null,val filesDone:Int=0,val filesTotal:Int=0,val verifying:Boolean=false)
    fun native(payload:String):Sample? {
        val parts=payload.split(':');if(parts.size!=3)return null
        val done=parts[0].toLongOrNull() ?: return null;val total=parts[1].toLongOrNull() ?: return null
        val speed=(parts[2].toLongOrNull() ?: return null)/1000.0
        if(done<0 || total<0 || done>total || speed<0)return null
        return Sample(done,total,speed)
    }
    fun croc(line:String):Telemetry?=runCatching {
        when {
            line=="DATABEAM_PREPARED_V1"->Telemetry(prepared=true)
            line=="DATABEAM_VERIFY_V1"->Telemetry(verifying=true)
            line.startsWith("DATABEAM_PREPARE_V1 ")->{
                val json=JSONObject(line.substringAfter(' '));val done=json.getLong("done");val total=json.getLong("total");val speed=json.getDouble("speed_bps")
                require(done>=0 && total>=0 && done<=total && speed.isFinite() && speed>=0)
                Telemetry(preparation=Sample(done,total,speed))
            }
            line.startsWith("DATABEAM_RATE_V1 ")->{val speed=JSONObject(line.substringAfter(' ')).getDouble("speed_bps");require(speed.isFinite() && speed>=0);Telemetry(speed=speed)}
            line.startsWith("DATABEAM_TRANSFER_V1 ")->{
                val json=JSONObject(line.substringAfter(' '));val done=json.getLong("done");val total=json.getLong("total");val speed=json.getDouble("speed_bps")
                val filesDone=json.getInt("files_done");val filesTotal=json.getInt("files_total")
                require(done>=0 && total>=0 && done<=total && speed.isFinite() && speed>=0 && filesDone>=0 && filesDone<=filesTotal)
                Telemetry(transfer=Sample(done,total,speed),filesDone=filesDone,filesTotal=filesTotal)
            }
            else->null
        }
    }.getOrNull()
    fun size(bytes:Long):String=when {
        bytes>=1073741824L->"%.2f GiB".format(bytes/1073741824.0)
        bytes>=1048576L->"%.1f MiB".format(bytes/1048576.0)
        bytes>=1024->"%.1f KiB".format(bytes/1024.0)
        else->"$bytes B"
    }
    fun percent(done:Long,total:Long,filesDone:Int=0,filesTotal:Int=0)=if(total>0)(done.toDouble()/total*100).toInt().coerceIn(0,100) else if(filesTotal>0)(filesDone.toDouble()/filesTotal*100).toInt().coerceIn(0,100) else 0
    fun crocPhase(line:String):String? {
        val clean=line.replace(Regex("\u001B\\[[0-?]*[ -/]*[@-~]"),"").trim().lowercase()
        return when {
            clean=="connecting..."->"Connecting"
            clean=="looking for sender..."->"Finding sender"
            clean=="waiting for sender..."->"Waiting for sender"
            clean=="authenticating code..."->"Authenticating code"
            clean=="opening transfer channels..."->"Opening transfer channels"
            clean=="waiting for file list..."->"Reading file list"
            clean=="waiting for file preparation..."->"Sender is preparing files"
            TransferRoute.croc(clean)!=null->"Transferring"
            else->null
        }
    }
    fun eta(done:Long,total:Long,speed:Double):String? {
        if(!speed.isFinite() || speed<=0 || total<=done)return null
        val seconds=((total-done)/speed).roundToLong().coerceAtLeast(1)
        return when{seconds>=3600->"${seconds/3600}h ${seconds%3600/60}m";seconds>=60->"${seconds/60}m ${seconds%60}s";else->"${seconds}s"}
    }
}
