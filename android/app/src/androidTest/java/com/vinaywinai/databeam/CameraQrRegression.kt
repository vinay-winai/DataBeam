package com.vinaywinai.databeam

import android.app.Instrumentation
import android.graphics.*
import android.os.Bundle

/** Tests the on-device ZXing scanner against the standard desktop renderer. */
object CameraQrRegression {
    private fun decode(bitmap:Bitmap):String? {
        val pixels=IntArray(bitmap.width*bitmap.height)
        bitmap.getPixels(pixels,0,bitmap.width,0,0,bitmap.width,bitmap.height)
        for(frame in 0 until 4) {
            QrScannerActivity.decodePixels(pixels,bitmap.width,bitmap.height,frame)?.let{return it}
        }
        return null
    }
    private fun rotate(source:Bitmap,degrees:Float):Bitmap {
        if(degrees==0f)return source
        val matrix=Matrix().apply{postRotate(degrees)}
        return Bitmap.createBitmap(source,0,0,source.width,source.height,matrix,true)
    }
    fun run(runner:Instrumentation){
        try {
            var count=0
            for(engine in listOf("croc","easysendme","sendme")){
                val image=runner.context.assets.open("desktop-qr/$engine.png").use{BitmapFactory.decodeStream(it)}!!
                val expected=runner.context.assets.open("desktop-qr/$engine.txt").bufferedReader().use{it.readText()}
                for(rotation in listOf(0,90,180,270)){
                    val rotated=rotate(image,rotation.toFloat())
                    val decoded=decode(rotated)
                    check(decoded==expected){"$engine rotation=$rotation did not decode exactly"};count++
                }
                val dialog=Bitmap.createBitmap(image.width+48,image.height+160,Bitmap.Config.ARGB_8888)
                Canvas(dialog).apply{
                    drawColor(Color.rgb(24,24,24));drawBitmap(image,24f,100f,null)
                    val paint=Paint().apply{color=Color.LTGRAY;textSize=20f}
                    drawText("DataBeam sharing QR",24f,35f,paint);drawText("Scan or copy this code",24f,70f,paint)
                }
                check(decode(dialog)==expected){"$engine standard QR in desktop dialog did not decode"};count++
            }
            runner.finish(0,Bundle().apply{putString("result","PASS $count standard desktop QR checks with offline ZXing scanner: all engines, rotations and dark dialogs")})
        }catch(e:Throwable){runner.finish(1,Bundle().apply{putString("error",e.stackTraceToString())})}
    }
}
