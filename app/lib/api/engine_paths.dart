/// Device-side engine paths (Android).
///
/// The bundled synth-server runs inside the app (EngineProcess.kt) and
/// extracts its assets to `filesDir/engine/`. This channel exposes those
/// paths so the UI can build render requests that point at real files on
/// this device (CONTRACT-v1 §4.5: v1 renders a fixed demo project).
///
/// On non-Android (dev desktop) the channel is absent — callers must
/// fall back to the host-side default paths.
library;

import 'package:flutter/services.dart';

class EnginePaths {
  const EnginePaths({
    required this.dir,
    required this.demoProject,
    required this.voicebanksDir,
  });

  final String dir;
  final String demoProject;
  final String voicebanksDir;

  static const MethodChannel _channel = MethodChannel('com.lilt.lilt/engine');

  /// Query the platform channel; null when not running on Android
  /// (desktop dev) or the channel isn't available yet.
  static Future<EnginePaths?> tryResolve() async {
    try {
      final raw = await _channel.invokeMethod<Map<dynamic, dynamic>>('engineDir');
      if (raw == null) return null;
      return EnginePaths(
        dir: raw['dir'] as String? ?? '',
        demoProject: raw['project'] as String? ?? '',
        voicebanksDir: raw['voicebanks'] as String? ?? '',
      );
    } on MissingPluginException {
      return null; // desktop / tests
    } on PlatformException {
      return null;
    }
  }

  /// Save rendered wav bytes to public `Downloads/Lilt/<fileName>` via
  /// MediaStore (Android 10+: no runtime permission needed for writing your
  /// own file to Downloads — that's why no permission dialog appears).
  ///
  /// Returns the content:// URI, or null when the platform channel is
  /// unavailable (desktop/tests) or the save failed.
  static Future<String?> saveWav(Uint8List bytes, String fileName) async {
    try {
      return await _channel.invokeMethod<String>('saveWav', {
        'bytes': bytes,
        'fileName': fileName,
      });
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }

  /// Persist a .ustx the editor built from its notes into the engine dir
  /// (`filesDir/engine/`) so the in-app server can render it by path.
  ///
  /// Returns the absolute path the server can read, or null off-Android.
  static Future<String?> writeProject(String content, {String fileName = 'editor-song.ustx'}) async {
    try {
      return await _channel.invokeMethod<String>('writeProject', {
        'fileName': fileName,
        'content': content,
      });
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }
}
