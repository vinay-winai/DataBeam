package com.vinaywinai.databeam

/** One disabled-engine value makes disabling both engines unrepresentable. */
object EngineOptions {
    const val CROC_HISTORY_ACTION=5
    fun modes(disabled:String)=when(disabled){
        "Sendme"->listOf("Croc")
        "Croc"->listOf("Sendme")
        else->listOf("EasySendMe","Sendme","Croc")
    }
    fun engines(disabled:String)=listOf("Croc","Sendme").filter{it!=disabled}
    fun change(disabled:String,engine:String,enabled:Boolean):String {
        require(engine in listOf("Sendme","Croc"))
        if(enabled)return if(disabled==engine)"" else disabled
        require(disabled.isEmpty() || disabled==engine){"Keep at least one engine enabled"}
        return engine
    }
    fun quickActions(disabled:String)=buildList{
        if("Croc" in modes(disabled))add(1)
        if("Sendme" in modes(disabled))add(2)
        if("Croc" in modes(disabled))add(3)
        if("EasySendMe" in modes(disabled))add(4)
        add(CROC_HISTORY_ACTION)
    }
}
