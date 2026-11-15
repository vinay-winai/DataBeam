package com.vinaywinai.databeam

import org.junit.Assert.*
import org.junit.Test

class GuideNewsTest {
    @Test fun bundledContentIsReadAndRealChangesRemainUnreadUntilOpened(){
        val original=GuideNews.revision("# News\nOriginal\n");val news=GuideNews(original)
        assertFalse(news.hasUnread());news.observe(original);assertFalse(news.hasUnread())
        news.observe(GuideNews.revision("# News\nNew release\n"));assertTrue(news.hasUnread())
        val restored=GuideNews(news.seen,news.latest);assertTrue(restored.hasUnread())
        restored.markRead();assertFalse(restored.hasUnread())
        restored.observe(restored.latest!!);assertFalse(restored.hasUnread())
        restored.observe(GuideNews.revision("Another update"));assertTrue(restored.hasUnread())
    }
    @Test fun platformLineEndingsCannotCreateAnUnreadBadge(){
        assertEquals(GuideNews.revision("# News\r\nHello\r\n"),GuideNews.revision("# News\nHello\n"))
        assertNotEquals(GuideNews.revision("Hello"),GuideNews.revision("Hello!"))
    }
    @Test fun removingGuideSectionsIsUnreadAndAnApkUpgradeDoesNotResetTheBaseline(){
        val old=GuideNews.revision("# News\nHello\n## Previous releases\nOld notes\n")
        val edited=GuideNews.revision("# News\nHello\n")
        val seen=GuideNews.initialSeen(null,old,edited)
        val news=GuideNews(seen,edited)
        assertTrue(news.hasUnread())
        news.markRead();assertFalse(news.hasUnread())
        assertEquals(old,GuideNews.initialSeen(old,edited,edited))
        assertEquals(edited,GuideNews.initialSeen(null,null,edited))
    }
}
