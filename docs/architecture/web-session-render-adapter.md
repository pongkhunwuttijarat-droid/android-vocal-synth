# Web Session Render Adapter

**Status:** browser capture and RenderPool job adapter implemented; production Driver/UI integration pending  
**Contract:** [`CONTRACT-v1.md` §6.13](../CONTRACT-v1.md#613-render-job-model-frozen--asynchronous)

This document defines the browser adapter between a committed Rust project
session and the Web Worker renderer. It uses the frozen logical render job
semantics; worker messages and scheduling remain browser implementation details.
It does not make a frontend project model or render plan authoritative.

## Boundary

The session owner captures the job input from Rust in one synchronous operation
using [`session-render-contract.js`](../../web/app/session-render-contract.js):

```ts
type CommittedRenderSnapshot = {
  sessionId: string;
  revision: number;
  ustx: string; // immutable canonical snapshot returned by the Rust session
};

type RenderRequest = {
  expectedRevision: number;
  engine: string;
};
```

`captureRenderSnapshot(expectedRevision)` must compare the revision and return
the session ID, revision, and canonical USTX snapshot together. A revision
conflict fails before a job is enqueued. The caller must not construct `ustx`
from a UI cache, gesture preview, or a separately timed project read.

`RenderPool.setCommittedSnapshot(snapshot)` installs that captured value. The
pool then exposes the following job operations:

```ts
startRender(request: RenderRequest): RenderJobDescriptor;
getRender(renderId: string): RenderJobState;
cancelRender(renderId: string): Promise<RenderJobState>;
onRenderEvent(listener: (event: RenderJobEvent) => void): () => void;
```

The job descriptor is `{sessionId, renderId, revision, status, engine}`. Events
and terminal states always carry the same `sessionId`, `renderId`, and captured
`revision`. Progress is reported at chunk boundaries as
`doneChunks`, `totalChunks`, and `elapsedMs`. Completion carries sample rate,
frame count, engine, and audio or an artifact reference. Failure and
cancellation use the §6.14 error codes.

## Worker and cache rules

- A worker receives `{sessionId, renderId, revision, engine, ustx}` as an
  immutable job input. It never reads a project path or asks the UI for current
  project data.
- Planning is derived from that input by Rust/WASM. The browser may schedule
  chunk indices and cache rendered PCM, but it does not derive musical render
  semantics.
- Worker messages include job identity. A worker must not re-plan a shared
  mutable engine for another job while chunks from an earlier job are still
  rendering. The initial implementation may serialize jobs through a pool
  queue; parallel workers within one job use the same captured snapshot.
- Cache entries are keyed by Rust chunk hash plus render-engine identity and
  any engine configuration that changes PCM. Session ID and revision identify
  the job, but do not need to defeat reuse when the engine-provided chunk hash
  proves identical render input.
- Cancellation is per `renderId`. It stops queued work immediately and active
  work at the next chunk boundary. A cancelled job returns
  `render_cancelled`; it cannot populate cache entries with incomplete chunks.
- A completed job may remain available as an explicitly stale result. The
  scheduler never installs it as current playback automatically after a newer
  session revision has been observed. UI playback must compare session and
  revision and label any explicitly selected stale result.

## Browser publication guard

Before changing current playback or export selection, the consumer compares
the result's `(sessionId, revision)` with the current session descriptor. A
mismatch is retained only as a stale, job-addressable result; it cannot replace
current audio silently. This guard is separate from cancellation: a mutation
does not alter a running job's captured input, and it does not implicitly
cancel that job.

## Existing RenderPool mapping

`RenderPool.create/setProject` retains its legacy YAML entry point. The new
Session job path re-plans each worker from the captured Rust YAML snapshot,
attaches session/revision/render identity to that plan, serializes jobs through
a FIFO queue, streams completed chunks for tagged progress, and handles
per-job cancellation at chunk boundaries. Worker errors reject pending calls;
closing the pool cancels active/queued jobs. The existing chunk cache and
incremental sample loading remain in use.

The production Driver can adopt this boundary independently of the React mock:
it calls the capture helper and passes its immutable result to the scheduler.
UI wiring can arrive later. The Web scheduler now implements these mechanics,
but `engine-wasm` capability reporting still says `async_jobs: false` and
`per_job_cancel: false`. Keep those claims until the Session adapter is wired
through the supported Engine surface and the remaining failure/recovery and
close lifecycle checks pass.

## Browser requirements

- Requires ES modules, module Web Workers, WebAssembly SIMD, and secure
  `crypto.getRandomValues` (HTTPS or localhost). The pool uses a bounded worker
  count and loads only the samples each worker needs.
- Shared memory is not required; no `SharedArrayBuffer` or COOP/COEP headers
  are needed for the R1 worker pool.
- `r2-webgpu` additionally requires a browser/device with WebGPU and its
  runtime/model assets available. This adapter does not silently change render
  engines when that path is unavailable.
- Browser memory and worker limits vary by device. Product UI should expose
  capability failures and must not promise an exact worker count on every host.
