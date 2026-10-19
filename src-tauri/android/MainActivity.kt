package com.koi.kellyfin

import android.content.Context
import android.content.pm.ActivityInfo
import android.graphics.Color
import android.media.AudioManager
import android.os.Bundle
import android.os.SystemClock
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlin.math.roundToInt

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

  // Device controls for the player (src/native.ts): screen brightness, media volume, fullscreen.
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.addJavascriptInterface(NativeBridge(this), "KellyfinNative")
  }

  private fun findWebView(v: View): WebView? {
    if (v is WebView) return v
    if (v is ViewGroup) for (i in 0 until v.childCount) findWebView(v.getChildAt(i))?.let { return it }
    return null
  }
}

/** Called from JavaScript on a binder thread: window changes hop to the UI thread. */
class NativeBridge(private val activity: MainActivity) {
  private val audio = activity.getSystemService(Context.AUDIO_SERVICE) as AudioManager

  /** 0..1; the system level when the app has not overridden it. */
  @JavascriptInterface
  fun getBrightness(): Float {
    val own = activity.window.attributes.screenBrightness
    if (own >= 0) return own
    return try {
      Settings.System.getInt(activity.contentResolver, Settings.System.SCREEN_BRIGHTNESS) / 255f
    } catch (e: Exception) {
      0.5f
    }
  }

  /** Brightness for this app's window only (no permission needed); < 0 returns to the system setting. */
  @JavascriptInterface
  fun setBrightness(v: Float) {
    activity.runOnUiThread {
      val attrs = activity.window.attributes
      attrs.screenBrightness = if (v < 0) WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE else v.coerceIn(0.01f, 1f)
      activity.window.attributes = attrs
    }
  }

  @JavascriptInterface
  fun getVolume(): Float {
    val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    return if (max > 0) audio.getStreamVolume(AudioManager.STREAM_MUSIC).toFloat() / max else 0f
  }

  @JavascriptInterface
  fun setVolume(v: Float) {
    val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    audio.setStreamVolume(AudioManager.STREAM_MUSIC, (v * max).roundToInt().coerceIn(0, max), 0)
  }

  /** Hides the system bars and turns landscape regardless of auto-rotate; off restores both. */
  @JavascriptInterface
  fun setFullscreen(on: Boolean) {
    activity.runOnUiThread {
      val bars = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
      if (on) {
        bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        bars.hide(WindowInsetsCompat.Type.systemBars())
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
      } else {
        bars.show(WindowInsetsCompat.Type.systemBars())
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
      }
    }
  }
}
