#ifndef WORLDLINE_PHRASE_SYNTH_H_
#define WORLDLINE_PHRASE_SYNTH_H_

#include <atomic>
#include <chrono>
#include <cstdint>
#include <deque>
#include <list>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

#include "worldline/model/model.h"
#include "worldline/synth_request.h"

namespace worldline {

typedef void(/*__stdcall*/ *LogCallback)(const char* log);
typedef int (*PcmStreamCallback)(const float* samples, int length,
                                  void* user_data);

struct PreparedModelData {
  int fs = 0;
  double frame_ms = 0.0;
  int fft_size = 0;
  std::vector<double> f0;
  std::vector<std::vector<double>> sp;
  std::vector<std::vector<double>> ap;
};

struct PreparedCacheKey {
  uint64_t sample_hash_hi = 0;
  uint64_t sample_hash_lo = 0;
  uint64_t frq_hash_hi = 0;
  uint64_t frq_hash_lo = 0;
  int32_t sample_fs = 0;
  int32_t sample_length = 0;
  int32_t frq_length = 0;
  double frame_ms = 0.0;
  double con_vel = 0.0;
  double offset = 0.0;
  double required_length = 0.0;
  double consonant = 0.0;
  double cut_off = 0.0;
  double volume = 0.0;
  int flag_P = 0;

  bool operator==(const PreparedCacheKey& other) const;
};

struct PreparedCacheKeyHash {
  size_t operator()(const PreparedCacheKey& k) const;
};

class PreparedCache {
 public:
  explicit PreparedCache(size_t max_entries = 512);

  std::shared_ptr<const PreparedModelData> Get(const PreparedCacheKey& key);
  void Put(const PreparedCacheKey& key,
           const std::shared_ptr<const PreparedModelData>& value);
  void Clear();
  size_t Size() const;

 private:
  size_t max_entries_;
  mutable std::mutex mutex_;
  std::list<PreparedCacheKey> lru_;
  std::unordered_map<
      PreparedCacheKey,
      std::pair<std::shared_ptr<const PreparedModelData>,
                std::list<PreparedCacheKey>::iterator>,
      PreparedCacheKeyHash>
      entries_;
};

struct StreamStats {
  double first_block_ms = 0.0;
  double total_ms = 0.0;
  double prep_wait_ms = 0.0;
  double backpressure_wait_ms = 0.0;
  int32_t prep_workers = 0;
  int32_t synth_workers = 1;
  int32_t peak_buffered_models = 0;
  int32_t peak_timeline_frames = 0;
  int32_t total_frames = 0;
  int32_t output_samples = 0;
  int32_t cache_hits = 0;
  int32_t cache_misses = 0;
};

class PhraseSynth {
 public:
  void SetPreparedCache(PreparedCache* cache) { prepared_cache_ = cache; }
  void AddRequest(const SynthRequest& request, double pos_ms, double skip_ms,
                  double length_ms, double fade_in_ms, double fade_out_ms,
                  LogCallback logCallback);
  void SetCurves(double* const f0, double* gender, double* tension,
                 double* breathiness, double* voicing, int length,
                 LogCallback logCallback);
  std::vector<double> Synth(LogCallback logCallback);
  int SynthStream(PcmStreamCallback callback, void* user_data,
                  int block_samples, LogCallback logCallback);
  int SynthStreamWithStats(PcmStreamCallback callback, void* user_data,
                           int block_samples, StreamStats* stats_out,
                           LogCallback logCallback);

 private:
  struct ModelTiming {
    int left_extra;
    int skip;
    int p0;
    int p1;
    int p3;
    int p4;
  };

  struct PendingRequest {
    SynthRequest request;
    std::vector<double> samples;
    std::vector<char> frq;
    std::vector<int> pitch_bend;
    double pos_ms;
    double skip_ms;
    double length_ms;
    double fade_in_ms;
    double fade_out_ms;
  };

  struct PreparedRequest {
    std::shared_ptr<const PreparedModelData> model;
    ModelTiming timing;
    bool cache_hit = false;
  };

  PreparedCache* prepared_cache_ = nullptr;
  std::vector<std::shared_ptr<const PreparedModelData>> models_;
  std::vector<ModelTiming> timings_;
  std::vector<PendingRequest> pending_requests_;
  std::chrono::nanoseconds enqueue_copy_time_{0};

  std::vector<double> f0_;
  std::vector<double> gender_;
  std::vector<double> tension_;
  std::vector<double> breathiness_;
  std::vector<double> voicing_;

  static ModelTiming ComputeTiming(const PendingRequest& pending,
                                   double frame_ms);
  static std::unique_ptr<PreparedRequest> PrepareSingleRequest(
      PendingRequest& pending, double frame_ms, PreparedCache* cache,
      std::atomic<long long>* analyze_cpu_ns,
      std::atomic<long long>* sp_cpu_ns, std::atomic<long long>* ap_cpu_ns,
      std::atomic<long long>* remap_cpu_ns);
  void PreparePendingRequests();
  std::vector<double> SynthInternal(int* output_length);
};

}  // namespace worldline

#endif  // WORLDLINE_PHRASE_SYNTH_H_
