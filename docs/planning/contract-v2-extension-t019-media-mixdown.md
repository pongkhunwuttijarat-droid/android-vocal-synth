# T019 Project Session v2 add-on proposal: media and mixdown

**Status:** implementation draft; not ratified. `docs/CONTRACT-v1.md` remains frozen.

This proposal records the draft command and route shapes used by the Web
OpenUtau clone. It does not claim that these operations are part of §6 or that
their schemas are stable for other clients.

## Commands

| Operation | Draft payload | Engine-owned effect |
| --- | --- | --- |
| `track.create` | `name`, optional `to_index` | Append or insert an empty track. |
| `track.remove` | `track_id` | Remove a track and its parts; refuse removal of the final track. |
| `track.reorder` | `track_id`, `to_index` | Reorder by stable id and remap dense serialized track numbers. |
| `project.import_tracks` | `ustx_yaml` | Parse with the Rust USTX loader and append incoming tracks/parts as one project change. |
| `part.import_audio` | `track_id`, `name`, `position_ticks`, `media_ref`, `duration_ms` | Create an engine-owned wave part whose duration is converted through the project time axis. |

All commands use the normal v2 session command envelope, revision checks,
snapshot invalidation, and undo semantics. `project.import_tracks` is limited by
the command body size. Track import currently accepts USTX; referenced external
audio must already be available to the host media resolver.

## Host media registration

The browser uploads raw audio bytes with `PUT /api/v2/media/{asset_id}` and
receives a validated `media_ref` plus decoded duration. The Direct WASM host
registers the same bytes with `Engine.session_register_media`. Browser media
bytes live in OPFS separately from portable USTX, keyed by the opaque asset id;
project open re-registers them with the active Rust session. The server's
uploaded-media resolver currently retains uploads for the server process
lifetime, so durable server-side media storage remains a host integration gap.

## Mixdown route

`POST /api/v2/sessions/{id}/mixdown` takes `expected_revision`, a voicebank
name, and render engine. It snapshots one session revision and returns a render
job id; clients use the ordinary render status and audio artifact endpoints.
Direct WASM calls the same shared Rust mixdown pipeline synchronously. Both
paths produce interleaved stereo from Rust; the frontend only decodes/downloads
the artifact.

## Ratification still needed

- Confirm stable names, limits, error codes, media retention, and auth policy.
- Decide whether media registration belongs in the shared session API or a
  host resource API, and specify lifecycle/cleanup behavior.
- Define cross-host conformance vectors for import, track reorder, media render,
  and revision-pinned mixdown before treating this draft as public API.
