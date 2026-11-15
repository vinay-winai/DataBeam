package com.vinaywinai.databeam

import org.junit.Test
import org.junit.Assert.*
import java.io.File
import java.nio.file.Files

class PayloadNamingTest {
    @Test fun desktopNamingForFilesFoldersAndTies(){
        val root=Files.createTempDirectory("payload-names").toFile()
        try{
            val first=File(root,"small.txt").apply{writeText("a")}
            assertEquals("small.txt_databeam",PayloadNaming.root(listOf(first)))
            val folder=File(root,"Photos").apply{mkdir()}
            File(folder,"nested").mkdir();File(folder,"nested/photo.bin").writeBytes(ByteArray(32))
            assertEquals("Photos_and_more_databeam",PayloadNaming.root(listOf(first,folder)))
            assertEquals("Photos_databeam",PayloadNaming.root(listOf(folder)))
            val tie=File(root,"tie").apply{writeText("a")}
            assertEquals("small.txt_and_more_databeam",PayloadNaming.root(listOf(first,tie)))
            val name="😀".repeat(21);val unicode=File(root,name).apply{writeText("test")}
            assertEquals("😀".repeat(20)+"_databeam",PayloadNaming.root(listOf(unicode)))
        }finally{root.deleteRecursively()}
    }
    @Test fun cancellationStopsFolderWalk(){
        assertThrows(IllegalStateException::class.java){PayloadNaming.root(listOf(File("unused"))){true}}
    }
}
