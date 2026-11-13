package com.fogwood.rally

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.util.Log

/** Streams the Rust sound engine to the speakers: a low-latency float AudioTrack fed from its own thread. */
class AudioOut {
    @Volatile private var running = false
    private var thread: Thread? = null
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
            thread = Thread({
                android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
                val buf = FloatArray(frames * 2)
                while (running) { NativeCore.audioRender(buf, frames); t.write(buf, 0, buf.size, AudioTrack.WRITE_BLOCKING) }
            }, "fogwood-audio").also { it.start() }
        } catch (e: Throwable) { Log.w("Fogwood", "audio out failed", e); running = false }
    }

    fun stop() {
        running = false
        try { thread?.join(300) } catch (_: Throwable) {}
        thread = null
        try { track?.pause(); track?.flush(); track?.release() } catch (_: Throwable) {}
        track = null
    }
}
