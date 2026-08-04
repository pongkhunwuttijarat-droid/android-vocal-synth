//! Cumulative render counters, shared across handlers.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// Cumulative render counters (shared across handlers).
#[derive(Debug, Default)]
pub struct Stats {
    renders_count: AtomicU64,
    total_ms: AtomicU64,
    cache_hits: AtomicU64,
}

/// Point-in-time snapshot, serialized by `GET /stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StatsSnapshot {
    /// Successful renders (synth-note + render).
    pub renders_count: u64,
    /// Accumulated engine render time in ms.
    pub total_ms: u64,
    /// Reserved for a future phrase cache; nothing caches yet, so this
    /// stays 0.
    pub cache_hits: u64,
}

impl Stats {
    /// Record one successful render that took `elapsed_ms` of engine time.
    pub fn record_render(&self, elapsed_ms: u64) {
        self.renders_count.fetch_add(1, Ordering::Relaxed);
        self.total_ms.fetch_add(elapsed_ms, Ordering::Relaxed);
    }

    /// Record a cache hit (unused until a phrase cache exists).
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot the counters for `GET /stats`.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            renders_count: self.renders_count.load(Ordering::Relaxed),
            total_ms: self.total_ms.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_accumulate() {
        let stats = Stats::default();
        assert_eq!(
            stats.snapshot(),
            StatsSnapshot {
                renders_count: 0,
                total_ms: 0,
                cache_hits: 0
            }
        );
        stats.record_render(12);
        stats.record_render(8);
        stats.record_cache_hit();
        assert_eq!(
            stats.snapshot(),
            StatsSnapshot {
                renders_count: 2,
                total_ms: 20,
                cache_hits: 1
            }
        );
    }
}
