package com.lilt.lilt

import android.content.ContentValues
import android.os.Bundle
import android.os.Environment
import android.provider.MediaStore
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Dev-mode: host the synth-server inside the app (CONTRACT-v1 §0).
        // Production (MS3) replaces this with JNI embedded calls.
        EngineProcess.start(applicationContext)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            "com.lilt.lilt/engine",
        ).setMethodCallHandler { call, result ->
            when (call.method) {
                "engineDir" -> {
                    val dir = File(filesDir, "engine")
                    result.success(mapOf(
                        "dir" to dir.absolutePath,
                        "project" to File(dir, "demo-song.ustx").absolutePath,
                        "voicebanks" to File(dir, "voicebanks").absolutePath,
                    ))
                }
                "engineRunning" -> {
                    result.success(EngineProcess.isRunning())
                }
                "writeProject" -> {
                    // Editor → engine: persist the .ustx the UI built from
                    // its notes into filesDir/engine/ (same dir the server
                    // reads demo-song.ustx from), so POST /render can find
                    // it by absolute path.
                    try {
                        val name = call.argument<String>("fileName") ?: "editor-song.ustx"
                        val content = call.argument<String>("content") ?: ""
                        val dir = File(filesDir, "engine")
                        dir.mkdirs()
                        val f = File(dir, name)
                        f.writeText(content)
                        result.success(f.absolutePath)
                    } catch (e: Exception) {
                        result.error("write_failed", e.message, null)
                    }
                }
                "saveWav" -> {
                    // MediaStore. Android 10+ (scoped storage): writing your
                    // own file to Downloads needs NO runtime permission —
                    // that's why no permission dialog appears.
                    try {
                        val bytes = call.argument<ByteArray>("bytes")
                        val name = call.argument<String>("fileName") ?: "lilt-export.wav"
                        if (bytes == null || bytes.isEmpty()) {
                            result.error("empty", "no wav bytes", null)
                            return@setMethodCallHandler
                        }
                        val values = ContentValues().apply {
                            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                            put(MediaStore.MediaColumns.MIME_TYPE, "audio/wav")
                            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Lilt")
                            put(MediaStore.MediaColumns.IS_PENDING, 1)
                        }
                        val uri = contentResolver.insert(
                            MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                        if (uri == null) {
                            result.error("insert_failed", "MediaStore insert returned null", null)
                            return@setMethodCallHandler
                        }
                        contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                        values.clear()
                        values.put(MediaStore.MediaColumns.IS_PENDING, 0)
                        contentResolver.update(uri, values, null, null)
                        result.success(uri.toString())
                    } catch (e: Exception) {
                        result.error("save_failed", e.message, null)
                    }
                }
                else -> result.notImplemented()
            }
        }
    }

    override fun onDestroy() {
        EngineProcess.stop()
        super.onDestroy()
    }
}
