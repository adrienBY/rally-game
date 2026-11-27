package com.fogwood.rally

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.opengl.GLSurfaceView
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.content.ContentValues
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Environment
import android.os.PowerManager
import android.provider.MediaStore
import android.widget.Toast
import android.util.Log
import android.view.Choreographer
import android.view.MotionEvent
import android.view.Surface
import android.view.View
import android.view.SurfaceHolder
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.RenderProcessGoneDetail
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
 * Runtime mode (normal): the Rust engine does everything in a GLSurfaceView - it builds each stage (terrain, props, textures, cars) on a
 * background thread, decodes the recordings, and runs the frame loop, physics, rivals, camera, HUD and sound at 60 fps on the display's
 * vsync. No WebView is made at all.
 * Fallback: if the Rust library or the native renderer can't start, a WebView runs the old all-JavaScript game with WebGL.
 */
class MainActivity : Activity() {
    /** only for the fallback (the old WebGL game); never made in runtime mode */
    private var web: WebView? = null
    private val updater by lazy { Updater(this) }
    private lateinit var root: FrameLayout
    private lateinit var assetLoader: WebViewAssetLoader
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
            if (t - last >= gap) { last = t; vsyncNs = t; glView?.requestRender() }
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    /** the vsync that asked for the frame being drawn: the game's clock, so every frame advances by a whole number of screen refreshes */
    @Volatile private var vsyncNs = 0L
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
    /** frames closer together than this are skipped: half a refresh short of the target interval, so a 60 or 90 Hz
     *  screen runs every refresh and only a faster one skips (120 Hz -> every other) */
    @Suppress("DEPRECATION")
    private fun gapFor(fps: Int): Long {
        val hz = ((if (Build.VERSION.SDK_INT >= 30) display else windowManager.defaultDisplay)?.refreshRate ?: 60f).coerceIn(30f, 240f)
        return (1_000_000_000L / fps - 500_000_000L / hz).toLong().coerceAtLeast(1_000_000L)
    }
    private fun applyMode(id: Int) {
        val lp = window.attributes
        if (lp.preferredDisplayModeId != id) { lp.preferredDisplayModeId = id; window.attributes = lp }
    }

    private inner class SceneRenderer : GLSurfaceView.Renderer {
        private var frames = 0
        private var lastNs = 0L
        override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
            NativeCore.glInit()   // a lost scene is rebuilt by the engine itself
            Log.i(TAG, "GL ready: ${NativeCore.glStatus()}")
            checkFailure()
        }
        override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) = NativeCore.glResize(width, height)
        override fun onDrawFrame(gl: GL10?) {
            if (rtMode) {
                val v = vsyncNs; val dt = if (lastNs == 0L || v <= lastNs) 16_666_667L else v - lastNs; if (v > lastNs) lastNs = v
                val ev = NativeCore.rtFrame(dt)
                if (ev.isNotEmpty()) try { handleEvents(ev) } catch (e: Throwable) { Log.e(TAG, "events", e) }
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

    /** a play-stats note from the game (see rust/src/game/stats.rs): posted to the relay's /stat in the background, best effort */
    private fun postStat(json: String) {
        val v = try { val pi = packageManager.getPackageInfo(packageName, 0); if (Build.VERSION.SDK_INT >= 28) pi.longVersionCode else pi.versionCode.toLong() } catch (_: Exception) { 0L }
        val body = "{\"p\":\"android\",\"v\":$v,\"x\":$json}"
        Thread {
            try {
                val c = java.net.URL("https://fogwood-relay.adrien-paul-bonnay.workers.dev/stat").openConnection() as java.net.HttpURLConnection
                c.requestMethod = "POST"; c.doOutput = true; c.connectTimeout = 8000; c.readTimeout = 8000
                c.setRequestProperty("Content-Type", "text/plain")
                c.outputStream.use { it.write(body.toByteArray()) }; c.responseCode; c.disconnect()
            } catch (_: Exception) {}
        }.start()
    }

    /** events from the native game: "h <kind> <strength>" haptics, "fps", the name dialog, stats, exports (stages are built in Rust) */
    private fun handleEvents(ev: String) {
        for (line in ev.lineSequence()) {
            if (line == "fps 30") vsync.gap = gapFor(30) else if (line == "fps 60") vsync.gap = gapFor(60)
            else if (line.startsWith("h ")) { val p = line.split(' '); if (p.size == 3) haptics.play(p[1], p[2].toFloatOrNull() ?: 0.5f) }
            else if (line.startsWith("name ")) { val first = line.startsWith("name 1"); val cur = line.drop(7); main.post { askName(first, cur) } }
            else if (line.startsWith("stat ")) postStat(line.substring(5))
            else if (line.startsWith("bench ")) { val path = line.substring(6); main.post { exportBench(path) } }
            else if (line.startsWith("crash ")) { val path = line.substring(6); main.post { exportCrash(path) } }
        }
    }

    private var nameDialog: android.app.AlertDialog? = null
    /** the driver name, typed on the phone's own keyboard; `first`: the first run, it can't be skipped */
    private fun askName(first: Boolean, current: String) {
        if (nameDialog?.isShowing == true) return
        val pad = (20 * resources.displayMetrics.density).toInt()
        val edit = android.widget.EditText(this).apply {
            setText(if (first) "" else current); setSelection(text.length); hint = "DRIVER NAME"; isSingleLine = true
            filters = arrayOf(android.text.InputFilter.LengthFilter(12), android.text.InputFilter.AllCaps())
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            imeOptions = android.view.inputmethod.EditorInfo.IME_ACTION_DONE
        }
        val box = android.widget.FrameLayout(this).apply { setPadding(pad, pad / 2, pad, 0); addView(edit) }
        val b = android.app.AlertDialog.Builder(this).setTitle(if (first) "Choose your driver name" else "Driver name")
            .setMessage("Other players see it in online races.").setView(box).setCancelable(!first)
            .setPositiveButton("OK") { _, _ -> val n = edit.text.toString().trim(); if (n.isNotEmpty()) NativeCore.rtName(n) }
        if (!first) b.setNegativeButton("Cancel", null)
        val d = b.create(); nameDialog = d
        d.setOnShowListener {
            val ok = d.getButton(android.app.AlertDialog.BUTTON_POSITIVE)
            fun check() { ok.isEnabled = edit.text.toString().any { it.isLetterOrDigit() } }
            check()
            edit.addTextChangedListener(object : android.text.TextWatcher {
                override fun afterTextChanged(s: android.text.Editable?) = check()
                override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {} })
            edit.setOnEditorActionListener { _, _, _ -> if (ok.isEnabled) ok.performClick(); true }
            edit.requestFocus()
            (getSystemService(INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).showSoftInput(edit, android.view.inputmethod.InputMethodManager.SHOW_IMPLICIT)
        }
        d.window?.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE)
        d.show()
    }

    private var crashShown = false
    /** the game caught an error and carried on: keep its note in Downloads/Fogwood (once per run) so it can be sent along */
    private fun exportCrash(path: String) {
        if (crashShown) return; crashShown = true
        val f = java.io.File(path); if (!f.exists()) return
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                val v = ContentValues().apply { put(MediaStore.Downloads.DISPLAY_NAME, "fogwood-crash-${System.currentTimeMillis() / 1000}.txt"); put(MediaStore.Downloads.MIME_TYPE, "text/plain"); put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Fogwood") }
                val uri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v) ?: return
                contentResolver.openOutputStream(uri)?.use { it.write(f.readBytes()) }
            }
        } catch (e: Throwable) { Log.e(TAG, "crash note export", e) }
    }
    /** The fallback page's process died (low memory): swap in a fresh WebView and reload the game instead of letting the app be killed. */
    private fun pageGone() {
        val old = web
        try { if (old != null) { root.removeView(old); old.destroy() } } catch (_: Throwable) {}
        val w = makeWeb(); web = w; root.addView(w, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        w.loadUrl("https://appassets.androidplatform.net/assets/index.html")
    }
    /** the recordings, straight from the APK's assets into the Rust decoder (one background thread, while the first stage builds) */
    private fun loadSounds() {
        Thread {
            try { for (f in assets.list("snd") ?: emptyArray()) { val b = assets.open("snd/$f").use { it.readBytes() }; if (!NativeCore.audioFile(f, b)) Log.w(TAG, "sound $f not decoded") } }
            catch (e: Throwable) { Log.e(TAG, "sounds", e) }
        }.apply { name = "sounds"; priority = Thread.MIN_PRIORITY }.start()
    }

    /** temporary benchmark: copy the log the game wrote into Downloads/Fogwood so it can be shared */
    private fun exportBench(path: String) {
        val f = java.io.File(path); if (!f.exists()) return
        val where = try {
            if (Build.VERSION.SDK_INT >= 29) {
                val v = ContentValues().apply { put(MediaStore.Downloads.DISPLAY_NAME, f.name); put(MediaStore.Downloads.MIME_TYPE, "text/csv"); put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Fogwood") }
                val uri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v) ?: throw Exception("no uri")
                contentResolver.openOutputStream(uri)?.use { it.write(f.readBytes()) }; "Downloads/Fogwood/${f.name}"
            } else { val d = getExternalFilesDir(null); val o = java.io.File(d, f.name); f.copyTo(o, true); o.absolutePath }
        } catch (e: Throwable) { Log.e(TAG, "bench export", e); f.absolutePath }
        Toast.makeText(this, "Saved: $where", Toast.LENGTH_LONG).show()
    }
    /** thermal status and battery temperature every 2 s, for the benchmark log */
    private val thermalTick = object : Runnable { override fun run() {
        if (!rtMode) return
        val st = if (Build.VERSION.SDK_INT >= 29) (getSystemService(POWER_SERVICE) as PowerManager).currentThermalStatus else -1
        val batt = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, 0) ?: 0
        NativeCore.rtThermal(st, batt / 10f); main.postDelayed(this, 2000) } }

    @SuppressLint("RequiresFeature")
    private fun fallBackToWebGl() {
        stopLoop(); rtMode = false
        glView?.let { (it.parent as? ViewGroup)?.removeView(it) }
        glView = null
        val w = web ?: makeWeb().also { web = it; root.addView(it, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)) }
        w.visibility = View.VISIBLE
        w.loadUrl("https://appassets.androidplatform.net/assets/index.html")
    }

    @SuppressLint("SetJavaScriptEnabled", "RequiresFeature")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        haptics = Haptics(this)

        assetLoader = WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(this))
            .addPathHandler("/native/") { path ->
                if (path == "grids.bin") {
                    val bytes = NativeCore.gridBytes() ?: ByteArray(0)
                    WebResourceResponse("application/octet-stream", null, ByteArrayInputStream(bytes))
                } else null
            }
            .build()

        root = FrameLayout(this)
        root.setBackgroundColor(Color.rgb(11, 16, 11))

        // the native game whenever the Rust library loaded (it starts building the first stage right here, on its own thread)
        val nativeGl = NativeCore.loaded
        rtMode = nativeGl
        if (rtMode) { NativeCore.rtInit(filesDir.absolutePath + "/fogwood.sav"); loadSounds() }
        if (rtMode) NativeCore.rtDevice("${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
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
                    // the first finger: nothing else is down (3 also lets go of anything whose lift was lost)
                    MotionEvent.ACTION_DOWN -> NativeCore.rtTouch(3, e.getPointerId(i), e.getX(i), e.getY(i))
                    MotionEvent.ACTION_POINTER_DOWN -> NativeCore.rtTouch(0, e.getPointerId(i), e.getX(i), e.getY(i))
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

        if (NativeCore.loaded) audio.start()
        updater.check()   // a newer build on GitHub? offer it
        if (!rtMode) {   // no Rust library: the old all-JavaScript game in a WebView
            val w = makeWeb(); web = w
            root.addView(w, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            if (savedInstanceState == null) w.loadUrl("https://appassets.androidplatform.net/assets/index.html") else w.restoreState(savedInstanceState)
        }
        setContentView(root)
        hideBars()
        pickMode(60f)?.let { applyMode(it.modeId); Log.i(TAG, "display ${it.refreshRate} Hz") }
        startLoop()
    }

    /** the fallback page: the old all-JavaScript game drawing with WebGL */
    @SuppressLint("SetJavaScriptEnabled", "RequiresFeature")
    private fun makeWeb(): WebView {
        val web = WebView(this).apply {
            setBackgroundColor(Color.rgb(11, 16, 11))
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
                override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                    Log.e(TAG, "page process gone (crashed: ${detail.didCrash()})")
                    if (view === this@MainActivity.web) main.post { pageGone() }
                    return true
                }
                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                    assetLoader.shouldInterceptRequest(request.url)
            }
            if (NativeCore.loaded) addJavascriptInterface(Bridge(), "FogwoodNative")
        }
        if (NativeCore.loaded && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER) && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_ARRAY_BUFFER)) {
            // window.FogwoodAudio.postMessage(ArrayBuffer): the page drives the Rust sound engine
            WebViewCompat.addWebMessageListener(web, "FogwoodAudio", setOf("https://appassets.androidplatform.net")) { _, message, _, _, _ ->
                if (message.type == WebMessageCompat.TYPE_ARRAY_BUFFER) NativeCore.audioMessage(message.arrayBuffer)
            }
        }
        return web
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
        if (!rtMode) web?.saveState(outState)
    }

    /** Back while driving returns to the stage menu; back on the menu leaves the app. */
    @Deprecated("Uses the classic back callback; predictive back is not enabled")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        if (rtMode) { if (!NativeCore.rtBack()) finish(); return }
        val w = web ?: run { finish(); return }
        w.evaluateJavascript("(window.__back && window.__back()) ? 1 : 0") { r -> if (r != "1") finish() }
    }

    override fun onPause() {
        stopLoop(); main.removeCallbacks(thermalTick)
        if (NativeCore.loaded && rtMode) NativeCore.rtTouch(5, 0, 0f, 0f)   // fingers lifted while away never report: let go of the pedals
        if (!rtMode) web?.evaluateJavascript("window.__appPause && window.__appPause(true)", null)
        web?.onPause(); glView?.onPause(); audio.stop(); super.onPause()
    }
    override fun onResume() {
        super.onResume(); web?.onResume(); glView?.onResume(); hideBars(); if (NativeCore.loaded) audio.start(); updater.resume()
        if (!rtMode) web?.evaluateJavascript("window.__appPause && window.__appPause(false)", null)
        startLoop(); main.removeCallbacks(thermalTick); main.post(thermalTick)
    }
    override fun onDestroy() { audio.stop(); web?.destroy(); super.onDestroy() }

    companion object { private const val TAG = "Fogwood" }
}
