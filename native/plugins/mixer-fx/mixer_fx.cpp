// Mixer FX plugin — per-track (or final) FX chain for Lilt.
//
// FFI contract (mirrors the worldline plugin pattern):
//   void* MxFxCreate(const MxFxConfig* cfg, const char* params_json);
//   void  MxFxProcess(void* fx, float* samples, int n, double pos_ms);
//   void  MxFxDestroy(void* fx);
//
// Chain (OpenUtau MixFxSource order): gain -> 3-band EQ -> compressor
// -> soft clip.
//
// EQ: 3-band biquad (low shelf 200 Hz, mid peak 1 kHz, high shelf 4 kHz),
// coefficients from the RBJ Audio EQ Cookbook, filters run in
// Transposed Direct Form II (numerically stable). Each band keeps its own
// DF2T state, so the chain is fully stateful across process() calls.
//
// Compressor: one-pole envelope follower (attack/release) with
// soft-knee-less over-threshold ratio reduction + fixed makeup gain.
//
// Soft clip: tanh — guarantees peak <= 1.0 without hard distortion.

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

// One biquad section in Transposed Direct Form II.
struct Biquad {
  double b0, b1, b2, a1, a2;  // a0 normalized to 1
  double s1 = 0.0, s2 = 0.0;  // DF2T state

  void reset() { s1 = s2 = 0.0; }

  // One sample, in place.
  double process(double x) {
    double y = b0 * x + s1;
    s1 = b1 * x - a1 * y + s2;
    s2 = b2 * x - a2 * y;
    return y;
  }
};

// --- RBJ Audio EQ Cookbook coefficient helpers ---

// Low shelf: f0 = corner, gain_db = shelf gain, S = shelf slope (1 = max).
static void lowshelf(Biquad& b, double f0, double gain_db, double S, double fs) {
  double A = std::pow(10.0, gain_db / 40.0);
  double w0 = 2.0 * M_PI * f0 / fs;
  double cs = std::cos(w0);
  double alpha = std::sin(w0) / 2.0 * std::sqrt(2.0 * A * S);
  double sqA = 2.0 * std::sqrt(A) * alpha;
  double a0 = (A + 1.0) + (A - 1.0) * cs + sqA;
  b.b0 = A * ((A + 1.0) - (A - 1.0) * cs + sqA) / a0;
  b.b1 = 2.0 * A * ((A - 1.0) - (A + 1.0) * cs) / a0;
  b.b2 = A * ((A + 1.0) - (A - 1.0) * cs - sqA) / a0;
  b.a1 = -2.0 * ((A - 1.0) + (A + 1.0) * cs) / a0;
  b.a2 = ((A + 1.0) + (A - 1.0) * cs - sqA) / a0;
}

// High shelf: f0 = corner, gain_db = shelf gain, S = shelf slope (1 = max).
static void highshelf(Biquad& b, double f0, double gain_db, double S, double fs) {
  double A = std::pow(10.0, gain_db / 40.0);
  double w0 = 2.0 * M_PI * f0 / fs;
  double cs = std::cos(w0);
  double alpha = std::sin(w0) / 2.0 * std::sqrt(2.0 * A * S);
  double sqA = 2.0 * std::sqrt(A) * alpha;
  double a0 = (A + 1.0) - (A - 1.0) * cs + sqA;
  b.b0 = A * ((A + 1.0) + (A - 1.0) * cs + sqA) / a0;
  b.b1 = -2.0 * A * ((A - 1.0) + (A + 1.0) * cs) / a0;
  b.b2 = A * ((A + 1.0) + (A - 1.0) * cs - sqA) / a0;
  b.a1 = 2.0 * ((A - 1.0) - (A + 1.0) * cs) / a0;
  b.a2 = ((A + 1.0) - (A - 1.0) * cs - sqA) / a0;
}

// Peak (parametric): f0 = center, gain_db = boost/cut, Q = bandwidth.
static void peak(Biquad& b, double f0, double gain_db, double Q, double fs) {
  double A = std::pow(10.0, gain_db / 40.0);
  double w0 = 2.0 * M_PI * f0 / fs;
  double cs = std::cos(w0);
  double alpha = std::sin(w0) / (2.0 * Q);
  double a0 = 1.0 + alpha / A;
  b.b0 = (1.0 + alpha * A) / a0;
  b.b1 = (-2.0 * cs) / a0;
  b.b2 = (1.0 - alpha * A) / a0;
  b.a1 = (-2.0 * cs) / a0;
  b.a2 = (1.0 - alpha / A) / a0;
}

struct MxFx {
  double sample_rate;
  // Chain state
  double gain;        // linear pre-gain
  double low_gain;    // dB, 3-band EQ
  double mid_gain;
  double high_gain;
  double comp_threshold;  // linear (e.g. 0.5)
  double comp_ratio;      // e.g. 4.0
  double comp_attack;     // seconds
  double comp_release;    // seconds
  double comp_makeup;     // dB post-compensation
  double comp_env;        // envelope follower state
  bool eq_enabled;
  bool comp_enabled;
  bool clip_enabled;

  // 3-band EQ (RBJ cookbook, DF2T)
  Biquad low, mid, high;
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
  fx->comp_makeup = parse_param(p, "comp_makeup", 0.0);
  fx->comp_env = 0.0;
  fx->eq_enabled = parse_param(p, "eq_enabled", 0.0) > 0.5;
  fx->comp_enabled = parse_param(p, "comp_enabled", 0.0) > 0.5;
  fx->clip_enabled = parse_param(p, "clip_enabled", 1.0) > 0.5;
  fx->low.reset(); fx->mid.reset(); fx->high.reset();
  // Precompute EQ coefficients (bands always configured; processing only
  // runs when eq_enabled, but coefficients must be valid regardless).
  double fs = fx->sample_rate;
  double S = parse_param(p, "eq_slope", 1.0);
  lowshelf(fx->low, 200.0, fx->low_gain, S, fs);
  peak(fx->mid, 1000.0, fx->mid_gain, parse_param(p, "eq_mid_q", 1.0), fs);
  highshelf(fx->high, 4000.0, fx->high_gain, S, fs);
  return fx;
}

DLL_API void MxFxDestroy(void* handle) {
  delete static_cast<MxFx*>(handle);
}

// One-pole compressor (envelope follower + ratio reduction + makeup).
static void compress(MxFx* fx, float* s, int n) {
  double th = fx->comp_threshold;
  double ratio = fx->comp_ratio;
  double makeup = db_to_lin(fx->comp_makeup);
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
    s[i] = (float)(s[i] * gain * makeup);
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
  // 3-band EQ: low shelf -> mid peak -> high shelf (stateful DF2T)
  if (fx->eq_enabled) {
    for (int i = 0; i < n; ++i) {
      double x = samples[i];
      x = fx->low.process(x);
      x = fx->mid.process(x);
      x = fx->high.process(x);
      samples[i] = (float)x;
    }
  }
  // Compressor
  if (fx->comp_enabled) compress(fx, samples, n);
  // Soft clip last (guarantees peak <= 1.0)
  if (fx->clip_enabled) softclip(fx, samples, n);
}

}  // extern "C"
