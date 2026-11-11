package com.fogwood.rally

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager

/**
 * Game haptics. Uses the phone's tuned haptic primitives when available (Android 11+ with a capable motor),
 * then predefined effects (Android 10), then plain amplitude-controlled pulses.
 */
class Haptics(context: Context) {
    private val vib: Vibrator? = if (Build.VERSION.SDK_INT >= 31)
        (context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
    else @Suppress("DEPRECATION") (context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator)

    private val amp = vib?.hasAmplitudeControl() == true
    private val prims: Set<Int> = if (Build.VERSION.SDK_INT >= 30 && vib != null) {
        val all = mutableListOf(VibrationEffect.Composition.PRIMITIVE_CLICK, VibrationEffect.Composition.PRIMITIVE_TICK)
        if (Build.VERSION.SDK_INT >= 31) { all += VibrationEffect.Composition.PRIMITIVE_LOW_TICK; all += VibrationEffect.Composition.PRIMITIVE_THUD }
        all.filter { vib.arePrimitivesSupported(it)[0] }.toSet()
    } else emptySet()
    @Volatile private var lastRumble = 0L

    fun play(kind: String, strength: Float) {
        val v = vib ?: return
        if (!v.hasVibrator()) return
        val s = strength.coerceIn(0f, 1f)
        try {
            if (kind == "rumble") { val now = System.nanoTime(); if (now - lastRumble < 40_000_000L) return; lastRumble = now }
            v.vibrate(effect(kind, s) ?: return)
        } catch (_: Throwable) {}
    }

    private fun prim(id: Int, scale: Float): VibrationEffect? {
        if (Build.VERSION.SDK_INT < 30 || id !in prims) return null
        return VibrationEffect.startComposition().addPrimitive(id, scale.coerceIn(0.05f, 1f)).compose()
    }

    private fun shot(ms: Long, s: Float): VibrationEffect =
        VibrationEffect.createOneShot(ms, if (amp) (30 + 225 * s).toInt().coerceIn(1, 255) else VibrationEffect.DEFAULT_AMPLITUDE)

    private fun effect(kind: String, s: Float): VibrationEffect? {
        return when (kind) {
            "tap" -> prim(VibrationEffect.Composition.PRIMITIVE_TICK, 0.5f + 0.5f * s)
                ?: predefined(VibrationEffect.EFFECT_TICK) ?: shot(8, 0.5f * s + 0.2f)
            "shift" -> prim(VibrationEffect.Composition.PRIMITIVE_CLICK, 0.35f + 0.5f * s)
                ?: predefined(VibrationEffect.EFFECT_CLICK) ?: shot(14, 0.4f + 0.4f * s)
            "land" -> (if (Build.VERSION.SDK_INT >= 31) prim(VibrationEffect.Composition.PRIMITIVE_THUD, 0.3f + 0.7f * s) else null)
                ?: prim(VibrationEffect.Composition.PRIMITIVE_CLICK, s) ?: shot((14 + 30 * s).toLong(), s)
            "impact" -> (if (Build.VERSION.SDK_INT >= 31) prim(VibrationEffect.Composition.PRIMITIVE_THUD, 0.5f + 0.5f * s) else null)
                ?: (if (s > 0.6f) predefined(VibrationEffect.EFFECT_HEAVY_CLICK) else null) ?: shot((18 + 45 * s).toLong(), 0.5f + 0.5f * s)
            "rumble" -> (if (Build.VERSION.SDK_INT >= 31) prim(VibrationEffect.Composition.PRIMITIVE_LOW_TICK, 0.2f + 0.8f * s) else null)
                ?: prim(VibrationEffect.Composition.PRIMITIVE_TICK, 0.15f + 0.6f * s) ?: shot((6 + 8 * s).toLong(), 0.15f + 0.5f * s)
            else -> null
        }
    }

    private fun predefined(id: Int): VibrationEffect? =
        if (Build.VERSION.SDK_INT >= 29) VibrationEffect.createPredefined(id) else null
}
