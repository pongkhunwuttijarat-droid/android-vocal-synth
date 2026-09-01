import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

/// FFI wrapper for the Rust `.ustx` codec plugin (`libustxcodec.so`).
///
/// ARCHITECTURE: the .ustx wire format lives in Rust — Dart only passes
/// bytes across the FFI boundary. [decode] parses a `.ustx` file into a
/// compact JSON view; [encode] builds `.ustx` bytes from that view.
///
/// Plugin contract (mirror of mixer-fx's dlopen pattern):
///   UstxDecode(ustx_yaml: *const c_char) -> *mut c_char  (JSON, free w/ UstxFree)
///   UstxEncode(json: *const c_char)     -> *mut c_char  (YAML, free w/ UstxFree)
///   UstxFree(p: *mut c_char)
class UstxCodec {
  UstxCodec._(this._lib);

  final DynamicLibrary _lib;

  static UstxCodec? _instance;

  /// Load the codec plugin. Android ships it in jniLibs next to
  /// libsynthserver.so (resolvable via `DynamicLibrary.open` by name);
  /// desktop builds dlopen from the native build dir.
  static Future<UstxCodec> load({String? soDir}) async {
    if (_instance != null) return _instance!;
    final lib = _open(soDir);
    _instance = UstxCodec._(lib);
    return _instance!;
  }

  static DynamicLibrary _open(String? soDir) {
    if (soDir != null) {
      return DynamicLibrary.open('$soDir/libustx_codec.so');
    }
    try {
      return DynamicLibrary.open('libustx_codec.so');
    } on ArgumentError {
      // Desktop dev fallback: the crate's cdylib output.
      return DynamicLibrary.open(
        'native/target/debug/libustx_codec.so',
      );
    }
  }

  late final Pointer<Utf8> Function(Pointer<Utf8>) _decode = _lib
      .lookup<NativeFunction<Pointer<Utf8> Function(Pointer<Utf8>)>>('UstxDecode')
      .asFunction();
  late final Pointer<Utf8> Function(Pointer<Utf8>) _encode = _lib
      .lookup<NativeFunction<Pointer<Utf8> Function(Pointer<Utf8>)>>('UstxEncode')
      .asFunction();
  late final void Function(Pointer<Utf8>) _free = _lib
      .lookup<NativeFunction<Void Function(Pointer<Utf8>)>>('UstxFree')
      .asFunction();

  /// Parse `.ustx` bytes → JSON view. Throws [FormatException] when the
  /// plugin rejects the input.
  Map<String, dynamic> decode(List<int> ustxBytes) {
    final cIn = utf8.decode(ustxBytes).toNativeUtf8();
    try {
      final cOut = _decode(cIn);
      if (cOut == nullptr) {
        throw const FormatException('ustx-codec: decode failed');
      }
      final json = cOut.toDartString();
      _free(cOut);
      return jsonDecode(json) as Map<String, dynamic>;
    } finally {
      malloc.free(cIn);
    }
  }

  /// Encode the JSON view → `.ustx` bytes.
  List<int> encode(Map<String, dynamic> json) {
    final cIn = jsonEncode(json).toNativeUtf8();
    try {
      final cOut = _encode(cIn);
      if (cOut == nullptr) {
        throw const FormatException('ustx-codec: encode failed');
      }
      final yaml = cOut.toDartString();
      _free(cOut);
      return utf8.encode(yaml);
    } finally {
      malloc.free(cIn);
    }
  }
}
