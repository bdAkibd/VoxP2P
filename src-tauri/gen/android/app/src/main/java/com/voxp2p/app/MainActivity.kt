package com.voxp2p.app

import android.os.Bundle

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        try {
            setTheme(androidx.appcompat.R.style.Theme_AppCompat_DayNight_NoActionBar)
        } catch (e: Exception) {
            e.printStackTrace()
        }
        super.onCreate(savedInstanceState)
    }
}
