# CONTRACT-v1 — UI ↔ Engine Data Contract (Freeze)

> **สถานะ:** Frozen 2026-08-02 · implementation update 2026-09-12
> **สถาปัตยกรรมปัจจุบัน:** Flutter/Android ใช้ bundled `synth-server` ผ่าน loopback HTTP;
> browser ใช้ `engine-wasm` in-process. JNI embedded ยังเป็นทางเลือกถัดไป ไม่ใช่ path
> ที่แอปใช้อยู่ตอนนี้.
> อ้างอิง: `docs/architecture/data-contracts.md` (engine internals), `native/tools/synth-server/`

---

## 0. Transport Strategy (สำคัญ)

| โหมด | Transport | ใช้เมื่อ | สถานะ |
|---|---|---|---|
| **Dev** | HTTP synth-server (`http://<host>:18080`) | desktop dev, LAN test บน Mi Pad, golden | ✅ พร้อม |
| **Android app (current)** | bundled synth-server + `http://127.0.0.1:18080` | แอปจริงบน Android | ✅ |
| **Web (current)** | `engine-wasm` + JS/Canvas | static browser editor | 🟡 vertical slice |
| **Future native** | JNI embedded (libvoicesynth.so + Kotlin bridge) | ลด process/HTTP overhead | ⬜ |

**กฎ UI:** โค้ด UI ทั้งหมดต้องคุยกับ `EngineClient` interface เท่านั้น — transport (HTTP/JNI) swap ได้โดยไม่แตะ UI
- Flutter v1 ใช้ `ApiClient`/HTTP ทั้งบน desktop และ Android; Android จะสตาร์ต
  bundled server ใน `EngineProcess.kt` และ bind loopback.
- Settings server URL ยังมีไว้สำหรับ dev/LAN override; ไม่ได้แปลว่า app ต้องมี
  external server เสมอ.
- Web ไม่ใช้ transport นี้: ใช้ wasm-bindgen adapter ใน process เดียว.

---

## 1. Transport

| Item | Value |
|---|---|
| Protocol | HTTP/1.1 JSON (wav = raw bytes) |
| Base URL | `http://<host>:18080` (configurable ใน Settings) |
| Timeout | 10s (health), 300s (render; client default) |
| Auth | ไม่มีใน v1 (LAN only) |
| Error | `{"error": "..."}` + HTTP status (400/404/500) |

## 2. Endpoints (v1 — engine side เสร็จแล้ว)

| Method | Path | Request | Response |
|---|---|---|---|
| GET | `/health` | — | `{status:"ok", version, so_loaded}` |
| GET | `/capabilities` | — | `{modes[], expressions[], needs_oto, needs_wav_samples, needs_frq, channels, sample_rate}` |
| GET | `/voicebanks` | — | `{voicebanks:[{dir, name, aliases_count, wav_count, samples_rate}]}` |
| POST | `/synth-note` | `{voicebank, phoneme, tone, duration_ms}` | **audio/wav** bytes (หรือ `{error}`) |
| POST | `/render` | `{project, voicebank, engine?}` | **audio/wav** bytes (หรือ `{error}`) |
| GET | `/stats` | — | `{renders_count, total_ms, cache_hits}` |

**ตัวอย่าง /render request:**
```json
{"project": "/abs/path/song.ustx", "voicebank": "library"}
```

## 3. UI View Models (Dart — `app/lib/models.dart`)

| Dart model | Source | Fields |
|---|---|---|
| `Note` | local (editor state) | lyric, pitch, position (beats), duration (beats), phoneme |
| `Track` | local | name, colorSeed, notes[] |
| `Voicebank` | **GET /voicebanks** | name←dir, format, status="ready", sizeMb←wav_count |
| `PitchPoint` | local | beat, semitones |

## 4. Wire Rules (UI side)

1. **ทุก screen รับ `EngineClient` interface** (inject ผ่าน constructor, default = `ApiClient`) — ห้าม import http/network ใน screen โดยตรง
2. **Voicebanks screen** โหลดจาก `/voicebanks` จริง — **fallback เป็น mock list ถ้า server offline** (UI options ห้ามหาย)
3. **Editor render/play/export** → `POST /render` → wav bytes → แสดงสถานะ + ขนาด + duration
   และเล่นผ่าน `audioplayers`; server รองรับ cancel/post-FX ด้วย
4. **Settings** มี field server URL + ปุ่ม "Test connection" (GET /health) — **ติด label "Dev mode"** ให้ชัดว่าไม่ใช่ production
5. **Project state** อยู่ใน editor และ save/load ผ่าน server-side `.ustx` codec/store;
   export/import เต็มรูปแบบยังขึ้นกับ host (Android SAF และ Web OPFS เป็นงาน product)
6. ทุก network call ต้องมี error state ที่ user เห็น (SnackBar/status text) — ห้าม crash

## 5. Out of Scope / follow-up

- จังหวะ piano roll ↔ server (ยัง local)
- per-track renderer selection ใน Flutter UI
- Android JNI direct call (ลด child-process/HTTP overhead)
- Web OPFS project browser, multi-track mix, expression lanes และ PWA
- neural renderer บน WASM

---
*Breaking changes ต้อง bump เป็น CONTRACT-v2 + แจ้งทั้งสองฝั่ง*
