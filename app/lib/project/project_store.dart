import 'project_doc.dart';

/// Storage abstraction for Rusted UTAU projects.
///
/// ARCHITECTURE: Dart is the file-access layer ONLY. [save] serializes
/// through the Rust .ustx codec (plan Task 2 — bytes via FFI) before
/// writing; [load] reads bytes then decodes through the same codec.
/// Until the codec lands, implementations that need bytes are free to
/// use a JSON interim format — the interface is the contract, the wire
/// format is the codec's business.
abstract class ProjectStore {
  /// All saved projects, newest first.
  Future<List<ProjectMeta>> list();

  /// Persist [doc], returning the path it was written to. Saving a
  /// project with the same name overwrites (upsert).
  Future<String> save(ProjectDoc doc);

  /// Load the project at [path]; null when it no longer exists.
  Future<ProjectDoc?> load(String path);

  /// Delete the project at [path].
  Future<void> delete(String path);
}

/// In-memory implementation — used by tests and as the POC default.
/// Deterministic paths: `memory://<name>`.
class MemoryProjectStore implements ProjectStore {
  final Map<String, ProjectDoc> _docs = {};

  @override
  Future<List<ProjectMeta>> list() async {
    final metas = _docs.entries
        .map((e) => ProjectMeta(name: e.value.name, path: e.key))
        .toList();
    // Newest first — insertion order approximates recency for the POC.
    return metas.reversed.toList();
  }

  @override
  Future<String> save(ProjectDoc doc) async {
    final path = 'memory://${doc.name}';
    _docs[path] = doc;
    return path;
  }

  @override
  Future<ProjectDoc?> load(String path) async => _docs[path];

  @override
  Future<void> delete(String path) async {
    _docs.remove(path);
  }
}
