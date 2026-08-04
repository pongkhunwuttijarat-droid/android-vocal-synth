# Test Data (fixtures)

Mock input/output data สำหรับ test engine — ใช้โดย feed pipeline, plugin tests, golden tests

## โครงสร้าง

```
native/test-data/
├── mock-voicebank/           ← voicebank เล็ก (subset จาก Teto English CVVC)
│   ├── character.txt         ← metadata
│   └── voice/
│       ├── oto.ini           ← 19 entries (3 aliases)
│       └── 3 × .wav          ← samples จริง
├── mock-song.ustx            ← project เล็ก 4 notes (120bpm 4/4)
└── render-input.example.json ← ตัวอย่าง RenderInput (superset schema)
```

## วิธีใช้

- **mock-voicebank** — test voicebank loader, oto mapper, feed pipeline (เร็ว ไม่ต้องโหลด 558 wav)
- **mock-song.ustx** — test domain load/save + command system + full render
- **render-input.example.json** — schema reference สำหรับ feed builder + plugin mock tests

## หมายเหตุ

- Golden reference wav (expected output) ยังว่าง — ต้องสร้างจาก OpenUtau desktop (P.4) หรือ
  เมื่อ engine เรารันได้แล้วล็อก output เป็น baseline
- Mock voicebank สร้างจาก Teto English CVVC (test/golden/teto-english) — เหลือแค่ 3 aliases
  ที่ feed/plugin test ใช้: `r`, `iy`, `a` (ในรูป CVVC alias)
