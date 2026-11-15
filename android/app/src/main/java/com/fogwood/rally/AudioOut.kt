package com.fogwood.rally

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.util.Log

/** Streams the Rust sound engine to the speakers: a low-latency float AudioTrack fed from its own thread. */
class AudioOut {
    @Volatile private var running = false
    @Volatile private var thread: Thread? = null
    private var track: AudioTrack? = null
    val sampleRate: Int = try { AudioTrack.getNativeOutputSampleRate(AudioManager.STREAM_MUSIC).coerceIn(22050, 96000) } catch (_: Throwable) { 48000 }

    fun start() {
        if (running || !NativeCore.loaded) return
        try {
            NativeCore.audioInit(sampleRate)
            val frames = 256
            val min = AudioTrack.getMinBufferSize(sampleRate, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT)
            val t = AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(sampleRate).setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build())
                .setBufferSizeInBytes(maxOf(min, frames * 8 * 4))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build()
            track = t; t.play(); running = true
            val th = Thread({
                android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
                val buf = FloatArray(frames * 2)
                // a track torn down under it (stop() timing out, an audio route change) ends the loop instead of the app
                val me = Thread.currentThread()
                try { while (running && thread === me) { NativeCore.audioRender(buf, frames); if (t.write(buf, 0, buf.size, AudioTrack.WRITE_BLOCKING) < 0) break } }
                catch (e: Throwable) { Log.w("Fogwood", "audio loop ended", e) }
                finally { if (thread !== me) try { t.release() } catch (_: Throwable) {} }
            }, "fogwood-audio")
            thread = th; th.start()
        } catch (e: Throwable) { Log.w("Fogwood", "audio out failed", e); running = false }
    }

    fun stop() {
        running = false
        // pausing wakes a write that is blocked on the track; the thread then ends (and releases the track if it outlives the wait)
        try { track?.pause() } catch (_: Throwable) {}
        val th = thread; thread = null
        try { th?.join(500) } catch (_: Throwable) {}
        if (th == null || !th.isAlive) try { track?.flush(); track?.release() } catch (_: Throwable) {}
        track = null
    }
}
