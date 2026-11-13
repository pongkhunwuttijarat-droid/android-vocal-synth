#include "phrase_synth.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <climits>
#include <cmath>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <iterator>
#include <mutex>
#include <thread>
#include <vector>

#include "world/constantnumbers.h"
#include "world/synthesis.h"
#include "worldline/classic/timing.h"
#include "worldline/common/vec_utils.h"
#include "worldline/f0/f0_estimator.h"
#include "worldline/f0/frq_estimator.h"
#include "worldline/f0/pyin_estimator.h"
#include "worldline/model/effects.h"

namespace worldline {

// Frame period in ms. v1 (WORLDLINE-R) uses 10 ms (441 samples @44.1k);
// v2 (WORLDLINE-R2) uses 512 samples = 11.61 ms — the reference picks
// `version == 1 ? 441 : 512` hop in `WorldlineRenderer`. The build
// chooses the default via WORLDLINE_FRAME_MS_DEFAULT (cmake): the v1
// artifact (libworldline-v1.so, matches the official OpenUtau Mobile
// .so) compiles with 10, the v2 artifact (libworldline.so) with 11.61.
// WORLD_FRAME_MS env still overrides for experiments.
#ifndef WORLDLINE_FRAME_MS_DEFAULT
#define WORLDLINE_FRAME_MS_DEFAULT (512.0 * 1000.0 / 44100.0)  // 11.61 — v2
#endif
static double FrameMs() {
  if (const char* v = std::getenv("WORLD_FRAME_MS")) {
    double ms = atof(v);
    if (ms > 0) return ms;
  }
  return WORLDLINE_FRAME_MS_DEFAULT;
}
const int padding = 2;

namespace {

static uint64_t SplitMix64(uint64_t x) {
  x += 0x9e3779b97f4a7c15ULL;
  x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ULL;
  x = (x ^ (x >> 27)) * 0x94d049bb133111ebULL;
  return x ^ (x >> 31);
}

static void HashBytes128(const void* data, size_t len, uint64_t* out_hi,
                         uint64_t* out_lo) {
  uint64_t h1 = 0xcbf29ce484222325ULL ^ static_cast<uint64_t>(len);
  uint64_t h2 = 0x9e3779b97f4a7c15ULL ^ static_cast<uint64_t>(len);
  const auto* bytes = static_cast<const uint8_t*>(data);
  size_t i = 0;
  while (i + 8 <= len) {
    uint64_t w = 0;
    std::memcpy(&w, bytes + i, sizeof(w));
    h1 = SplitMix64(h1 ^ w);
    h2 = SplitMix64(h2 + w * 0xff51afd7ed558ccdULL);
    i += 8;
  }
  if (i < len) {
    uint64_t tail = 0;
    std::memcpy(&tail, bytes + i, len - i);
    h1 = SplitMix64(h1 ^ tail);
    h2 = SplitMix64(h2 + tail * 0xff51afd7ed558ccdULL);
  }
  *out_hi = h1;
  *out_lo = h2;
}

static uint64_t DoubleBits(double v) {
  uint64_t bits = 0;
  std::memcpy(&bits, &v, sizeof(bits));
  return bits;
}

static PreparedCacheKey MakeCacheKey(const SynthRequest& request,
                                     const std::vector<double>& samples,
                                     const std::vector<char>& frq,
                                     double frame_ms) {
  PreparedCacheKey key;
  if (!samples.empty()) {
    HashBytes128(samples.data(), samples.size() * sizeof(double),
                 &key.sample_hash_hi, &key.sample_hash_lo);
  }
  if (!frq.empty()) {
    HashBytes128(frq.data(), frq.size(), &key.frq_hash_hi, &key.frq_hash_lo);
  }
  key.sample_fs = request.sample_fs;
  key.sample_length = static_cast<int32_t>(samples.size());
  key.frq_length = static_cast<int32_t>(frq.size());
  key.frame_ms = frame_ms;
  key.con_vel = request.con_vel;
  key.offset = request.offset;
  key.required_length = request.required_length;
  key.consonant = request.consonant;
  key.cut_off = request.cut_off;
  key.volume = request.volume;
  key.flag_P = request.flag_P;
  return key;
}

static size_t ParseEnvPositiveOr(const char* name, size_t fallback) {
  if (const char* v = std::getenv(name)) {
    char* end = nullptr;
    const long parsed = std::strtol(v, &end, 10);
    if (end != v && *end == '\0' && parsed > 0) {
      return static_cast<size_t>(parsed);
    }
  }
  return fallback;
}

static double CurveAt(const std::vector<double>& curve, int index,
                      double fallback) {
  if (curve.empty()) return fallback;
  if (index < static_cast<int>(curve.size())) return curve[index];
  return curve.back();
}

}  // namespace

bool PreparedCacheKey::operator==(const PreparedCacheKey& other) const {
  return sample_hash_hi == other.sample_hash_hi &&
         sample_hash_lo == other.sample_hash_lo &&
         frq_hash_hi == other.frq_hash_hi &&
         frq_hash_lo == other.frq_hash_lo && sample_fs == other.sample_fs &&
         sample_length == other.sample_length &&
         frq_length == other.frq_length && frame_ms == other.frame_ms &&
         con_vel == other.con_vel && offset == other.offset &&
         required_length == other.required_length &&
         consonant == other.consonant && cut_off == other.cut_off &&
         volume == other.volume && flag_P == other.flag_P;
}

size_t PreparedCacheKeyHash::operator()(const PreparedCacheKey& k) const {
  uint64_t h = SplitMix64(k.sample_hash_hi ^ k.frq_hash_lo);
  h = SplitMix64(h ^ k.sample_hash_lo ^ k.frq_hash_hi);
  h = SplitMix64(h ^ static_cast<uint64_t>(k.sample_fs));
  h = SplitMix64(h ^ DoubleBits(k.offset));
  h = SplitMix64(h ^ DoubleBits(k.required_length));
  h = SplitMix64(h ^ DoubleBits(k.con_vel));
  h = SplitMix64(h ^ DoubleBits(k.frame_ms));
  return static_cast<size_t>(h);
}

PreparedCache::PreparedCache(size_t max_entries) : max_entries_(max_entries) {}

std::shared_ptr<const PreparedModelData> PreparedCache::Get(
    const PreparedCacheKey& key) {
  if (max_entries_ == 0) return nullptr;
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = entries_.find(key);
  if (it == entries_.end()) return nullptr;
  lru_.splice(lru_.begin(), lru_, it->second.second);
  return it->second.first;
}

void PreparedCache::Put(const PreparedCacheKey& key,
                        const std::shared_ptr<const PreparedModelData>& value) {
  if (max_entries_ == 0 || !value) return;
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = entries_.find(key);
  if (it != entries_.end()) {
    it->second.first = value;
    lru_.splice(lru_.begin(), lru_, it->second.second);
    return;
  }
  lru_.push_front(key);
  entries_.emplace(key, std::make_pair(value, lru_.begin()));
  while (entries_.size() > max_entries_ && !lru_.empty()) {
    const PreparedCacheKey& oldest = lru_.back();
    entries_.erase(oldest);
    lru_.pop_back();
  }
}

void PreparedCache::Clear() {
  std::lock_guard<std::mutex> lock(mutex_);
  entries_.clear();
  lru_.clear();
}

size_t PreparedCache::Size() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return entries_.size();
}

void PhraseSynth::AddRequest(const SynthRequest& request, double pos_ms,
                             double skip_ms, double length_ms,
                             double fade_in_ms, double fade_out_ms,
                             LogCallback logCallback) {
  const auto start = std::chrono::steady_clock::now();
  PendingRequest pending{};
  pending.request = request;
  if (request.sample != nullptr && request.sample_length > 0) {
    pending.samples.assign(request.sample,
                           request.sample + request.sample_length);
  }
  if (request.frq != nullptr && request.frq_length > 0) {
    pending.frq.assign(request.frq, request.frq + request.frq_length);
  }
  if (request.pitch_bend != nullptr && request.pitch_bend_length > 0) {
    pending.pitch_bend.assign(request.pitch_bend,
                              request.pitch_bend + request.pitch_bend_length);
  }
  pending.request.sample = nullptr;
  pending.request.frq = nullptr;
  pending.request.pitch_bend = nullptr;
  pending.pos_ms = pos_ms;
  pending.skip_ms = skip_ms;
  pending.length_ms = length_ms;
  pending.fade_in_ms = fade_in_ms;
  pending.fade_out_ms = fade_out_ms;
  pending_requests_.push_back(std::move(pending));
  enqueue_copy_time_ += std::chrono::steady_clock::now() - start;
}

PhraseSynth::ModelTiming PhraseSynth::ComputeTiming(
    const PendingRequest& pending, double frame_ms) {
  ModelTiming timing{};
  timing.left_extra = padding;
  timing.skip = (int)round(pending.skip_ms / frame_ms);
  timing.p0 = (int)round(pending.pos_ms / frame_ms);
  timing.p1 = (int)round((pending.pos_ms + pending.fade_in_ms) / frame_ms);
  timing.p3 = (int)round(
      (pending.pos_ms + pending.length_ms - pending.fade_out_ms) / frame_ms);
  timing.p4 = (int)round((pending.pos_ms + pending.length_ms) / frame_ms);
  timing.p0 = std::max(0, timing.p0);
  timing.p1 = std::max(timing.p0 + 1, timing.p1);
  timing.p3 = std::min(timing.p4 - 1, timing.p3);
  return timing;
}

std::unique_ptr<PhraseSynth::PreparedRequest> PhraseSynth::PrepareSingleRequest(
    PendingRequest& pending, double frame_ms, PreparedCache* cache,
    std::atomic<long long>* analyze_cpu_ns, std::atomic<long long>* sp_cpu_ns,
    std::atomic<long long>* ap_cpu_ns, std::atomic<long long>* remap_cpu_ns) {
  const SynthRequest request = pending.request;
  const ModelTiming timing = ComputeTiming(pending, frame_ms);

  PreparedCacheKey cache_key;
  const bool use_cache =
      cache != nullptr && std::getenv("WORLDLINE_DISABLE_PREP_CACHE") == nullptr;
  if (use_cache) {
    cache_key =
        MakeCacheKey(request, pending.samples, pending.frq, frame_ms);
    if (auto cached = cache->Get(cache_key)) {
      std::vector<double>().swap(pending.samples);
      std::vector<char>().swap(pending.frq);
      std::vector<int>().swap(pending.pitch_bend);
      auto output = std::make_unique<PreparedRequest>();
      output->model = std::move(cached);
      output->timing = timing;
      output->cache_hit = true;
      return output;
    }
  }

  std::vector<double> samples = std::move(pending.samples);
  std::unique_ptr<F0Estimator> f0_estimator;
  if (!pending.frq.empty()) {
    std::string_view frq_data(pending.frq.data(), pending.frq.size());
    f0_estimator = std::make_unique<FrqEstimator>(frq_data);
  } else {
    f0_estimator = std::make_unique<PyinEstimator>();
  }

  Model model(std::move(samples), request.sample_fs, frame_ms,
              std::move(f0_estimator));
  const double src_max = vec_maxabs(model.samples());
  const auto analyze_start = std::chrono::steady_clock::now();
  model.BuildF0();
  auto mapping = GetTimeMapping(model, request);

  const double in_start_ms = request.offset;
  const double in_length_ms = GetInTotalMs(model, request);
  const int in_start_frame = static_cast<int>(in_start_ms / frame_ms);
  const int in_length_frame =
      static_cast<int>(std::ceil(in_start_ms + in_length_ms) / frame_ms) -
      in_start_frame;
  const double left_trimmed = in_start_frame * frame_ms;
  model.Trim(in_start_frame, in_length_frame);

  const double seg_max = vec_maxabs(model.samples());
  AutoGain(model.samples(), src_max, seg_max, model.GetVoicedRatio(),
           request.volume, request.flag_P);
  if (analyze_cpu_ns != nullptr) {
    analyze_cpu_ns->fetch_add(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - analyze_start)
            .count(),
        std::memory_order_relaxed);
  }

  const auto sp_start = std::chrono::steady_clock::now();
  model.BuildSp();
  if (sp_cpu_ns != nullptr) {
    sp_cpu_ns->fetch_add(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - sp_start)
            .count(),
        std::memory_order_relaxed);
  }

  const auto ap_start = std::chrono::steady_clock::now();
  model.BuildAp();
  if (ap_cpu_ns != nullptr) {
    ap_cpu_ns->fetch_add(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - ap_start)
            .count(),
        std::memory_order_relaxed);
  }

  const auto remap_start = std::chrono::steady_clock::now();
  ShiftTimeMapping(mapping, -left_trimmed);
  PadTimeMapping(mapping, padding);
  model.Remap(mapping);
  if (remap_cpu_ns != nullptr) {
    remap_cpu_ns->fetch_add(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - remap_start)
            .count(),
        std::memory_order_relaxed);
  }

  std::vector<char>().swap(pending.frq);
  std::vector<int>().swap(pending.pitch_bend);

  auto data = std::make_shared<PreparedModelData>();
  data->fs = model.fs();
  data->frame_ms = model.frame_ms();
  data->fft_size = model.fft_size();
  data->f0 = std::move(model.f0());
  data->sp = std::move(model.sp());
  data->ap = std::move(model.ap());

  if (use_cache) {
    cache->Put(cache_key, data);
  }

  auto output = std::make_unique<PreparedRequest>();
  output->model = std::move(data);
  output->timing = timing;
  output->cache_hit = false;
  return output;
}

void PhraseSynth::PreparePendingRequests() {
  if (pending_requests_.empty()) return;

  const double frame_ms = FrameMs();
  std::vector<std::unique_ptr<PreparedRequest>> prepared(
      pending_requests_.size());
  std::vector<std::exception_ptr> errors(pending_requests_.size());
  std::atomic<size_t> next{0};
  std::atomic<long long> analyze_cpu_ns{0};
  std::atomic<long long> sp_cpu_ns{0};
  std::atomic<long long> ap_cpu_ns{0};
  std::atomic<long long> remap_cpu_ns{0};

  auto prepare_one = [this, frame_ms, &prepared, &errors, &next,
                      &analyze_cpu_ns, &sp_cpu_ns, &ap_cpu_ns,
                      &remap_cpu_ns]() {
    for (;;) {
      const size_t index = next.fetch_add(1, std::memory_order_relaxed);
      if (index >= pending_requests_.size()) return;
      try {
        prepared[index] = PrepareSingleRequest(
            pending_requests_[index], frame_ms, prepared_cache_,
            &analyze_cpu_ns, &sp_cpu_ns, &ap_cpu_ns, &remap_cpu_ns);
      } catch (...) {
        errors[index] = std::current_exception();
      }
    }
  };

  size_t worker_count = pending_requests_.size() < 4
                            ? 1
                            : std::thread::hardware_concurrency();
  if (worker_count == 0) worker_count = 1;
  worker_count = std::min<size_t>(worker_count, 4);
  worker_count = ParseEnvPositiveOr("WORLDLINE_PREP_THREADS", worker_count);
  worker_count =
      std::max<size_t>(1, std::min(worker_count, pending_requests_.size()));

  const auto workers_start = std::chrono::steady_clock::now();
  std::vector<std::thread> workers;
  workers.reserve(worker_count > 0 ? worker_count - 1 : 0);
  for (size_t i = 1; i < worker_count; ++i) workers.emplace_back(prepare_one);
  prepare_one();
  for (auto& worker : workers) worker.join();
  const auto workers_end = std::chrono::steady_clock::now();

  const auto append_start = std::chrono::steady_clock::now();
  for (size_t i = 0; i < prepared.size(); ++i) {
    if (errors[i]) std::rethrow_exception(errors[i]);
    models_.push_back(std::move(prepared[i]->model));
    timings_.push_back(prepared[i]->timing);
  }
  const auto append_end = std::chrono::steady_clock::now();
  pending_requests_.clear();

  if (std::getenv("WORLDLINE_PROFILE_PREP") != nullptr) {
    const auto ms = [](auto duration) {
      return std::chrono::duration<double, std::milli>(duration).count();
    };
    std::fprintf(
        stderr,
        "WORLDLINE_PROFILE prep jobs=%zu workers=%zu enqueue_copy_ms=%.3f "
        "workers_wall_ms=%.3f analyze_cpu_sum_ms=%.3f sp_cpu_sum_ms=%.3f "
        "ap_cpu_sum_ms=%.3f remap_cpu_sum_ms=%.3f append_ms=%.3f\n",
        prepared.size(), worker_count, ms(enqueue_copy_time_),
        ms(workers_end - workers_start),
        analyze_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        sp_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        ap_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        remap_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        ms(append_end - append_start));
  }
  enqueue_copy_time_ = std::chrono::nanoseconds(0);
}

void PhraseSynth::SetCurves(double* const f0, double* gender, double* tension,
                            double* breathiness, double* voicing, int length,
                            LogCallback logCallback) {
  std::copy(f0, f0 + length, std::back_inserter(f0_));
  std::copy(gender, gender + length, std::back_inserter(gender_));
  std::copy(tension, tension + length, std::back_inserter(tension_));
  std::copy(breathiness, breathiness + length,
            std::back_inserter(breathiness_));
  std::copy(voicing, voicing + length, std::back_inserter(voicing_));
}

std::vector<double> PhraseSynth::SynthInternal(int* output_length) {
  const auto phrase_start = std::chrono::steady_clock::now();
  PreparePendingRequests();
  const auto preparation_end = std::chrono::steady_clock::now();
  if (models_.empty()) {
    if (output_length != nullptr) *output_length = 0;
    return {};
  }
  int fs = models_[0]->fs;
  int fft_size = models_[0]->fft_size;
  int width = static_cast<int>(models_[0]->sp[0].size());

  std::vector<double> f0;
  std::vector<std::vector<double>> sp;
  std::vector<std::vector<double>> ap;
  std::vector<int> dirty;

  const auto timeline_start = std::chrono::steady_clock::now();
  for (size_t k = 0; k < models_.size(); ++k) {
    const auto& model = *models_[k];
    const auto& timing = timings_[k];
    f0.resize(timing.p4, 0);
    sp.resize(timing.p4,
              std::vector<double>(width, world::kMySafeGuardMinimum));
    ap.resize(timing.p4, std::vector<double>(width, 1));
    dirty.resize(timing.p4, 0);

    for (int i = timing.p0; i < timing.p4; ++i) {
      double weight = 1;
      if (i < timing.p1) {
        weight = (double)(i - timing.p0) / (timing.p1 - timing.p0);
      } else if (i >= timing.p3) {
        weight = (double)(timing.p4 - i) / (timing.p4 - timing.p3);
      }
      int model_i = timing.left_extra + timing.skip + i - timing.p0;
      if (model_i < timing.left_extra) {
        continue;
      }
      if (dirty[i] == 0 || weight > 0.5) {
        f0[i] = model.f0[model_i];
      }
      for (int j = 0; j < width; j++) {
        sp[i][j] = sp[i][j] + model.sp[model_i][j] * weight;
      }
      double wa = dirty[i] == 0 ? 0 : 1.0 - weight;
      double wb = dirty[i] == 0 ? 1 : weight;
      for (int j = 0; j < width; j++) {
        ap[i][j] = ap[i][j] * wa + model.ap[model_i][j] * wb;
      }
      dirty[i] = 1;
    }
  }
  int length = static_cast<int>(f0.size()) + 1;
  f0.resize(length, f0.back());
  sp.resize(length, sp.back());
  ap.resize(length, ap.back());
  const auto timeline_end = std::chrono::steady_clock::now();

  f0_.resize(length, f0_.back());
  gender_.resize(length, gender_.back());
  tension_.resize(length, tension_.back());
  breathiness_.resize(length, breathiness_.back());
  voicing_.resize(length, voicing_.back());

  std::vector<std::vector<double>> ten = vec2d(width, length, 1);
  std::vector<double> bre(length, 1);
  std::vector<double> voi(length, 1);
  const auto curves_start = std::chrono::steady_clock::now();
  for (int i = 0; i < length; ++i) {
    if (f0[i] > 0) {
      f0[i] = f0_[i];
    }
    ShiftGender(sp[i].data(), width, (gender_[i] - 0.5) * 200);
    bre[i] = breathiness_[i] > 0.5 ? breathiness_[i] * 4 : breathiness_[i] * 2;
    ten[i] =
        GetTensionCoefficients(f0_[i], fs, (tension_[i] - 0.5) * 200, width);
    voi[i] = voicing_[i];
  }
  const auto curves_end = std::chrono::steady_clock::now();

  Model final_model(models_[0]->fs, models_[0]->frame_ms,
                    models_[0]->fft_size);
  final_model.f0() = std::move(f0);
  final_model.ap() = std::move(ap);
  final_model.sp() = std::move(sp);

  const auto world_start = std::chrono::steady_clock::now();
  final_model.Synth(ten, bre, voi);
  const auto world_end = std::chrono::steady_clock::now();
  if (std::getenv("WORLDLINE_PROFILE_PREP") != nullptr) {
    const auto ms = [](auto duration) {
      return std::chrono::duration<double, std::milli>(duration).count();
    };
    std::fprintf(stderr,
                 "WORLDLINE_PROFILE phrase preparation_wall_ms=%.3f "
                 "timeline_ms=%.3f curves_ms=%.3f world_synth_ms=%.3f "
                 "cpp_total_ms=%.3f\n",
                 ms(preparation_end - phrase_start),
                 ms(timeline_end - timeline_start),
                 ms(curves_end - curves_start), ms(world_end - world_start),
                 ms(world_end - phrase_start));
  }
  std::vector<double> samples = std::move(final_model.samples());

  // 10ms fade out to ease abruptive ending (legacy whole-phrase path).
  int fade_out_samples = static_cast<int>(fs * 10.0 / 1000.0);
  for (int i = 0; i < fade_out_samples && i < static_cast<int>(samples.size());
       ++i) {
    samples[samples.size() - 1 - i] *= i * 1.0 / fade_out_samples;
  }
  if (output_length != nullptr) *output_length = static_cast<int>(samples.size());
  return samples;
}

std::vector<double> PhraseSynth::Synth(LogCallback logCallback) {
  return SynthInternal(nullptr);
}

int PhraseSynth::SynthStreamWithStats(PcmStreamCallback callback,
                                      void* user_data, int block_samples,
                                      StreamStats* stats_out,
                                      LogCallback logCallback) {
  if (callback == nullptr || block_samples <= 0) return -1;
  const auto stream_start = std::chrono::steady_clock::now();
  StreamStats local_stats{};
  local_stats.synth_workers = 1;
  local_stats.first_block_ms = -1.0;

  const size_t pre_count = models_.size();
  const size_t pending_count = pending_requests_.size();
  const size_t total_count = pre_count + pending_count;
  if (total_count == 0) {
    local_stats.first_block_ms = 0.0;
    local_stats.total_ms =
        std::chrono::duration<double, std::milli>(
            std::chrono::steady_clock::now() - stream_start)
            .count();
    if (stats_out != nullptr) *stats_out = local_stats;
    return 0;
  }

  const double frame_ms = FrameMs();
  std::vector<ModelTiming> timings(total_count);
  for (size_t i = 0; i < pre_count; ++i) {
    timings[i] = timings_[i];
  }
  for (size_t i = 0; i < pending_count; ++i) {
    timings[pre_count + i] = ComputeTiming(pending_requests_[i], frame_ms);
  }

  std::vector<int> suffix_min_p0(total_count + 1, INT_MAX);
  std::vector<int> suffix_min_p4(total_count + 1, INT_MAX);
  for (size_t i = total_count; i-- > 0;) {
    suffix_min_p0[i] = std::min(timings[i].p0, suffix_min_p0[i + 1]);
    suffix_min_p4[i] = std::min(timings[i].p4, suffix_min_p4[i + 1]);
  }

  size_t prep_workers = std::min<size_t>(2, pending_count);
  if (pending_count > 0) {
    if (std::getenv("WORLDLINE_STREAM_PREP_THREADS") != nullptr) {
      prep_workers =
          ParseEnvPositiveOr("WORLDLINE_STREAM_PREP_THREADS", prep_workers);
    } else if (std::getenv("WORLDLINE_PREP_THREADS") != nullptr) {
      prep_workers =
          ParseEnvPositiveOr("WORLDLINE_PREP_THREADS", prep_workers);
    }
    prep_workers = std::max<size_t>(1, std::min(prep_workers, pending_count));
  }
  local_stats.prep_workers = static_cast<int32_t>(prep_workers);

  size_t max_in_flight = std::max<size_t>(4, prep_workers * 4);
  max_in_flight =
      ParseEnvPositiveOr("WORLDLINE_STREAM_MAX_INFLIGHT", max_in_flight);
  if (max_in_flight == 0) max_in_flight = 1;

  struct Coordinator {
    std::mutex mu;
    std::condition_variable cv_workers;
    std::condition_variable cv_synth;
    size_t next_assign = 0;
    size_t commit_idx = 0;
    size_t buffered_models = 0;
    int32_t peak_buffered_models = 0;
    int32_t cache_hits = 0;
    int32_t cache_misses = 0;
    bool cancelled = false;
    std::vector<std::shared_ptr<const PreparedModelData>> slots;
    std::vector<std::exception_ptr> errors;
    std::atomic<long long> backpressure_wait_ns{0};
    std::atomic<long long> analyze_cpu_ns{0};
    std::atomic<long long> sp_cpu_ns{0};
    std::atomic<long long> ap_cpu_ns{0};
    std::atomic<long long> remap_cpu_ns{0};
  } coord;

  coord.slots.resize(total_count);
  coord.errors.resize(total_count);
  coord.next_assign = pre_count;
  for (size_t i = 0; i < pre_count; ++i) {
    coord.slots[i] = std::move(models_[i]);
  }
  coord.buffered_models = pre_count;
  coord.peak_buffered_models = static_cast<int32_t>(pre_count);

  std::vector<std::thread> workers;
  workers.reserve(prep_workers);

  struct WorkerJoinGuard {
    Coordinator& coord;
    std::vector<std::thread>& workers;
    ~WorkerJoinGuard() {
      {
        std::lock_guard<std::mutex> lock(coord.mu);
        coord.cancelled = true;
      }
      coord.cv_workers.notify_all();
      for (auto& t : workers) {
        if (t.joinable()) t.join();
      }
    }
  } join_guard{coord, workers};

  auto worker_loop = [this, &coord, frame_ms, pre_count, total_count,
                      max_in_flight]() {
    for (;;) {
      size_t index = 0;
      {
        std::unique_lock<std::mutex> lock(coord.mu);
        if (!coord.cancelled && coord.next_assign < total_count &&
            (coord.next_assign - coord.commit_idx) >= max_in_flight) {
          const auto wait_start = std::chrono::steady_clock::now();
          coord.cv_workers.wait(lock, [&coord, total_count, max_in_flight]() {
            return coord.cancelled || coord.next_assign >= total_count ||
                   (coord.next_assign - coord.commit_idx) < max_in_flight;
          });
          coord.backpressure_wait_ns.fetch_add(
              std::chrono::duration_cast<std::chrono::nanoseconds>(
                  std::chrono::steady_clock::now() - wait_start)
                  .count(),
              std::memory_order_relaxed);
        }
        if (coord.cancelled || coord.next_assign >= total_count) {
          return;
        }
        index = coord.next_assign++;
      }

      try {
        std::unique_ptr<PreparedRequest> prepared = PrepareSingleRequest(
            pending_requests_[index - pre_count], frame_ms, prepared_cache_,
            &coord.analyze_cpu_ns, &coord.sp_cpu_ns, &coord.ap_cpu_ns,
            &coord.remap_cpu_ns);

        std::lock_guard<std::mutex> lock(coord.mu);
        if (coord.cancelled) return;
        coord.slots[index] = std::move(prepared->model);
        if (prepared->cache_hit) {
          ++coord.cache_hits;
        } else {
          ++coord.cache_misses;
        }
        ++coord.buffered_models;
        const int32_t cur_buf = static_cast<int32_t>(coord.buffered_models);
        if (cur_buf > coord.peak_buffered_models) {
          coord.peak_buffered_models = cur_buf;
        }
      } catch (...) {
        std::lock_guard<std::mutex> lock(coord.mu);
        coord.errors[index] = std::current_exception();
      }
      coord.cv_synth.notify_one();
    }
  };

  for (size_t w = 0; w < prep_workers; ++w) {
    workers.emplace_back(worker_loop);
  }

  // Synth worker runs exclusively on the caller thread: incremental timeline
  // merge, curve conversion, continuous WORLD cursor, and 10 ms tail fade.
  struct CursorGuard {
    WorldStreamCursor* cursor = nullptr;
    ~CursorGuard() { WorldStreamCursorDestroy(cursor); }
  } world_guard;

  int fs = 0;
  int fft_size = 0;
  int width = 0;
  double synth_frame_ms = frame_ms;

  int raw_base = 0;
  int cur_raw_len = 0;
  std::deque<double> raw_f0;
  std::deque<std::vector<double>> raw_sp;
  std::deque<std::vector<double>> raw_ap;
  std::deque<int> raw_dirty;

  int curve_base = 0;
  int converted_end = 0;
  std::deque<double> c_f0;
  std::deque<std::vector<double>> c_sp;
  std::deque<std::vector<double>> c_ap;
  std::deque<std::vector<double>> c_ten;
  std::deque<double> c_bre;
  std::deque<double> c_voi;

  struct SinkState {
    PcmStreamCallback callback;
    void* user_data;
    int tail_samples;
    int block_samples;
    std::vector<double> pending;
    size_t pending_head = 0;
    std::chrono::steady_clock::time_point start;
    double first_block_ms = -1.0;
    int emitted_total = 0;
    bool cancelled = false;
    std::vector<float> float_scratch;
  } sink{callback, user_data, 0, std::max(1, block_samples), {}, 0,
         stream_start, -1.0, 0, false,
         std::vector<float>(static_cast<size_t>(std::max(1, block_samples)))};

  auto world_emit = [](const double* samples, int count, void* opaque) -> int {
    auto* st = static_cast<SinkState*>(opaque);
    st->pending.insert(st->pending.end(), samples, samples + count);
    const int avail =
        static_cast<int>(st->pending.size() - st->pending_head) -
        st->tail_samples;
    int consumed = 0;
    while (avail - consumed >= st->block_samples) {
      for (int i = 0; i < st->block_samples; ++i) {
        st->float_scratch[i] =
            static_cast<float>(st->pending[st->pending_head + consumed + i]);
      }
      if (st->first_block_ms < 0.0) {
        st->first_block_ms =
            std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - st->start)
                .count();
      }
      if (st->callback(st->float_scratch.data(), st->block_samples,
                       st->user_data) == 0) {
        st->cancelled = true;
        return 0;
      }
      st->emitted_total += st->block_samples;
      consumed += st->block_samples;
    }
    if (consumed > 0) {
      st->pending_head += static_cast<size_t>(consumed);
      if (st->pending_head >= 8192 &&
          st->pending_head * 2 >= st->pending.size()) {
        st->pending.erase(st->pending.begin(),
                          st->pending.begin() + st->pending_head);
        st->pending_head = 0;
      }
    }
    return 1;
  };

  std::vector<double> win_f0;
  std::vector<const double*> win_sp;
  std::vector<const double*> win_ap;
  std::vector<double*> win_ten;
  std::vector<double> win_bre;
  std::vector<double> win_voi;

  long long prep_wait_ns = 0;
  int32_t peak_timeline_frames = 0;
  int last_fed_end = 0;
  int final_y_len = 0;
  bool aborted = false;

  for (size_t k = 0; k < total_count; ++k) {
    std::shared_ptr<const PreparedModelData> model_ptr;
    std::exception_ptr err;
    {
      std::unique_lock<std::mutex> lock(coord.mu);
      if (!coord.slots[k] && !coord.errors[k] && !coord.cancelled) {
        const auto wait_start = std::chrono::steady_clock::now();
        coord.cv_synth.wait(lock, [&coord, k]() {
          return coord.slots[k] != nullptr || coord.errors[k] != nullptr ||
                 coord.cancelled;
        });
        prep_wait_ns += std::chrono::duration_cast<std::chrono::nanoseconds>(
                            std::chrono::steady_clock::now() - wait_start)
                            .count();
      }
      if (coord.errors[k]) {
        err = coord.errors[k];
      } else if (coord.cancelled || !coord.slots[k]) {
        aborted = true;
      } else {
        model_ptr = std::move(coord.slots[k]);
        coord.commit_idx = k + 1;
        --coord.buffered_models;
      }
    }
    coord.cv_workers.notify_all();

    if (err) {
      {
        std::lock_guard<std::mutex> lock(coord.mu);
        coord.cancelled = true;
      }
      coord.cv_workers.notify_all();
      for (auto& t : workers) {
        if (t.joinable()) t.join();
      }
      std::rethrow_exception(err);
    }
    if (aborted) {
      break;
    }

    if (world_guard.cursor == nullptr) {
      fs = model_ptr->fs;
      fft_size = model_ptr->fft_size;
      synth_frame_ms = model_ptr->frame_ms;
      width = model_ptr->sp.empty() ? 0 : static_cast<int>(model_ptr->sp[0].size());
      if (fs <= 0 || fft_size <= 0 || width <= 0) {
        aborted = true;
        break;
      }
      sink.tail_samples = static_cast<int>(fs * 10.0 / 1000.0);
      world_guard.cursor = WorldStreamCursorCreate(
          fft_size, synth_frame_ms, fs, std::max(1, block_samples));
      if (world_guard.cursor == nullptr) {
        aborted = true;
        break;
      }
    }

    // Merge request k into the sliding raw timeline.
    const ModelTiming& timing = timings[k];
    if (timing.p4 < cur_raw_len) {
      const size_t keep = static_cast<size_t>(std::max(0, timing.p4 - raw_base));
      raw_f0.resize(keep);
      raw_sp.resize(keep);
      raw_ap.resize(keep);
      raw_dirty.resize(keep);
      cur_raw_len = raw_base + static_cast<int>(keep);
    } else if (timing.p4 > cur_raw_len) {
      const size_t new_sz = static_cast<size_t>(timing.p4 - raw_base);
      raw_f0.resize(new_sz, 0.0);
      raw_sp.resize(new_sz,
                    std::vector<double>(width, world::kMySafeGuardMinimum));
      raw_ap.resize(new_sz, std::vector<double>(width, 1.0));
      raw_dirty.resize(new_sz, 0);
      cur_raw_len = timing.p4;
    }

    for (int i = timing.p0; i < timing.p4; ++i) {
      double weight = 1.0;
      if (i < timing.p1) {
        weight = static_cast<double>(i - timing.p0) / (timing.p1 - timing.p0);
      } else if (i >= timing.p3) {
        weight = static_cast<double>(timing.p4 - i) / (timing.p4 - timing.p3);
      }
      const int model_i = timing.left_extra + timing.skip + i - timing.p0;
      if (model_i < timing.left_extra) {
        continue;
      }
      const size_t local_i = static_cast<size_t>(i - raw_base);
      if (raw_dirty[local_i] == 0 || weight > 0.5) {
        raw_f0[local_i] = model_ptr->f0[model_i];
      }
      for (int j = 0; j < width; ++j) {
        raw_sp[local_i][j] =
            raw_sp[local_i][j] + model_ptr->sp[model_i][j] * weight;
      }
      const double wa = raw_dirty[local_i] == 0 ? 0.0 : 1.0 - weight;
      const double wb = raw_dirty[local_i] == 0 ? 1.0 : weight;
      for (int j = 0; j < width; ++j) {
        raw_ap[local_i][j] =
            raw_ap[local_i][j] * wa + model_ptr->ap[model_i][j] * wb;
      }
      raw_dirty[local_i] = 1;
    }

    // Drop our reference to model_ptr promptly after merging its timeline range.
    model_ptr.reset();

    const bool is_final = (k + 1 == total_count);
    const int finalized_end =
        is_final ? cur_raw_len
                 : std::min({cur_raw_len, suffix_min_p0[k + 1],
                             suffix_min_p4[k + 1]});
    // Hold back at least one frame before suffix_min_p4 so the pre-curve trailing
    // frame at cur_raw_len - 1 is always available to duplicate when commit finishes.
    const int target_convert_end =
        is_final ? cur_raw_len
                 : std::max(converted_end, finalized_end - 1);

    double dup_f0 = 0.0;
    std::vector<double> dup_sp;
    std::vector<double> dup_ap;
    bool have_dup = false;

    for (int i = converted_end; i < target_convert_end; ++i) {
      double f0_val = raw_f0.front();
      std::vector<double> sp_row = std::move(raw_sp.front());
      std::vector<double> ap_row = std::move(raw_ap.front());
      raw_f0.pop_front();
      raw_sp.pop_front();
      raw_ap.pop_front();
      raw_dirty.pop_front();
      ++raw_base;

      if (is_final && i == cur_raw_len - 1) {
        dup_f0 = f0_val;
        dup_sp = sp_row;
        dup_ap = ap_row;
        have_dup = true;
      }

      const double cf0 = CurveAt(f0_, i, 0.0);
      const double cgen = CurveAt(gender_, i, 0.5);
      const double cten = CurveAt(tension_, i, 0.5);
      const double cbre = CurveAt(breathiness_, i, 0.5);
      const double cvoi = CurveAt(voicing_, i, 1.0);

      if (f0_val > 0) {
        f0_val = cf0;
      }
      ShiftGender(sp_row.data(), width, (cgen - 0.5) * 200);
      const double bre_val = cbre > 0.5 ? cbre * 4 : cbre * 2;
      std::vector<double> ten_row =
          GetTensionCoefficients(cf0, fs, (cten - 0.5) * 200, width);

      c_f0.push_back(f0_val);
      c_sp.push_back(std::move(sp_row));
      c_ap.push_back(std::move(ap_row));
      c_ten.push_back(std::move(ten_row));
      c_bre.push_back(bre_val);
      c_voi.push_back(cvoi);
    }
    converted_end = target_convert_end;

    if (is_final && have_dup) {
      const int i = cur_raw_len;
      const double cf0 = CurveAt(f0_, i, 0.0);
      const double cgen = CurveAt(gender_, i, 0.5);
      const double cten = CurveAt(tension_, i, 0.5);
      const double cbre = CurveAt(breathiness_, i, 0.5);
      const double cvoi = CurveAt(voicing_, i, 1.0);

      if (dup_f0 > 0) {
        dup_f0 = cf0;
      }
      ShiftGender(dup_sp.data(), width, (cgen - 0.5) * 200);
      const double bre_val = cbre > 0.5 ? cbre * 4 : cbre * 2;
      std::vector<double> ten_row =
          GetTensionCoefficients(cf0, fs, (cten - 0.5) * 200, width);

      c_f0.push_back(dup_f0);
      c_sp.push_back(std::move(dup_sp));
      c_ap.push_back(std::move(dup_ap));
      c_ten.push_back(std::move(ten_row));
      c_bre.push_back(bre_val);
      c_voi.push_back(cvoi);
      ++converted_end;
    }

    const int32_t buffered_timeline =
        static_cast<int32_t>(raw_f0.size() + c_f0.size());
    if (buffered_timeline > peak_timeline_frames) {
      peak_timeline_frames = buffered_timeline;
    }

    if (is_final) {
      final_y_len =
          converted_end > 0
              ? static_cast<int>(fs * (converted_end - 1) * synth_frame_ms /
                                 1000.0) +
                    1
              : 0;
    }

    bool next_ready = false;
    if (!is_final) {
      std::lock_guard<std::mutex> lock(coord.mu);
      next_ready = coord.slots[k + 1] != nullptr;
    }

    const bool should_feed =
        is_final ||
        (converted_end - curve_base >= 2 &&
         (!next_ready || sink.first_block_ms < 0.0 ||
          converted_end - last_fed_end >= 8));

    if (should_feed && (is_final || converted_end - curve_base >= 2)) {
      const size_t avail = static_cast<size_t>(converted_end - curve_base);
      win_f0.resize(avail);
      win_sp.resize(avail);
      win_ap.resize(avail);
      win_ten.resize(avail);
      win_bre.resize(avail);
      win_voi.resize(avail);
      for (size_t i = 0; i < avail; ++i) {
        win_f0[i] = c_f0[i];
        win_sp[i] = c_sp[i].data();
        win_ap[i] = c_ap[i].data();
        win_ten[i] = c_ten[i].data();
        win_bre[i] = c_bre[i];
        win_voi[i] = c_voi[i];
      }

      int consumed_end = curve_base;
      const int feed_rc = WorldStreamCursorFeed(
          world_guard.cursor, curve_base, converted_end,
          is_final ? converted_end : 0, is_final ? final_y_len : -1,
          win_f0.data(), win_sp.data(), win_ap.data(), win_ten.data(),
          win_bre.data(), win_voi.data(), world_emit, &sink, &consumed_end);
      last_fed_end = converted_end;

      if (feed_rc != 0 || sink.cancelled) {
        aborted = true;
        {
          std::lock_guard<std::mutex> lock(coord.mu);
          coord.cancelled = true;
        }
        coord.cv_workers.notify_all();
        break;
      }

      while (curve_base < consumed_end && !c_f0.empty()) {
        c_f0.pop_front();
        c_sp.pop_front();
        c_ap.pop_front();
        c_ten.pop_front();
        c_bre.pop_front();
        c_voi.pop_front();
        ++curve_base;
      }
    }
  }

  // Ensure background prep workers are joined before emitting final stats or
  // touching remaining state.
  {
    std::lock_guard<std::mutex> lock(coord.mu);
    if (aborted) coord.cancelled = true;
  }
  coord.cv_workers.notify_all();
  for (auto& t : workers) {
    if (t.joinable()) t.join();
  }

  if (aborted || sink.cancelled) {
    if (stats_out != nullptr) {
      local_stats.first_block_ms = sink.first_block_ms;
      local_stats.total_ms =
          std::chrono::duration<double, std::milli>(
              std::chrono::steady_clock::now() - stream_start)
              .count();
      local_stats.prep_wait_ms = prep_wait_ns / 1e6;
      local_stats.backpressure_wait_ms =
          coord.backpressure_wait_ns.load(std::memory_order_relaxed) / 1e6;
      local_stats.peak_buffered_models = coord.peak_buffered_models;
      local_stats.peak_timeline_frames = peak_timeline_frames;
      local_stats.total_frames = converted_end;
      local_stats.output_samples = sink.emitted_total;
      local_stats.cache_hits = coord.cache_hits;
      local_stats.cache_misses = coord.cache_misses;
      *stats_out = local_stats;
    }
    return -1;
  }

  // Apply the exact same 10 ms linear fadeout to retained tail samples.
  const size_t tail_len = sink.pending.size() - sink.pending_head;
  for (int i = 0; i < sink.tail_samples && i < static_cast<int>(tail_len);
       ++i) {
    sink.pending[sink.pending.size() - 1 - i] *= i * 1.0 / sink.tail_samples;
  }
  if (tail_len > 0) {
    size_t pos = sink.pending_head;
    while (pos < sink.pending.size()) {
      const int chunk = static_cast<int>(
          std::min<size_t>(sink.block_samples, sink.pending.size() - pos));
      for (int i = 0; i < chunk; ++i) {
        sink.float_scratch[i] = static_cast<float>(sink.pending[pos + i]);
      }
      if (sink.first_block_ms < 0.0) {
        sink.first_block_ms =
            std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - stream_start)
                .count();
      }
      if (sink.callback(sink.float_scratch.data(), chunk, sink.user_data) ==
          0) {
        sink.cancelled = true;
        break;
      }
      sink.emitted_total += chunk;
      pos += static_cast<size_t>(chunk);
    }
  }
  if (sink.cancelled) {
    return -1;
  }

  const auto stream_end = std::chrono::steady_clock::now();
  local_stats.first_block_ms =
      sink.first_block_ms >= 0.0
          ? sink.first_block_ms
          : std::chrono::duration<double, std::milli>(stream_end - stream_start)
                .count();
  local_stats.total_ms =
      std::chrono::duration<double, std::milli>(stream_end - stream_start)
          .count();
  local_stats.prep_wait_ms = prep_wait_ns / 1e6;
  local_stats.backpressure_wait_ms =
      coord.backpressure_wait_ns.load(std::memory_order_relaxed) / 1e6;
  local_stats.peak_buffered_models = coord.peak_buffered_models;
  local_stats.peak_timeline_frames = peak_timeline_frames;
  local_stats.total_frames = converted_end;
  local_stats.output_samples = sink.emitted_total;
  local_stats.cache_hits = coord.cache_hits;
  local_stats.cache_misses = coord.cache_misses;
  if (stats_out != nullptr) *stats_out = local_stats;

  if (std::getenv("WORLDLINE_PROFILE_PREP") != nullptr) {
    std::fprintf(
        stderr,
        "WORLDLINE_PROFILE stream jobs=%zu prep_workers=%d synth_workers=%d "
        "first_block_ms=%.3f total_ms=%.3f prep_wait_ms=%.3f "
        "backpressure_wait_ms=%.3f peak_buffered_models=%d "
        "peak_timeline_frames=%d total_frames=%d cache_hits=%d cache_misses=%d "
        "analyze_cpu_sum_ms=%.3f sp_cpu_sum_ms=%.3f ap_cpu_sum_ms=%.3f "
        "remap_cpu_sum_ms=%.3f\n",
        total_count, local_stats.prep_workers, local_stats.synth_workers,
        local_stats.first_block_ms, local_stats.total_ms,
        local_stats.prep_wait_ms, local_stats.backpressure_wait_ms,
        local_stats.peak_buffered_models, local_stats.peak_timeline_frames,
        local_stats.total_frames, local_stats.cache_hits,
        local_stats.cache_misses,
        coord.analyze_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        coord.sp_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        coord.ap_cpu_ns.load(std::memory_order_relaxed) / 1e6,
        coord.remap_cpu_ns.load(std::memory_order_relaxed) / 1e6);
  }

  pending_requests_.clear();
  enqueue_copy_time_ = std::chrono::nanoseconds(0);
  return sink.emitted_total;
}

int PhraseSynth::SynthStream(PcmStreamCallback callback, void* user_data,
                             int block_samples, LogCallback logCallback) {
  return SynthStreamWithStats(callback, user_data, block_samples, nullptr,
                              logCallback);
}

}  // namespace worldline
