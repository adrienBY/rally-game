package com.fogwood.rally

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * Keeps the app on the newest build: at launch it asks GitHub for the latest release ("build-N"); if N is newer than
 * this app's build it offers to update, downloads the APK and hands it to Android's installer (same signing key, so
 * it installs over this one, keeping the saves). Android always asks the player to confirm an install.
 */
class Updater(private val act: Activity) {
    private val main = Handler(Looper.getMainLooper())
    private val prefs = act.getSharedPreferences("fogwood-update", Activity.MODE_PRIVATE)
    private var pending: File? = null

    private fun myBuild(): Long = try {
        val pi = act.packageManager.getPackageInfo(act.packageName, 0)
        if (Build.VERSION.SDK_INT >= 28) pi.longVersionCode else @Suppress("DEPRECATION") pi.versionCode.toLong()
    } catch (_: Throwable) { Long.MAX_VALUE }

    fun check() {
        Thread({
            try {
                val c = URL(API).openConnection() as HttpURLConnection
                c.setRequestProperty("Accept", "application/vnd.github+json"); c.setRequestProperty("User-Agent", "FogwoodRally")
                c.connectTimeout = 6000; c.readTimeout = 8000
                if (c.responseCode != 200) return@Thread
                // the newest "build-N" release (other releases, like the sound engine's, are skipped)
                val all = JSONArray(c.inputStream.bufferedReader().readText())
                var j: JSONObject? = null
                for (i in 0 until all.length()) { val r = all.getJSONObject(i); if (r.optString("tag_name").startsWith("build-") && !r.optBoolean("prerelease")) { j = r; break } }
                if (j == null) return@Thread
                val n = j.optString("tag_name").removePrefix("build-").toLongOrNull() ?: return@Thread
                val assets = j.optJSONArray("assets") ?: return@Thread
                var url: String? = null
                for (i in 0 until assets.length()) { val a = assets.getJSONObject(i); if (a.optString("name").endsWith(".apk")) url = a.optString("browser_download_url") }
                val apk = url ?: return@Thread
                val later = prefs.getLong("later-$n", 0L)
                if (n > myBuild() && System.currentTimeMillis() - later > 6 * 3600_000L) main.post { offer(n, apk) }
            } catch (e: Throwable) { Log.w(TAG, "update check", e) }
        }, "update check").start()
    }

    private fun offer(n: Long, apk: String) {
        if (act.isFinishing) return
        AlertDialog.Builder(act).setTitle("Fogwood Rally build $n is out")
            .setMessage("Update now? Your records and settings stay.")
            .setPositiveButton("Update") { _, _ -> download(n, apk) }
            .setNegativeButton("Later") { _, _ -> prefs.edit().putLong("later-$n", System.currentTimeMillis()).apply() }
            .show()
    }

    private fun download(n: Long, apk: String) {
        val box = AlertDialog.Builder(act).setTitle("Downloading build $n").setMessage("0 %").setCancelable(false).show()
        Thread({
            try {
                val dir = File(act.cacheDir, "updates").apply { mkdirs(); listFiles()?.forEach { it.delete() } }
                val out = File(dir, "FogwoodRally-$n.apk")
                var u = URL(apk); var c: HttpURLConnection
                var hops = 0
                while (true) {   // GitHub redirects to its file storage
                    c = u.openConnection() as HttpURLConnection; c.instanceFollowRedirects = true; c.setRequestProperty("User-Agent", "FogwoodRally")
                    c.connectTimeout = 8000; c.readTimeout = 15000
                    val code = c.responseCode
                    if (code in 300..399 && hops++ < 5) { u = URL(u, c.getHeaderField("Location")); continue }
                    if (code != 200) throw Exception("HTTP $code"); break
                }
                val total = c.contentLengthLong; var got = 0L; var shown = -1
                c.inputStream.use { inp -> out.outputStream().use { o ->
                    val buf = ByteArray(64 * 1024)
                    while (true) { val k = inp.read(buf); if (k < 0) break; o.write(buf, 0, k); got += k
                        val pct = if (total > 0) (got * 100 / total).toInt() else -1
                        if (pct != shown) { shown = pct; main.post { box.setMessage(if (pct >= 0) "$pct %" else "${got / 1_000_000} MB") } } }
                } }
                main.post { box.dismiss(); pending = out; install() }
            } catch (e: Throwable) {
                Log.w(TAG, "update download", e)
                main.post { box.dismiss(); AlertDialog.Builder(act).setMessage("The download didn't work. Try again later.").setPositiveButton("OK", null).show() }
            }
        }, "update download").start()
    }

    /** hand the APK to the installer (first time: Android asks to allow installs from Fogwood Rally) */
    private fun install() {
        val f = pending ?: return
        if (Build.VERSION.SDK_INT >= 26 && !act.packageManager.canRequestPackageInstalls()) {
            AlertDialog.Builder(act).setTitle("One permission needed")
                .setMessage("Allow Fogwood Rally to install its updates, then come back to the game.")
                .setPositiveButton("Open settings") { _, _ -> act.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + act.packageName))) }
                .setNegativeButton("Cancel") { _, _ -> pending = null }
                .show()
            return
        }
        pending = null
        val uri = FileProvider.getUriForFile(act, act.packageName + ".updates", f)
        act.startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** back from the settings screen: carry on with the install if it is now allowed */
    fun resume() { if (pending != null && (Build.VERSION.SDK_INT < 26 || act.packageManager.canRequestPackageInstalls())) install() }

    companion object {
        private const val TAG = "Fogwood"
        private const val API = "https://api.github.com/repos/adrienBY/rally-game/releases?per_page=15"
    }
}
