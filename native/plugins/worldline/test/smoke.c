/* Smoke test for libworldline.so (Plugin track P.3).
 *
 * Pure C consumer of the worldline C API (declarations by hand: worldline.h
 * is a C++ header and cannot be #included from C). Two stages:
 *   1. PhraseSynthNew/PhraseSynthDelete lifecycle (acceptance gate).
 *   2. Full functional pass: AddRequest (0.3 s 220 Hz sine) + SetCurves +
 *      PhraseSynthSynth, and the standalone Resample API — exercises pyin F0,
 *      CheapTrick, D4C, spline effects and WORLD Synthesis inside the .so.
 *
 * Build:
 *   cc -O2 smoke.c -o smoke -L<build_dir> -lworldline -lm
 * Run (Linux):
 *   LD_LIBRARY_PATH=<build_dir> ./smoke
 *
 * Note: buffers returned by Synth/Resample are allocated with C++ new[] in
 * the .so; free() is used here (glibc/NDK allocators are compatible, and
 * this matches how the ref .so is consumed over FFI).
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void (*WorldlineLogCallback)(const char *log);

struct SynthRequest {
  int32_t sample_fs;
  int32_t sample_length;
  double *sample;
  int32_t frq_length;
  char *frq;
  int32_t tone;
  double con_vel;
  double offset;
  double required_length;
  double consonant;
  double cut_off;
  double volume;
  double modulation;
  double tempo;
  int32_t pitch_bend_length;
  int32_t *pitch_bend;
  int flag_g;
  int flag_O;
  int flag_P;
  int flag_Mt;
  int flag_Mb;
  int flag_Mv;
};

void *PhraseSynthNew(void);
void PhraseSynthDelete(void *phrase_synth);
void PhraseSynthAddRequest(void *phrase_synth, const struct SynthRequest *request,
                           double pos_ms, double skip_ms, double length_ms,
                           double fade_in_ms, double fade_out_ms,
                           WorldlineLogCallback logCallback);
void PhraseSynthSetCurves(void *phrase_synth, double *f0, double *gender,
                          double *tension, double *breathiness, double *voicing,
                          int length, WorldlineLogCallback logCallback);
int PhraseSynthSynth(void *phrase_synth, float **y,
                     WorldlineLogCallback logCallback);
int Resample(const struct SynthRequest *request, float **y);

static void on_log(const char *msg) {
  if (msg) fprintf(stderr, "[worldline log] %s\n", msg);
}

#define FS 44100
#define DURATION_S 0.3
#define N_SAMPLES ((int)(FS * DURATION_S))
#define CURVE_LEN 40

static int failures = 0;
#define CHECK(cond, msg)                                        \
  do {                                                          \
    if (cond) {                                                 \
      printf("  PASS: %s\n", msg);                              \
    } else {                                                    \
      printf("  FAIL: %s\n", msg);                              \
      failures++;                                               \
    }                                                           \
  } while (0)

static void make_request(struct SynthRequest *req, double *samples) {
  memset(req, 0, sizeof(*req));
  for (int i = 0; i < N_SAMPLES; ++i) {
    samples[i] = 0.5 * sin(2.0 * M_PI * 220.0 * i / FS);
  }
  req->sample_fs = FS;
  req->sample_length = N_SAMPLES;
  req->sample = samples;
  req->frq_length = 0;
  req->frq = NULL;
  req->tone = 48;            /* C3 */
  req->con_vel = 120;
  req->offset = 0;
  req->required_length = 200;  /* ms */
  req->consonant = 5;
  req->cut_off = 5;
  req->volume = 0.8;
  req->modulation = 0;
  req->tempo = 120;
  req->pitch_bend_length = 0;
  req->pitch_bend = NULL;
  req->flag_g = req->flag_O = req->flag_P = 0;
  req->flag_Mt = req->flag_Mb = req->flag_Mv = 0;
}

int main(void) {
  double *samples = malloc(sizeof(double) * N_SAMPLES);
  struct SynthRequest req;
  double f0[CURVE_LEN], gender[CURVE_LEN], tension[CURVE_LEN];
  double breathiness[CURVE_LEN], voicing[CURVE_LEN];
  float *y = NULL;
  int len;

  printf("smoke: stage 1 — lifecycle\n");
  void *ps = PhraseSynthNew();
  CHECK(ps != NULL, "PhraseSynthNew returns non-NULL");
  if (ps == NULL) return 1;
  PhraseSynthDelete(ps);
  printf("  PASS: PhraseSynthDelete\n");

  printf("smoke: stage 2 — full synthesis pass\n");
  make_request(&req, samples);
  ps = PhraseSynthNew();
  /* length_ms MUST be <= required_length: the model is trimmed to
     offset+required_length frames, and timing.p4 = (pos+length)/frame_ms
     indexes into that trimmed model (phrase_synth.cpp:130). OpenUtau
     always passes the phoneme duration, which equals required_length. */
  PhraseSynthAddRequest(ps, &req, /*pos_ms=*/0, /*skip_ms=*/0, /*length_ms=*/150,
                        /*fade_in_ms=*/5, /*fade_out_ms=*/5, on_log);
  printf("  PASS: PhraseSynthAddRequest\n");

  for (int i = 0; i < CURVE_LEN; ++i) {
    f0[i] = 0;              /* 0 = keep estimated pitch */
    gender[i] = 0.5;
    tension[i] = 0.5;
    breathiness[i] = 0.5;
    voicing[i] = 1.0;
  }
  PhraseSynthSetCurves(ps, f0, gender, tension, breathiness, voicing,
                       CURVE_LEN, on_log);
  printf("  PASS: PhraseSynthSetCurves\n");

  len = PhraseSynthSynth(ps, &y, on_log);
  CHECK(y != NULL && len > 0, "PhraseSynthSynth produces output");
  if (y != NULL && len > 0) {
    double peak = 0;
    for (int i = 0; i < len; ++i) {
      double a = fabs(y[i]);
      if (a > peak) peak = a;
    }
    printf("  info: synth length=%d samples (%.1f ms), peak=%.4f\n", len,
           len * 1000.0 / FS, peak);
    CHECK(peak > 1e-6, "synth output is non-silent");
  }
  free(y);
  y = NULL;
  PhraseSynthDelete(ps);

  len = Resample(&req, &y);
  CHECK(y != NULL && len > 0, "Resample produces output");
  if (y != NULL && len > 0) {
    printf("  info: resample length=%d samples (%.1f ms)\n", len,
           len * 1000.0 / FS);
  }
  free(y);

  free(samples);
  if (failures == 0) {
    printf("SMOKE TEST PASSED\n");
    return 0;
  }
  printf("SMOKE TEST FAILED (%d failures)\n", failures);
  return 1;
}
