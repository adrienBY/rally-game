package com.fogwood.rally

/** JNI bindings to the Rust core (rust/src/jni_api.rs). */
object NativeCore {
    val loaded: Boolean = try { System.loadLibrary("fogwood_core"); true } catch (e: Throwable) { false }

    /** Builds the stage grids. Returns "ok <cells> <ms>" or "err ...". */
    @JvmStatic external fun prepare(map: Int, path: String, roadh: String, gxh: Int, gzh: Int): String
    /** Packed grids for the game: [pd f32][h f32][pi i16], little endian. */
    @JvmStatic external fun gridBytes(): ByteArray?
    @JvmStatic external fun obstacles(csv: String): Int
    /** One frame of vehicle physics (see rust/src/bridge.rs for the layout). */
    @JvmStatic external fun step(csv: String): String

    // native renderer
    /** GL context created; true if the page must resend its scene. */
    @JvmStatic external fun glInit(): Boolean
    @JvmStatic external fun glResize(w: Int, h: Int)
    @JvmStatic external fun glDraw()
    /** A scene or frame packet posted by the page. */
    @JvmStatic external fun glMessage(data: ByteArray)
    @JvmStatic external fun glStatus(): String

    // runtime mode: the native game loop
    /** Where records and settings are kept; returns the stage to open with. */
    @JvmStatic external fun rtInit(savePath: String): String
    /** Pointer event in screen pixels: action 0 down, 1 move, 2 up/cancel. */
    @JvmStatic external fun rtTouch(action: Int, id: Int, x: Float, y: Float)
    /** Back pressed: true when the game used it (driving -> menu). */
    @JvmStatic external fun rtBack(): Boolean
    @JvmStatic external fun rtInsets(top: Float, right: Float, bottom: Float, left: Float)
    /** GL thread: run and draw one frame (dt in ns); returns app events, one per line. */
    @JvmStatic external fun rtFrame(dtNanos: Long): String

    // sound engine
    @JvmStatic external fun audioInit(sampleRate: Int)
    /** A binary message from the page (params, events, emitters, recordings, stage preset). */
    @JvmStatic external fun audioMessage(data: ByteArray)
    /** Fills [out] with [frames] interleaved stereo frames. */
    @JvmStatic external fun audioRender(out: FloatArray, frames: Int)
}
