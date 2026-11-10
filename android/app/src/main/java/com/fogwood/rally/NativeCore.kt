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
}
