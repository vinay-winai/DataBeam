package com.vinaywinai.databeam

internal fun android.app.Activity.selectEngineForTest(mode: String) {
    MainActivity::class.java.getDeclaredMethod("selectMode", String::class.java)
        .apply { isAccessible = true }.invoke(this, mode)
}
