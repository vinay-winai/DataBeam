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
}
