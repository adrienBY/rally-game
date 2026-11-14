package com.fogwood.rally

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.opengl.GLSurfaceView
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Choreographer
import android.view.MotionEvent
import android.view.Surface
import android.view.View
import android.view.SurfaceHolder
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.core.view.ViewCompat
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
 * Runtime mode (normal): the Rust engine runs the whole game in a GLSurfaceView - frame loop, physics, rivals, camera, HUD, sound -
 * driven at 60 fps by the display's vsync. A hidden WebView only builds each stage once and hands it over, then sleeps.
 * Fallback: if the native renderer can't start, the page runs the game itself with WebGL.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private lateinit var haptics: Haptics
    private var glView: GLSurfaceView? = null
    private val main = Handler(Looper.getMainLooper())
    private val audio = AudioOut()
    /** the native game loop is in charge (the page only builds stages) */
    private var rtMode = false
    @Volatile private var looping = false
    /** vsync ticks: one game frame every ~16.7 ms, so 90/120 Hz screens still run a steady 60 */
    private val vsync = object : Choreographer.FrameCallback {
        var last = 0L
        /** 12.5 ms: every vsync at 60 Hz (every other at 120); 29 ms: 30 fps for the menu */
        @Volatile var gap = 12_500_000L
        override fun doFrame(t: Long) {
            if (!looping) return
            if (t - last >= gap) { last = t; glView?.requestRender() }
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    private fun startLoop() { if (rtMode && !looping) { looping = true; vsync.last = 0L; Choreographer.getInstance().postFrameCallback(vsync) } }
    private fun stopLoop() { looping = false; Choreographer.getInstance().removeFrameCallback(vsync) }

    /** Exposed to the game as window.FogwoodNative. Only present when the Rust library loaded. */
    inner class Bridge {
        /** kind: tap | shift | land | impact | rumble; strength 0..1 */
        @JavascriptInterface fun haptic(kind: String, strength: Float) = haptics.play(kind, strength)
        @JavascriptInterface fun prepare(map: Int, path: String, roadh: String, gxh: Int, gzh: Int): String =
            NativeCore.prepare(map, path, roadh, gxh, gzh)
        @JavascriptInterface fun obstacles(csv: String): Int = NativeCore.obstacles(csv)
        @JavascriptInterface fun step(csv: String): String = NativeCore.step(csv)
        @JavascriptInterface fun glStatus(): String = NativeCore.glStatus()
        @JavascriptInterface fun version(): String = "fogwood-core-rust"
    }

    /** The display mode at the current resolution closest to hz: the game runs locked at 60, so the screen is asked for 60 Hz (less work, less battery). */
    @Suppress("DEPRECATION")
    private fun pickMode(hz: Float): android.view.Display.Mode? {
        val d = (if (Build.VERSION.SDK_INT >= 30) display else windowManager.defaultDisplay) ?: return null
        val cur = d.mode
        val same = d.supportedModes.filter { it.physicalWidth == cur.physicalWidth && it.physicalHeight == cur.physicalHeight && it.refreshRate <= 145f }
        if (same.isEmpty()) return null
        return same.minByOrNull { kotlin.math.abs(it.refreshRate - hz) }
    }
    private fun applyMode(id: Int) {
        val lp = window.attributes
        if (lp.preferredDisplayModeId != id) { lp.preferredDisplayModeId = id; window.attributes = lp }
    }

    private inner class SceneRenderer : GLSurfaceView.Renderer {
        private var frames = 0
        private var lastNs = 0L
        override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
            val resend = NativeCore.glInit()
            Log.i(TAG, "GL ready: ${NativeCore.glStatus()}")
            checkFailure()
            if (resend) main.post { web.resumeTimers(); web.evaluateJavascript("window.__nglResend && window.__nglResend()") { if (rtMode) web.pauseTimers() } }
        }
        override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) = NativeCore.glResize(width, height)
        override fun onDrawFrame(gl: GL10?) {
            if (rtMode) {
                val now = System.nanoTime(); val dt = if (lastNs == 0L) 16_666_667L else now - lastNs; lastNs = now
                val ev = NativeCore.rtFrame(dt)
                if (ev.isNotEmpty()) handleEvents(ev)
            } else NativeCore.glDraw()
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
    /** the page has handed its stage over (and had time to finish decoding the recordings): no timers, no work until the next stage */
    private val sleepPage = Runnable { if (rtMode) web.pauseTimers() }

    /** events from the native game: "h <kind> <strength>" haptics, "map <name>" build that stage */
    private fun handleEvents(ev: String) {
        for (line in ev.lineSequence()) {
            if (line == "fps 30") vsync.gap = 29_000_000L else if (line == "fps 60") vsync.gap = 12_500_000L
            else if (line.startsWith("h ")) { val p = line.split(' '); if (p.size == 3) haptics.play(p[1], p[2].toFloatOrNull() ?: 0.5f) }
            else if (line.startsWith("map ")) { val name = line.substring(4).filter { it.isLetter() }
                main.post { main.removeCallbacks(sleepPage); web.resumeTimers(); web.evaluateJavascript("window.__rtBoot && window.__rtBoot('$name')", null) } }
        }
    }

    @SuppressLint("RequiresFeature")
    private fun fallBackToWebGl() {
        stopLoop(); rtMode = false
        try { WebViewCompat.removeWebMessageListener(web, "FogwoodGL") } catch (_: Throwable) {}
        glView?.let { (it.parent as? ViewGroup)?.removeView(it) }
        glView = null
        web.resumeTimers(); web.visibility = View.VISIBLE
        web.setBackgroundColor(Color.rgb(11, 16, 11))
        web.loadUrl("https://appassets.androidplatform.net/assets/index.html")
    }

    @SuppressLint("SetJavaScriptEnabled", "RequiresFeature")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        haptics = Haptics(this)

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
        rtMode = nativeGl
        val startMap = if (rtMode) NativeCore.rtInit(filesDir.absolutePath + "/fogwood.sav") else "forest"
        if (nativeGl) {
            glView = GLSurfaceView(this).apply {
                setEGLContextClientVersion(3)
                setEGLConfigChooser(8, 8, 8, 0, 16, 0)
                preserveEGLContextOnPause = true
                setRenderer(SceneRenderer())
                renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
                // tell the compositor this surface is fixed 60 fps content, so it can keep the panel at 60 Hz
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(h: SurfaceHolder) {
                        if (Build.VERSION.SDK_INT >= 30) try { h.surface.setFrameRate(60f, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE) } catch (_: Throwable) {}
                    }
                    override fun surfaceChanged(h: SurfaceHolder, format: Int, w: Int, hh: Int) {}
                    override fun surfaceDestroyed(h: SurfaceHolder) {}
                })
            }
            root.addView(glView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            // touch goes straight to the native game, in screen pixels
            glView?.setOnTouchListener { _, e ->
                if (!rtMode) return@setOnTouchListener false
                val i = e.actionIndex
                when (e.actionMasked) {
                    MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> NativeCore.rtTouch(0, e.getPointerId(i), e.getX(i), e.getY(i))
                    MotionEvent.ACTION_MOVE -> for (k in 0 until e.pointerCount) NativeCore.rtTouch(1, e.getPointerId(k), e.getX(k), e.getY(k))
                    MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> NativeCore.rtTouch(2, e.getPointerId(i), e.getX(i), e.getY(i))
                    MotionEvent.ACTION_CANCEL -> for (k in 0 until e.pointerCount) NativeCore.rtTouch(2, e.getPointerId(k), e.getX(k), e.getY(k))
                }
                true
            }
            // the camera notch / rounded corners: the HUD keeps clear of them
            ViewCompat.setOnApplyWindowInsetsListener(root) { v, ins ->
                val c = ins.getInsets(WindowInsetsCompat.Type.displayCutout())
                NativeCore.rtInsets(c.top.toFloat(), c.right.toFloat(), c.bottom.toFloat(), c.left.toFloat()); ViewCompat.onApplyWindowInsets(v, ins) }
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
            // no long-press selection / copy-paste menu and no system long-press buzz
            isLongClickable = false
            setOnLongClickListener { true }
            isHapticFeedbackEnabled = false
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
                    val b = message.arrayBuffer
                    NativeCore.glMessage(b)
                    if (!rtMode) glView?.requestRender()
                    // the stage handoff is the page's last word: let it sleep until the next stage is wanted
                    else if (b.size >= 4 && b[0] == 'F'.code.toByte() && b[1] == 'R'.code.toByte() && b[2] == 'T'.code.toByte()) { main.removeCallbacks(sleepPage); main.postDelayed(sleepPage, 8000) }
                }
            }
        }
        if (NativeCore.loaded) {
            // window.FogwoodAudio.postMessage(ArrayBuffer): the page drives the Rust sound engine
            WebViewCompat.addWebMessageListener(web, "FogwoodAudio", setOf("https://appassets.androidplatform.net")) { _, message, _, _, _ ->
                if (message.type == WebMessageCompat.TYPE_ARRAY_BUFFER) NativeCore.audioMessage(message.arrayBuffer)
            }
            audio.start()
        }
        root.addView(web, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        if (rtMode) web.visibility = View.INVISIBLE   // builds stages only; never drawn, never touched
        setContentView(root)
        hideBars()
        pickMode(60f)?.let { applyMode(it.modeId); Log.i(TAG, "display ${it.refreshRate} Hz") }
        if (rtMode) web.loadUrl("https://appassets.androidplatform.net/assets/index.html?rt=1#$startMap")
        else if (savedInstanceState == null) web.loadUrl("https://appassets.androidplatform.net/assets/index.html")
        else web.restoreState(savedInstanceState)
        startLoop()
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
        if (!rtMode) web.saveState(outState)
    }

    /** Back while driving returns to the stage menu; back on the menu leaves the app. */
    @Deprecated("Uses the classic back callback; predictive back is not enabled")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        if (rtMode) { if (!NativeCore.rtBack()) finish(); return }
        web.evaluateJavascript("(window.__back && window.__back()) ? 1 : 0") { r -> if (r != "1") finish() }
    }

    override fun onPause() {
        stopLoop()
        if (!rtMode) web.evaluateJavascript("window.__appPause && window.__appPause(true)", null)
        web.onPause(); glView?.onPause(); audio.stop(); super.onPause()
    }
    override fun onResume() {
        super.onResume(); web.onResume(); glView?.onResume(); hideBars(); if (NativeCore.loaded) audio.start()
        if (!rtMode) web.evaluateJavascript("window.__appPause && window.__appPause(false)", null)
        startLoop()
    }
    override fun onDestroy() { audio.stop(); web.destroy(); super.onDestroy() }

    companion object { private const val TAG = "Fogwood" }
}
