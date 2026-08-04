// STUB implementation of the Platinum voice-quality module.
//
// The ref OpenUtau desktop build vendors full platinum.cpp / synthesisplatinum.cpp
// (residual-spectrum extraction + synthesis from Masanori Morise's work). The
// worldline synthesis pipeline (Model::BuildResidual / Model::SynthPlatinum)
// is NEVER invoked: Resampler only calls BuildF0/BuildSp/BuildAp/Remap/Synth.
// Per the P.3 trim plan, the platinum implementations are therefore replaced by
// this stub that zero-fills its outputs. model.cpp stays byte-identical; only
// the two entry points are stubbed. If platinum quality is ever needed, drop
// the originals back in and delete this file.
//
// Signatures must match src/platinum/*.h (copied from ref, unchanged). The
// functions keep C++ linkage (no extern "C") so the mangled symbols match the
// ref .so: _Z8PlatinumPdiiS_S_iPS_iS0_ and
// _Z17SynthesisPlatinumPdiPS_S0_idiiS_.

#include <cstdio>
#include <cstring>

#include "worldline/platinum/platinum.h"
#include "worldline/platinum/synthesisplatinum.h"

namespace {

void WarnOnce() {
  static bool warned = false;
  if (!warned) {
    std::fprintf(stderr,
                 "[worldline] note: Platinum residual synthesis is stubbed "
                 "(WORLDLINE_PLATINUM_STUB); BuildResidual/SynthPlatinum are "
                 "not used by the resampler pipeline.\n");
    warned = true;
  }
}

}  // namespace

void Platinum(double *x, int x_length, int fs, double *time_axis, double *f0,
              int f0_length, double **spectrogram, int fft_size,
              double **residual_spectrogram) {
  (void)x; (void)x_length; (void)fs; (void)time_axis;
  (void)f0; (void)f0_length; (void)spectrogram;
  WarnOnce();
  if (residual_spectrogram != nullptr) {
    for (int i = 0; i < f0_length; ++i) {
      if (residual_spectrogram[i] != nullptr) {
        std::memset(residual_spectrogram[i], 0, sizeof(double) * fft_size);
      }
    }
  }
}

void SynthesisPlatinum(double *f0, int f0_length, double **spectrogram,
                       double **residual_spectrogram, int fft_size,
                       double frame_period, int fs, int y_length, double *y) {
  (void)f0; (void)f0_length; (void)spectrogram; (void)residual_spectrogram;
  (void)fft_size; (void)frame_period; (void)fs;
  WarnOnce();
  if (y != nullptr) {
    std::memset(y, 0, sizeof(double) * y_length);
  }
}
