package com.vinaywinai.databeam
object NativeBridge {
    init { System.loadLibrary("databeam_android") }
    @JvmStatic external fun initialize(context: android.content.Context)
    @JvmStatic external fun run(config: String): String
    @JvmStatic external fun poll(): String
    @JvmStatic external fun hash(ticket: String): String
    @JvmStatic external fun prepare()
    @JvmStatic external fun cancel()
    @JvmStatic external fun checkSendmeRelay():String
    @JvmStatic external fun networkChanged()
}
