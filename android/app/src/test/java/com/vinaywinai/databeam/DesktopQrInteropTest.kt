package com.vinaywinai.databeam

import com.google.zxing.*
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import javax.imageio.ImageIO
import org.junit.Assert.*
import org.junit.Test

/** Fixtures come from DataBeam's Rust desktop renderer, not Android's QR generator. */
class DesktopQrInteropTest {
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
}
