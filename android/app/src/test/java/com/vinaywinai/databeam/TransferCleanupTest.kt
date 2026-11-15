package com.vinaywinai.databeam

import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.file.Files

class TransferCleanupTest {
    @Test fun deletionCannotTargetRootOtherNamespacesOrTraversal(){
        val root=Files.createTempDirectory("databeam-cleanup-").toFile()
        try {
            for(relative in listOf("received","","../outside","received/../outside","received/a/b","shared_prefs/state","blobs/..","/received/a")){
                assertTrue(relative,runCatching{TransferCleanup.target(root,relative)}.isFailure)
            }
        }finally{root.delete()}
    }
    @Test fun deletingOneTransferKeepsItsNeighbourAndOtherPrivateFiles(){
        val root=Files.createTempDirectory("databeam-cleanup-").toFile()
        try {
            val first=File(root,"received/one").apply{mkdirs()};File(first,"payload").writeText("temporary")
            val second=File(root,"received/two").apply{mkdirs()};val sentinel=File(second,"keep").apply{writeText("resume")}
            val other=File(root,"other").apply{writeText("settings")}
            assertTrue(TransferCleanup.remove(root,"received/one")>0)
            assertFalse(first.exists());assertEquals("resume",sentinel.readText());assertEquals("settings",other.readText())
            assertEquals(0,TransferCleanup.remove(root,"received/absent"))
        }finally{root.deleteRecursively()}
    }
}
