package com.vinaywinai.databeam
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import java.net.URI
import java.net.URLDecoder

object QrSharing {
    data class Payload(val value:String,val sendme:Boolean,val crocLink:Boolean=false)
    fun matrix(value:String)=QRCodeWriter().encode(value,BarcodeFormat.QR_CODE,640,640,mapOf(EncodeHintType.CHARACTER_SET to "UTF-8"))
    fun parse(raw:String):Payload {
        val text=raw.trim()
        require(text.isNotEmpty() && text.length<=8192){"QR code is empty or too large"}
        if(text.startsWith("https://") || text.startsWith("http://")) {
            val uri=URI(text)
            require(uri.host?.lowercase() in listOf("getcroc.com","www.getcroc.com")){"This QR contains an unsupported link. Scan a Croc code or Sendme ticket."}
            val query=uri.rawQuery.orEmpty().split('&').map{it.split('=',limit=2)}
            val value=query.firstOrNull{it[0]=="code" && it.size==2}?.get(1)?.let{URLDecoder.decode(it,"UTF-8")} ?: error("This Croc link has no sharing code")
            require(value.length>=7 && !value.any{it=='\n' || it=='\r' || it=='\u0000'}){"Invalid Croc sharing code"}
            return Payload(value,false,true)
        }
        require(!text.contains("://") && text.length>=7 && !text.any{it.isWhitespace() || it=='\u0000'}){"This QR is not a sharing code or Sendme ticket"}
        return Payload(text,text.startsWith("blob"))
    }
}
