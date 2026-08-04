# CONTRACT-v1 — UI ↔ Engine Data Contract (Freeze)

> **สถานะ:** FREEZED 2026-08-02 — หลัง UI (lilt) + engine (synth-server) พร้อมทั้งคู่
> **สถาปัตยกรรม:** synth-server HTTP = **DEV-ONLY tool** (LAN test, golden compare, UI dev)
> **Production path = engine ฝังใน app ผ่าน JNI** (.so + Kotlin bridge) — UI ต้องไม่ผูกกับ HTTP
> อ้างอิง: `docs/architecture/data-contracts.md` (engine internals), `native/tools/synth-server/`

---

## 0. Transport Strategy (สำคัญ)

| โหมด | Transport | ใช้เมื่อ | สถานะ |
|---|---|---|---|
| **Dev** | HTTP synth-server (`http://<host>:18080`) | desktop dev, LAN test บน Mi Pad, golden | ✅ พร้อม |
| **Production** | JNI embedded (libvoicesynth.so + Kotlin bridge) | แอปจริงบน Android | ⬜ MS3+ |

**กฎ UI:** โค้ด UI ทั้งหมดต้องคุยกับ `EngineClient` interface เท่านั้น — transport (HTTP/JNI) swap ได้โดยไม่แตะ UI
- v1 ใช้ HttpEngineClient (dev) — ฝั่ง JNI มาเมื่อไหร่ก็แค่เพิ่ม JniEngineClient
- Settings มี server URL ก็เพื่อ dev mode เท่านั้น

---

## 1. Transport

| Item | Value |
|---|---|
| Protocol | HTTP/1.1 JSON (wav = raw bytes) |
| Base URL | `http://<host>:18080` (configurable ใน Settings) |
| Timeout | 10s (health), 60s (render) |
| Auth | ไม่มีใน v1 (LAN only) |
| Error | `{"error": "..."}` + HTTP status (400/404/500) |

## 2. Endpoints (v1 — engine side เสร็จแล้ว)

| Method | Path | Request | Response |
|---|---|---|---|
| GET | `/health` | — | `{status:"ok", version, so_loaded}` |
| GET | `/capabilities` | — | `{modes[], expressions[], needs_oto, needs_wav_samples, needs_frq, channels, sample_rate}` |
| GET | `/voicebanks` | — | `{voicebanks:[{dir, name, aliases_count, wav_count, samples_rate}]}` |
| POST | `/synth-note` | `{voicebank, phoneme, tone, duration_ms}` | **audio/wav** bytes (หรือ `{error}`) |
| POST | `/render` | `{project, voicebank}` | **audio/wav** bytes (หรือ `{error}`) |
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

1. **ทุก screen รับ `EngineClient` interface** (inject ผ่าน constructor, default = HttpEngineClient) — ห้าม import http/network ใน screen โดยตรง
2. **Voicebanks screen** โหลดจาก `/voicebanks` จริง — **fallback เป็น mock list ถ้า server offline** (UI options ห้ามหาย)
3. **Editor export** → `POST /render` → ได้ wav bytes → แสดงสถานะ + ขนาด + duration (v1: ไม่เล่นเสียงใน app — user ฟังผ่านไฟล์)
4. **Settings** มี field server URL + ปุ่ม "Test connection" (GET /health) — **ติด label "Dev mode"** ให้ชัดว่าไม่ใช่ production
5. **โน้ตใน editor ยังเป็น local state** (ยังไม่ load/save .ustx ผ่าน server ใน v1 — render ใช้ project path บนเครื่อง server)
6. ทุก network call ต้องมี error state ที่ user เห็น (SnackBar/status text) — ห้าม crash

## 5. Out of Scope (v1)

- จังหวะ piano roll ↔ server (ยัง local)
- .ustx load/save ผ่าน UI
- Audio playback ใน app
- JNI direct call (v2 — ตอน Android app ฝัง engine)

---
*Breaking changes ต้อง bump เป็น CONTRACT-v2 + แจ้งทั้งสองฝั่ง*
