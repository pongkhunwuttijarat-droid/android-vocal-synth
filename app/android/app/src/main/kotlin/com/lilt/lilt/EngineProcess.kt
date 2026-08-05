package com.lilt.lilt

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileOutputStream

/**
 * Spawns the bundled synth-server (Rust, aarch64) as a child process and
 * keeps it alive for the app's lifetime.
 *
 * Layout inside the APK:
 *   assets/engine/synth-server          — Rust server binary (arm64)
 *   assets/engine/libworldline.so       — renderer .so (dlopened by server)
 *
 * On first launch the assets are extracted to filesDir/engine/, made
 * executable, and the server is started with
 * `--so <dir>/libworldline.so --voicebanks <dir>/voicebanks --port 18080
 *  --bind 127.0.0.1`.
 *
 * The Flutter UI talks to http://127.0.0.1:18080 via EngineClient
 * (CONTRACT-v1 §0, dev transport) — no changes needed on the Dart side.
 *
 * Dev-mode only: production (MS3) replaces this with JNI embedded calls,
 * and the EngineClient interface swaps transport without touching UI.
 */
object EngineProcess {
    private const val TAG = "LiltEngine"
    private const val PORT = 18080

    @Volatile
    private var process: Process? = null

    /** Extract bundled engine assets into [dir] (idempotent). */
    private fun extractAssets(context: Context, dir: File) {
        val assets = context.assets
        for (name in listOf("engine/demo-song.ustx")) {
            val target = File(dir, name.removePrefix("engine/"))
            if (target.exists() && target.length() > 0) {
                continue // already extracted
            }
            target.parentFile?.mkdirs()
            assets.open(name).use { input ->
                FileOutputStream(target).use { output -> input.copyTo(output) }
            }
            Log.i(TAG, "extracted $name -> ${target.absolutePath} (${target.length()} bytes)")
        }
    }

    /** Start the bundled server if it is not already running. */
    fun start(context: Context) {
        synchronized(this) {
            if (process?.isAlive == true) return

            // The Rust binary + renderer .so live in jniLibs/arm64-v8a and
            // are extracted to nativeLibraryDir — the ONE app-owned dir
            // whose SELinux label (apk_data_file) still allows exec().
            // filesDir / getDir("bin") are app_data_file on Android 16 and
            // exec there fails with EACCES (error=13).
            val engineDir = File(context.applicationInfo.nativeLibraryDir)
            // AGP only ships *.so files from jniLibs — the Rust server binary
            // is named libsynthserver.so (exec works fine regardless of the
            // extension; it's a static-PIE ELF, not a shared library).
            val serverBin = File(engineDir, "libsynthserver.so")
            if (!serverBin.canExecute()) {
                Log.e(TAG, "synth-server not executable in nativeLibraryDir: $engineDir")
                return
            }
            Log.i(TAG, "engine dir: $engineDir (nativeLibraryDir)")

            // Voicebanks: assets under assets/engine/voicebanks are NOT in
            // jniLibs — extract them to an app dir (read-only usage, no exec
            // needed, so app_data_file is fine).
            val engineDataDir = File(context.filesDir, "engine")
            extractAssets(context, engineDataDir)
            val voicebanksDir = File(engineDataDir, "voicebanks")
            extractVoicebanks(context, voicebanksDir)

            val cmd = listOf(
                serverBin.absolutePath,
                "--so", File(engineDir, "libworldline.so").absolutePath,
                // Mixer FX plugin (libmixerfx.so, bundled with jniLibs):
                // EQ/comp/softclip applied to the final mix. Params start
                // at defaults (passthrough: clip disabled → 0 dB EQ).
                "--mixer-so", File(engineDir, "libmixerfx.so").absolutePath,
                "--mixer-params", """{"clip_enabled":0,"eq_enabled":1}""",
                "--voicebanks", voicebanksDir.absolutePath,
                "--port", PORT.toString(),
                "--bind", "127.0.0.1",
            )
            Log.i(TAG, "starting engine: $cmd")

            try {
                val pb = ProcessBuilder(cmd)
                pb.redirectErrorStream(true)
                process = pb.start()
                // Drain stdout so the pipe never fills up and blocks the server.
                Thread {
                    try {
                        process?.inputStream?.bufferedReader()?.forEachLine { line ->
                            Log.i(TAG, "[server] $line")
                        }
                    } catch (e: java.io.IOException) {
                        // Server exited (port conflict / force-stop / crash):
                        // the stream closes and the drain thread must not
                        // take the app down with an uncaught exception.
                        Log.d(TAG, "engine output stream closed: $e")
                    }
                }.start()
                Log.i(TAG, "engine started (pid ${process?.hashCode()})")
            } catch (e: Exception) {
                Log.e(TAG, "failed to start engine", e)
            }
        }
    }

    /** Stop the server (called when the app process is going away). */
    fun stop() {
        synchronized(this) {
            process?.destroy()
            process = null
        }
    }

    /** Extract bundled voicebank assets into [dir] (idempotent). */
    private fun extractVoicebanks(context: Context, dir: File) {
        val assets = context.assets
        // The whole assets/engine/voicebanks tree is copied recursively.
        val root = "engine/voicebanks"
        val names = assets.list(root) ?: return
        fun copyTree(prefix: String) {
            val entries = assets.list(prefix) ?: return
            for (name in entries) {
                val child = "$prefix/$name"
                val target = File(dir, child.removePrefix(root + "/"))
                // AssetManager.list() does NOT append "/" to directory
                // entries (API 26+: plain names for both) — the only
                // reliable dir test is: opening fails for directories.
                val isDir = try {
                    assets.open(child).use { false }
                } catch (e: java.io.IOException) {
                    true
                }
                if (isDir) {
                    target.mkdirs()
                    copyTree(child)
                } else {
                    if (target.exists() && target.length() > 0) continue
                    target.parentFile?.mkdirs()
                    assets.open(child).use { input ->
                        FileOutputStream(target).use { output -> input.copyTo(output) }
                    }
                }
            }
        }
        copyTree(root)
        Log.i(TAG, "voicebanks extracted -> ${dir.absolutePath}")
    }

    /** Whether the bundled server process is currently alive. */
    fun isRunning(): Boolean = synchronized(this) { process?.isAlive == true }
}
