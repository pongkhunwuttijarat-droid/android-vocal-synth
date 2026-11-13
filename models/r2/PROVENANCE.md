# models/r2 — provenance (T5)

โมเดล R2 ตัวจริงสำหรับ POC "R2 chain บน wasm" — **ไม่ commit** (`.gitignore`: `models/r2/`)
ทุกไฟล์ดาวน์โหลด/คัดลอกซ้ำได้จาก URL ด้านล่าง และตรวจ sha256 ได้ตามตาราง

## 1. vocoder: OpenUtau package `pc-nsf-hifigan`

| อย่าง | ค่า |
|---|---|
| registry | https://openutau.github.io/svs-index/registry/v1/softwares/all.json |
| registry item id | `pc_nsf_hifigan_44.1k_hop512_128bin_2025.02` (category `vocoder`, Team OpenVPI) |
| download URL | https://github.com/openvpi/vocoders/releases/download/pc-nsf-hifigan-44.1k-hop512-128bin-2025.02/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep |
| ไฟล์ที่โหลด | `models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep` (52,760,610 B) |
| sha256 (ตรงกับ `hash` ใน registry) | `ba7d43142d41f6900c8264b5662ca7125a50feb8760bb8b9615c61a8f5e6902e` |
| แตกไฟล์ไปที่ | `models/r2/pc-nsf-hifigan/` (zip: vocoder.yaml, oudep.yaml, NOTICE*, STATEMENTS.txt, *.onnx) |
| ONNX ข้างใน | `pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx` (56,616,643 B) |
| sha256 ของ ONNX | `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` |
| หมายเหตุสำคัญ | **ไฟล์ ONNX นี้ byte-identical กับ vocoder ที่ T1 ใช้** (`test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx`) — ต่างกันที่ "mel" ที่ป้อนเข้า ไม่ใช่ตัว vocoder |

`vocoder.yaml` ที่มากับ package (สเปกจริง):

```yaml
name: "pc_nsf_hifigan_44.1k_hop512_128bin_2025.02"
model: "pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx"
sample_rate: 44100
hop_size: 512
win_size: 2048
fft_size: 2048
num_mel_bins: 128
mel_fmin: 40
mel_fmax: 16000
mel_base: e
mel_scale: slaney
pitch_controllable: true
force_on_cpu: false
```

⇒ `sp_size = fft_size/2 + 1 = 1025` ตรงกับ input shape ของ mel.onnx

## 2. mel.onnx (ตัวที่ WorldlineRenderer.cs โหลด)

| อย่าง | ค่า |
|---|---|
| source ในเครื่อง | `ref(openutau+openutau mobile)/core/OpenUtau.Core/Classic/Data/mel.onnx` |
| ปลายทาง | `models/r2/mel.onnx` |
| size | 85,404 B |
| sha256 | `1493ffc868c6fb8b66104e2c6a2902cbf1a8c02bb09215db014f02e9d0e82f94` |
| อ้างอิงโค้ด | `.../Classic/WorldlineRenderer.cs:163-178` (`OpenUtau.Core.Classic.Data.Resources.mel` แล้ว fallback ไฟล์บนดิสก์) |

## 3. วิธีสร้างใหม่ (ทั้งชุด)

```bash
mkdir -p models/r2
url='https://github.com/openvpi/vocoders/releases/download/pc-nsf-hifigan-44.1k-hop512-128bin-2025.02/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep'
curl -fL "$url" -o models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep
echo 'ba7d43142d41f6900c8264b5662ca7125a50feb8760bb8b9615c61a8f5e6902e  models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep' | sha256sum -c
python3 -c "import zipfile; zipfile.ZipFile('models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep').extractall('models/r2/pc-nsf-hifigan')"
cp 'ref(openutau+openutau mobile)/core/OpenUtau.Core/Classic/Data/mel.onnx' models/r2/mel.onnx
```

## 4. ไฟล์ที่ POC สร้าง (ไม่ commit)

| ไฟล์ | อะไร |
|---|---|
| `/tmp/r2-result.json` | ตัวเลขทั้งหมดจากหน้าเว็บ |
| `/tmp/r2-wasm.wav` | WAV จากเส้นทาง wasm (browser) |
| `/tmp/r2-native.wav` | WAV จากเส้นทาง native CPU EP (onnxruntime-node) สำหรับ parity |
| `/tmp/r2-run.txt`, `/tmp/r2-rss.txt` | log + peak RSS |
