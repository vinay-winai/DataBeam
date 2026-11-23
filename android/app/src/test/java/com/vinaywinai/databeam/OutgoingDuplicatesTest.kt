package com.vinaywinai.databeam

import org.junit.Assert.assertEquals
import org.junit.Test
import java.nio.file.Files
import java.io.File

class OutgoingDuplicatesTest {
    @Test fun skipsIdenticalSelectedFilesAndKeepsFolders() {
        val root = Files.createTempDirectory("outgoing-duplicates").toFile()
        try {
            val first = File(root, "report.txt").apply { writeText("abc") }
            val copy = File(root, "renamed copy.txt").apply { writeText("abc") }
            val folder = File(root, "folder").apply { mkdir() }
            File(folder, first.name).writeText("abc")
            val other = File(root, "same size.txt").apply { writeText("xyz") }
            val selected = listOf(first, copy, folder, other)
            var comparisons = 0

            assertEquals(listOf(first, folder, other), withoutDuplicateFiles(selected, { false }) { comparisons++ })
            assertEquals(1, comparisons)
            assertEquals("abc", copy.readText())
            assertEquals("abc", File(folder, first.name).readText())
        } finally {
            root.deleteRecursively()
        }
    }
}
