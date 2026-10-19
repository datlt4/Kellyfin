package com.koi.kellyfin

import android.graphics.Color
import android.os.Bundle
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  private var lastBack = 0L

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
    // Back goes back through the app's pages; on the first page it asks for a second back within 2 s.
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val web = findWebView(content)
        if (web != null && web.canGoBack()) {
          web.goBack()
          return
        }
        val now = SystemClock.elapsedRealtime()
        if (now - lastBack < 2000) {
          moveTaskToBack(true)
        } else {
          lastBack = now
          Toast.makeText(this@MainActivity, "Swipe back again to exit", Toast.LENGTH_SHORT).show()
        }
      }
    })
  }

  private fun findWebView(v: View): WebView? {
    if (v is WebView) return v
    if (v is ViewGroup) for (i in 0 until v.childCount) findWebView(v.getChildAt(i))?.let { return it }
    return null
  }
}
