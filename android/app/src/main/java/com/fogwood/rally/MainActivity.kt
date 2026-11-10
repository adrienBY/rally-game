package com.fogwood.rally

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.opengl.GLSurfaceView
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.io.ByteArrayInputStream
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/**
 * Layout: a GLSurfaceView where the Rust renderer draws the 3D world, with the game's WebView on top
 * (transparent page; it runs the game logic, input, audio and the HUD).
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private var glView: GLSurfaceView? = null
    private val main = Handler(Looper.getMainLooper())

    /** Exposed to the game as window.FogwoodNative. Only present when the Rust library loaded. */
    class Bridge {
        @JavascriptInterface fun prepare(map: Int, path: String, roadh: String, gxh: Int, gzh: Int): String =
            NativeCore.prepare(map, path, roadh, gxh, gzh)
        @JavascriptInterface fun obstacles(csv: String): Int = NativeCore.obstacles(csv)
        @JavascriptInterface fun step(csv: String): String = NativeCore.step(csv)
        @JavascriptInterface fun glStatus(): String = NativeCore.glStatus()
        @JavascriptInterface fun version(): String = "fogwood-core-rust"
    }

    private inner class SceneRenderer : GLSurfaceView.Renderer {
        private var frames = 0
        override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
            val resend = NativeCore.glInit()
            Log.i(TAG, "GL ready: ${NativeCore.glStatus()}")
            checkFailure()
            if (resend) main.post { web.evaluateJavascript("window.__nglResend && window.__nglResend()", null) }
        }
        override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) = NativeCore.glResize(width, height)
        override fun onDrawFrame(gl: GL10?) {
            NativeCore.glDraw()
            frames++
            if (frames < 240 || frames % 600 == 0) checkFailure()
            if (frames % 600 == 1) Log.i(TAG, "renderer: ${NativeCore.glStatus()}")
        }
        /** If the native renderer can't work on this device, reload the page with the WebGL renderer instead. */
        private fun checkFailure() {
            val st = NativeCore.glStatus()
            if (!st.contains("failed") || fellBack) return
            fellBack = true
            Log.e(TAG, "native renderer off: $st")
            main.post { fallBackToWebGl() }
        }
    }
    @Volatile private var fellBack = false

    @SuppressLint("RequiresFeature")
    private fun fallBackToWebGl() {
        try { WebViewCompat.removeWebMessageListener(web, "FogwoodGL") } catch (_: Throwable) {}
        glView?.let { (it.parent as? ViewGroup)?.removeView(it) }
        glView = null
        web.setBackgroundColor(Color.rgb(11, 16, 11))
        web.reload()
    }

    @SuppressLint("SetJavaScriptEnabled", "RequiresFeature")
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

        val root = FrameLayout(this)
        root.setBackgroundColor(Color.rgb(11, 16, 11))

        // native renderer only when the Rust library loaded and this WebView can post binary messages
        val nativeGl = NativeCore.loaded &&
            WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER) &&
            WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_ARRAY_BUFFER)
        if (nativeGl) {
            glView = GLSurfaceView(this).apply {
                setEGLContextClientVersion(3)
                setEGLConfigChooser(8, 8, 8, 0, 16, 0)
                preserveEGLContextOnPause = true
                setRenderer(SceneRenderer())
                renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
            }
            root.addView(glView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }

        web = WebView(this).apply {
            setBackgroundColor(if (nativeGl) Color.TRANSPARENT else Color.rgb(11, 16, 11))
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
        if (nativeGl) {
            // window.FogwoodGL.postMessage(ArrayBuffer): scene once, then one small packet per frame
            WebViewCompat.addWebMessageListener(web, "FogwoodGL", setOf("https://appassets.androidplatform.net")) { _, message, _, _, _ ->
                if (message.type == WebMessageCompat.TYPE_ARRAY_BUFFER) {
                    NativeCore.glMessage(message.arrayBuffer)
                    glView?.requestRender()
                }
            }
        }
        root.addView(web, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        setContentView(root)
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

    /** Back while driving returns to the stage menu; back on the menu leaves the app. */
    @Deprecated("Uses the classic back callback; predictive back is not enabled")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        web.evaluateJavascript("(window.__back && window.__back()) ? 1 : 0") { r -> if (r != "1") finish() }
    }

    override fun onPause() {
        web.evaluateJavascript("window.__appPause && window.__appPause(true)", null)
        web.onPause(); glView?.onPause(); super.onPause()
    }
    override fun onResume() {
        super.onResume(); web.onResume(); glView?.onResume(); hideBars()
        web.evaluateJavascript("window.__appPause && window.__appPause(false)", null)
    }
    override fun onDestroy() { web.destroy(); super.onDestroy() }

    companion object { private const val TAG = "Fogwood" }
}
