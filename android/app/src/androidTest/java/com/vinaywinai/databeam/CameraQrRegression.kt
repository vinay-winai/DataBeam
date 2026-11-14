package com.vinaywinai.databeam

import android.app.Instrumentation
import android.graphics.*
import android.os.Bundle
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.TimeUnit

/** Tests the fresh ML Kit scanner against the standard desktop renderer. */
object CameraQrRegression {
    fun run(runner:Instrumentation){
        val scanner=BarcodeScanning.getClient(QrScannerActivity.scannerOptions())
        try {
            var count=0
            for(engine in listOf("croc","easysendme","sendme")){
                val image=runner.context.assets.open("desktop-qr/$engine.png").use{BitmapFactory.decodeStream(it)}!!
                val expected=runner.context.assets.open("desktop-qr/$engine.txt").bufferedReader().use{it.readText()}
                for(rotation in listOf(0,90,180,270)){
                    val result=Tasks.await(scanner.process(InputImage.fromBitmap(image,rotation)),10,TimeUnit.SECONDS)
                    check(result.any{it.rawValue==expected}){"$engine rotation=$rotation did not decode exactly"};count++
                }
                val dialog=Bitmap.createBitmap(image.width+48,image.height+160,Bitmap.Config.ARGB_8888)
                Canvas(dialog).apply{
                    drawColor(Color.rgb(24,24,24));drawBitmap(image,24f,100f,null)
                    val paint=Paint().apply{color=Color.LTGRAY;textSize=20f}
                    drawText("DataBeam sharing QR",24f,35f,paint);drawText("Scan or copy this code",24f,70f,paint)
                }
                val result=Tasks.await(scanner.process(InputImage.fromBitmap(dialog,0)),10,TimeUnit.SECONDS)
                check(result.any{it.rawValue==expected}){"$engine standard QR in desktop dialog did not decode"};count++
            }
            runner.finish(0,Bundle().apply{putString("result","PASS $count standard desktop QR checks with fresh ML Kit scanner: all engines, rotations and dark dialogs")})
        }catch(e:Throwable){runner.finish(1,Bundle().apply{putString("error",e.stackTraceToString())})}
        finally{scanner.close()}
    }
}
