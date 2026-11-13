# `.rustx` project bundle format

## Status

Initial version 1 format proposal implemented by the current Web host adapter. It reuses the existing ZIP layout and media behavior. The codec is still JavaScript (`ui-mock/react-dock/src/openutau/project-bundle.js`), which is a known architecture gap; Rust codec parity is a separately scoped migration. Do not change this on-disk contract during that migration without a compatibility decision.

## Identity and container

- Extension: `.rustx`
- Manifest identity: `format: "rusted-utau-project"`
- Initial version: integer `1`
- Container: ZIP, stored (uncompressed) entries in current writer; ZIP64 unsupported.
- `.ustx` remains the OpenUtau-compatible text import/export format and is not renamed or replaced.
- Legacy pre-RUSTX project bundle identity is retired. Backward compatibility for that unreleased internal format is not required; no legacy reader or migration layer is implemented.

## Version 1 entries

```text
example.rustx (ZIP)
├── manifest.json
├── project.ustx
└── media/{asset-id}/{sanitized-file-name}
```

`project.ustx` is the canonical musical document. The manifest records bundle identity/version, project display name and USTX entry path, plus media metadata (`id`, `path`, `name`, `contentType`, `size`) and an `unresolvedMedia` count. Musical state is not duplicated in the manifest. Media paths are derived from stable asset IDs and sanitized file names. Only media available to the exporting host is embedded; missing external media leaves the project importable and is reported for relinking.

## Validation and limits in current implementation

The Web reader validates ZIP structure, entry count, total expanded size, duplicate and unexpected paths, required manifest/project entries, format/version, media IDs/paths/types/sizes, and entry CRC when reading. It does not extract archive paths to a filesystem. The bundle limit is 256 MiB, manifest limit 1 MiB, project payload limit 128 MiB, and entries are limited to 8,194. ZIP64 is unsupported. Current manifest does not contain content hashes; ZIP CRC detects accidental/corrupt entry data but is not a cryptographic asset identity check.

The writer emits stored entries and computes CRCs. The browser download flow has no filesystem atomic-rename/recovery contract. Android/server native bundle save/load is not currently implemented. Do not claim atomic save or cross-host support until those adapters exist.

## Compatibility and future work

- `.ustx` import/export must remain functional and distinct from `.rustx`.
- Legacy bundle compatibility is intentionally dropped because the format was unreleased. Existing user files must not be deleted; conversion can rewrite the manifest identity and save as `.rustx` while preserving USTX and media entries.
- Unsupported `format` or `version` is rejected; no implicit upgrade is defined.
- A separate Rust codec task must reproduce these entry names, manifest semantics, limits, and validation behavior, with cross-host fixtures before replacing the temporary Web adapter.
- Voicebank packaging, virtual instruments, and backend handoff/synchronization are out of scope.
