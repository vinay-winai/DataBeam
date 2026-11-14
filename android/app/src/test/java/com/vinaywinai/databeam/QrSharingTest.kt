package com.vinaywinai.databeam
import com.google.zxing.BinaryBitmap
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import org.junit.Assert.*
import org.junit.Test

class QrSharingTest {
    @Test fun generatedQrPreservesCodesAndTicketsExactly() {
        for(value in listOf("birch-coral-otter","custom123", "blob"+"a".repeat(220), "custom-\u03b1\u03b2\u03b3")) {
            val matrix=QrSharing.matrix(value)
            val pixels=IntArray(matrix.width*matrix.height){i->if(matrix[i%matrix.width,i/matrix.width])0xff000000.toInt() else 0xffffffff.toInt()}
            val decoded=QRCodeReader().decode(BinaryBitmap(HybridBinarizer(RGBLuminanceSource(matrix.width,matrix.height,pixels))))
            assertEquals(value,decoded.text)
        }
    }
    @Test fun scannerAcceptsPlainCodesAndSendmeTickets() {
        assertEquals("birch-coral-otter",QrSharing.parse("birch-coral-otter").value)
        assertTrue(QrSharing.parse("blob"+"a".repeat(220)).sendme)
        assertFalse(QrSharing.parse("custom123").sendme)
    }
    @Test fun browserCrocLinkExtractsOnlyTheCode() {
        val parsed=QrSharing.parse("https://getcroc.com/?code=custom%2B123")
        assertEquals("custom+123",parsed.value)
        assertTrue(parsed.crocLink)
        assertFalse(parsed.sendme)
    }
    @Test fun unrelatedLinksAndMalformedPayloadsAreRejected() {
        for(value in listOf("https://example.com/?code=custom123","https://getcroc.com/","file:///private","abc", "abc1234\nnext")) {
            assertTrue(value,runCatching{QrSharing.parse(value)}.isFailure)
        }
    }
}
