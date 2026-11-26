package com.vinaywinai.databeam

import com.google.zxing.*
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import java.awt.Color
import java.awt.image.BufferedImage
import javax.imageio.ImageIO
import org.junit.Assert.*
import org.junit.Test

/** Fixtures come from DataBeam's Rust desktop renderer, not Android's QR generator. */
class DesktopQrInteropTest {
    private fun toLuma(image:BufferedImage):ByteArray {
        val w=image.width;val h=image.height
        val luma=ByteArray(w*h)
        for(y in 0 until h) for(x in 0 until w) {
            val rgb=image.getRGB(x,y)
            val r=(rgb ushr 16) and 0xff;val g=(rgb ushr 8) and 0xff;val b=rgb and 0xff
            luma[y*w+x]=((306*r+601*g+117*b) ushr 10).toByte()
        }
        return luma
    }
    private fun rotate90(src:ByteArray,w:Int,h:Int):ByteArray {
        val dst=ByteArray(w*h)
        for(y in 0 until h) for(x in 0 until w) dst[x*h+(h-1-y)]=src[y*w+x]
        return dst
    }
    @Test fun desktopCodesDecodeWithAndroidScannerHints(){
        for(engine in listOf("croc","easysendme","sendme")){
            val image=ImageIO.read(javaClass.getResourceAsStream("/desktop-qr/$engine.png"))
            val expected=javaClass.getResourceAsStream("/desktop-qr/$engine.txt")!!.bufferedReader().use{it.readText()}
            val pixels=image.getRGB(0,0,image.width,image.height,null,0,image.width)
            val decoded=QRCodeReader().decode(BinaryBitmap(HybridBinarizer(RGBLuminanceSource(image.width,image.height,pixels))),mapOf(DecodeHintType.TRY_HARDER to true))
            assertEquals(engine,expected,decoded.text)
            assertEquals(expected,QrSharing.parse(decoded.text).value)
        }
    }
    @Test fun zxingYuvAndDarkDialogDecodeAllRotations(){
        for(engine in listOf("croc","easysendme","sendme")){
            val image=ImageIO.read(javaClass.getResourceAsStream("/desktop-qr/$engine.png"))
            val expected=javaClass.getResourceAsStream("/desktop-qr/$engine.txt")!!.bufferedReader().use{it.readText()}
            var luma=toLuma(image);var w=image.width;var h=image.height
            for(rot in listOf(0,90,180,270)){
                assertEquals("$engine rot=$rot",expected,QrScannerActivity.decodeYuv(luma,w,h,frameIndex=0))
                luma=rotate90(luma,w,h);val nw=h;h=w;w=nw
            }
            val dialog=BufferedImage(image.width+48,image.height+160,BufferedImage.TYPE_INT_RGB)
            val g=dialog.createGraphics()
            g.color=Color(24,24,24);g.fillRect(0,0,dialog.width,dialog.height)
            g.drawImage(image,24,100,null)
            g.dispose()
            assertEquals("$engine dark dialog",expected,QrScannerActivity.decodeYuv(toLuma(dialog),dialog.width,dialog.height,frameIndex=1))
        }
    }
}
