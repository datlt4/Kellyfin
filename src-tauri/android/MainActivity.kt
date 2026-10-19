package com.koi.kellyfin

import android.graphics.Color
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Edge-to-edge ignores adjustResize, so the keyboard would cover inputs: keep the WebView between
    // the status bar and the keyboard / navigation bar ourselves, in the app's background colour.
    val content = findViewById<View>(android.R.id.content)
    content.setBackgroundColor(Color.parseColor("#0b0d12"))
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      v.setPadding(0, bars.top, 0, maxOf(ime, bars.bottom))
      insets
    }
  }
}
