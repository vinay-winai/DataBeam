package com.vinaywinai.databeam

import org.junit.Assert.*
import org.junit.Test

class EngineOptionsTest {
    @Test fun disablingAnEngineAlsoHidesItsCombinedModeAndQuickActions(){
        assertEquals(listOf("EasySendMe","Sendme","Croc"),EngineOptions.modes(""))
        assertEquals(listOf("Croc"),EngineOptions.modes("Sendme"))
        assertEquals(listOf(1,3),EngineOptions.quickActions("Sendme"))
        assertEquals(listOf("Sendme"),EngineOptions.modes("Croc"))
        assertEquals(listOf(2),EngineOptions.quickActions("Croc"))
    }
    @Test fun theLastEnabledEngineCannotBeDisabled(){
        for(engine in listOf("Sendme","Croc")){
            val disabled=EngineOptions.change("",engine,false)
            val other=if(engine=="Croc")"Sendme" else "Croc"
            try{EngineOptions.change(disabled,other,false);fail("Disabled both engines")}catch(_:IllegalArgumentException){}
            assertEquals("",EngineOptions.change(disabled,engine,true))
        }
    }
}
