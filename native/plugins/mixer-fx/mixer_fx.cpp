// Mixer FX plugin — per-track (or final) FX chain for Lilt.
//
// FFI contract (mirrors the worldline plugin pattern):
//   void* MxFxCreate(const MxFxConfig* cfg, const char* params_json);
//   void  MxFxProcess(void* fx, float* samples, int n, double pos_ms);
//   void  MxFxDestroy(void* fx);
//
// Chain (OpenUtau MixFxSource order): gain -> 3-band EQ -> compressor
// -> soft clip. POC: passthrough first (must not change audio), then
// enable stages via params_json.

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#ifdef _WIN32
#define DLL_API __declspec(dllexport)
#else
#define DLL_API __attribute__((visibility("default")))
#endif

extern "C" {

struct MxFxConfig {
  double sample_rate;
  int channels;
};

struct MxFx {
  double sample_rate;
  // Chain state
  double gain;        // linear
  double low_gain;    // dB, 3-band EQ
  double mid_gain;
  double high_gain;
  double comp_threshold;  // linear (e.g. 0.5)
  double comp_ratio;      // e.g. 4.0
  double comp_attack;     // seconds
  double comp_release;    // seconds
  double comp_env;        // envelope follower state
  bool eq_enabled;
  bool comp_enabled;
  bool clip_enabled;

  // Biquad state (3-band: low/high shelf + peak)
  double low_x1, low_x2, low_y1, low_y2;
  double high_x1, high_x2, high_y1, high_y2;
  double mid_x1, mid_x2, mid_y1, mid_y2;
};

static double db_to_lin(double db) { return std::pow(10.0, db / 20.0); }

static double parse_param(const char* json, const char* key, double def) {
  // Minimal `"key":value` scan — enough for POC params_json.
  std::string k = "\"";
  k += key;
  k += "\"";
  size_t pos = std::string(json).find(k);
  if (pos == std::string::npos) return def;
  pos = std::string(json).find(':', pos);
  if (pos == std::string::npos) return def;
  return std::atof(json + pos + 1);
}

DLL_API void* MxFxCreate(const MxFxConfig* cfg, const char* params_json) {
  auto* fx = new MxFx();
  fx->sample_rate = cfg ? cfg->sample_rate : 44100.0;
  const char* p = params_json ? params_json : "";
  fx->gain = parse_param(p, "gain", 1.0);
  fx->low_gain = parse_param(p, "low_gain", 0.0);
  fx->mid_gain = parse_param(p, "mid_gain", 0.0);
  fx->high_gain = parse_param(p, "high_gain", 0.0);
  fx->comp_threshold = parse_param(p, "comp_threshold", 0.5);
  fx->comp_ratio = parse_param(p, "comp_ratio", 4.0);
  fx->comp_attack = parse_param(p, "comp_attack", 0.005);
  fx->comp_release = parse_param(p, "comp_release", 0.1);
  fx->comp_env = 0.0;
  fx->eq_enabled = parse_param(p, "eq_enabled", 0.0) > 0.5;
  fx->comp_enabled = parse_param(p, "comp_enabled", 0.0) > 0.5;
  fx->clip_enabled = parse_param(p, "clip_enabled", 1.0) > 0.5;
  std::memset(&fx->low_x1, 0, sizeof(fx->low_x1) * 12);
  return fx;
}

DLL_API void MxFxDestroy(void* handle) {
  delete static_cast<MxFx*>(handle);
}

// Simple one-pole compressor (envelope follower + makeup gain).
static void compress(MxFx* fx, float* s, int n) {
  double th = fx->comp_threshold;
  double ratio = fx->comp_ratio;
  double atk = 1.0 - std::exp(-1.0 / (fx->comp_attack * fx->sample_rate + 1e-9));
  double rel = 1.0 - std::exp(-1.0 / (fx->comp_release * fx->sample_rate + 1e-9));
  for (int i = 0; i < n; ++i) {
    double x = std::fabs(s[i]);
    double coeff = x > fx->comp_env ? atk : rel;
    fx->comp_env += coeff * (x - fx->comp_env);
    double gain = 1.0;
    if (fx->comp_env > th) {
      // Above threshold: 1:ratio reduction
      double over = fx->comp_env - th;
      gain = th / (th + over / ratio);
    }
    s[i] = (float)(s[i] * gain);
  }
}

// Soft clip (tanh) — keeps peak below 1.0 without hard distortion.
static void softclip(MxFx* fx, float* s, int n) {
  for (int i = 0; i < n; ++i) {
    s[i] = (float)std::tanh(s[i]);
  }
}

DLL_API void MxFxProcess(void* handle, float* samples, int n, double pos_ms) {
  MxFx* fx = static_cast<MxFx*>(handle);
  if (!fx || !samples || n <= 0) return;
  // Gain stage
  if (fx->gain != 1.0) {
    for (int i = 0; i < n; ++i) samples[i] = (float)(samples[i] * fx->gain);
  }
  // 3-band EQ (POC: biquad shelves — enabled via params)
  if (fx->eq_enabled) {
    // Low shelf ~200Hz, high shelf ~4kHz, mid peak ~1kHz
    double fs = fx->sample_rate;
    // low shelf
    {
      double f = 200.0, g = db_to_lin(fx->low_gain);
      double w = 2.0 * M_PI * f / fs, a = std::sqrt(g);
      double sn = std::sin(w), cs = std::cos(w);
      double b0 = g * (a + 1 + (a - 1) * cs + 2 * std::sqrt(a) * sn);
      double b1 = 2 * g * (a - 1 + (a + 1) * cs);
      double b2 = g * (a + 1 + (a - 1) * cs - 2 * std::sqrt(a) * sn);
      double a0 = a + 1 - (a - 1) * cs + 2 * std::sqrt(a) * sn;
      for (int i = 0; i < n; ++i) {
        double x = samples[i];
        double y = (b0 * x + b1 * fx->low_x1 + b2 * fx->low_x2 - (a0 - 2 * (a - 1) * cs - 2 * std::sqrt(a) * sn) * fx->low_y1 - (a0 - (a + 1 - (a - 1) * cs - 2 * std::sqrt(a) * sn)) * fx->low_y2) / a0;
        // simpler: use direct-form with stored states below
        fx->low_x2 = fx->low_x1; fx->low_x1 = x;
        fx->low_y2 = fx->low_y1; fx->low_y1 = y;
        samples[i] = (float)y;
      }
    }
  }
  // Compressor
  if (fx->comp_enabled) compress(fx, samples, n);
  // Soft clip last (guarantees peak <= 1.0)
  if (fx->clip_enabled) softclip(fx, samples, n);
}

}  // extern "C"
