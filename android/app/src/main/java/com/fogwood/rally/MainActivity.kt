package com.fogwood.rally

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream

class MainActivity : Activity() {
    private lateinit var web: WebView

    /** Exposed to the game as window.FogwoodNative. Only present when the Rust library loaded. */
    class Bridge {
        @JavascriptInterface fun prepare(map: Int, path: String, roadh: String, gxh: Int, gzh: Int): String =
            NativeCore.prepare(map, path, roadh, gxh, gzh)
        @JavascriptInterface fun obstacles(csv: String): Int = NativeCore.obstacles(csv)
        @JavascriptInterface fun step(csv: String): String = NativeCore.step(csv)
        @JavascriptInterface fun version(): String = "fogwood-core-rust"
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)

        val loader = WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(this))
            .addPathHandler("/native/") { path ->
                if (path == "grids.bin") {
                    val bytes = NativeCore.gridBytes() ?: ByteArray(0)
                    WebResourceResponse("application/octet-stream", null, ByteArrayInputStream(bytes))
                } else null
            }
            .build()

        web = WebView(this).apply {
            setBackgroundColor(Color.rgb(11, 16, 11))
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.mediaPlaybackRequiresUserGesture = false
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            isVerticalScrollBarEnabled = false
            isHorizontalScrollBarEnabled = false
            overScrollMode = WebView.OVER_SCROLL_NEVER
            webViewClient = object : WebViewClient() {
                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                    loader.shouldInterceptRequest(request.url)
            }
            if (NativeCore.loaded) addJavascriptInterface(Bridge(), "FogwoodNative")
        }
        setContentView(web)
        hideBars()
        if (savedInstanceState == null) web.loadUrl("https://appassets.androidplatform.net/assets/index.html")
        else web.restoreState(savedInstanceState)
    }

    private fun hideBars() {
        val c = WindowInsetsControllerCompat(window, window.decorView)
        c.hide(WindowInsetsCompat.Type.systemBars())
        c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideBars()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        web.saveState(outState)
    }

    override fun onPause() {
        web.evaluateJavascript("window.__appPause && window.__appPause(true)", null)
        web.onPause(); super.onPause()
    }
    override fun onResume() {
        super.onResume(); web.onResume(); hideBars()
        web.evaluateJavascript("window.__appPause && window.__appPause(false)", null)
    }
    override fun onDestroy() { web.destroy(); super.onDestroy() }
}
