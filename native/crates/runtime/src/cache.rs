//! Hash-keyed render cache: in-memory LRU + disk persistence.
//!
//! Mirrors OpenUtau's `RenderCache` (LRU over `uint hash -> byte[]`) and
//! its `res-{hash}` disk pattern, with two differences: capacity is counted
//! in *bytes* (not entries), and the backing store is the [`storage`]
//! crate's [`Storage`] trait so the same code runs on desktop (`FsStorage`)
//! and Android (`JniStorage`).
//!
//! Disk layout: `{storage root}/cache/res-{key:016x}.bin` containing
//! `"RCA1"` magic, a `u64` little-endian sample count and raw `f32`
//! little-endian samples. A cache miss in memory falls back to disk, so a
//! freshly started process still benefits from previous sessions' renders.
//!
//! LRU eviction only drops *memory* entries — evicted chunks keep their
//! disk files (that is the point of a persistent render cache). Use
//! [`RenderCache::invalidate`] to drop a chunk from memory *and* disk.
//!
//! Not thread-safe by itself; wrap in a `Mutex` when shared across
//! workers.

use std::collections::{HashMap, VecDeque};
use std::io;

use storage::{FsStorage, Storage};

/// File magic, "RCA1" (render cache audio, v1).
const MAGIC: &[u8; 4] = b"RCA1";

#[derive(Debug)]
struct Entry {
    data: Vec<f32>,
    size_bytes: usize,
}

/// A file-backed, byte-capped LRU cache of mono `f32` audio keyed by
/// XXH64 hash.
#[derive(Debug)]
pub struct RenderCache<S: Storage = FsStorage> {
    storage: S,
    capacity_bytes: usize,
    entries: HashMap<u64, Entry>,
    /// LRU order: front = least recently used (evicted first),
    /// back = most recently used.
    order: VecDeque<u64>,
    memory_bytes: usize,
}

impl<S: Storage> RenderCache<S> {
    /// Create a cache over `storage` holding at most `capacity_bytes` of
    /// audio in memory. When the capacity is exceeded, the least recently
    /// used entries are evicted *before* the new entry is inserted (like
    /// OpenUtau's evict-then-add); a single entry larger than the capacity
    /// is retained, so memory may temporarily exceed the budget. A
    /// `capacity_bytes` of `0` keeps only the most recently inserted
    /// chunk in memory; disk is always consulted on miss.
    pub fn new(storage: S, capacity_bytes: usize) -> Self {
        Self {
            storage,
            capacity_bytes,
            entries: HashMap::new(),
            order: VecDeque::new(),
            memory_bytes: 0,
        }
    }

    /// Fetch a chunk: memory first (LRU-touched), then disk.
    ///
    /// Disk read errors and corrupt files are treated as misses (a cache
    /// must never break rendering).
    pub fn get(&mut self, key: u64) -> Option<Vec<f32>> {
        if let Some(entry) = self.entries.get(&key) {
            let data = entry.data.clone();
            self.touch(key);
            return Some(data);
        }
        let bytes = self.storage.read_bytes(&self.disk_logical(key)).ok()?;
        let data = decode(&bytes)?;
        self.insert_memory(key, data.clone());
        Some(data)
    }

    /// Store a chunk: written to disk first, then inserted into the LRU
    /// (evicting least-recently-used entries as needed).
    ///
    /// # Errors
    ///
    /// Returns the underlying IO error if the disk write fails; the
    /// memory cache is left untouched in that case.
    pub fn put(&mut self, key: u64, data: Vec<f32>) -> io::Result<()> {
        self.storage.write_bytes(&self.disk_logical(key), &encode(&data))?;
        self.insert_memory(key, data);
        Ok(())
    }

    /// Fetch a chunk, computing and storing it on miss.
    pub fn get_or_compute<F>(&mut self, key: u64, compute: F) -> Vec<f32>
    where
        F: FnOnce() -> Vec<f32>,
    {
        if let Some(data) = self.get(key) {
            return data;
        }
        let data = compute();
        // A failed disk write must not break rendering: keep the data in
        // memory only.
        let _ = self
            .storage
            .write_bytes(&self.disk_logical(key), &encode(&data));
        self.insert_memory(key, data.clone());
        data
    }

    /// Whether `key` is currently in the memory cache (does not touch
    /// LRU order; does not consult disk).
    pub fn contains(&self, key: u64) -> bool {
        self.entries.contains_key(&key)
    }

    /// Number of chunks currently in memory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the memory cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes of audio currently held in memory.
    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    /// The byte capacity configured at construction.
    pub fn capacity_bytes(&self) -> usize {
        self.capacity_bytes
    }

    /// Drop a chunk from memory and best-effort delete its disk file.
    ///
    /// Returns `true` if the key was known (in memory or on disk).
    pub fn invalidate(&mut self, key: u64) -> bool {
        let in_memory = self.remove_memory(key);
        let logical = self.disk_logical(key);
        let on_disk = self.storage.exists(&logical);
        if on_disk {
            // The Storage trait has no delete method; remove the file via
            // its resolved absolute path (best-effort).
            let _ = std::fs::remove_file(self.storage.resolve(&logical));
        }
        in_memory || on_disk
    }

    /// Drop all memory entries (disk files are kept for future sessions).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.memory_bytes = 0;
    }

    /// Absolute path of the disk file for `key`.
    pub fn disk_path(&self, key: u64) -> std::path::PathBuf {
        self.storage.cache_path(&file_name(key))
    }

    fn disk_logical(&self, key: u64) -> String {
        format!("cache/{}", file_name(key))
    }

    fn touch(&mut self, key: u64) {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
            self.order.push_back(key);
        }
    }

    fn insert_memory(&mut self, key: u64, data: Vec<f32>) {
        let size_bytes = data.len() * std::mem::size_of::<f32>();
        if let Some(existing) = self.entries.get_mut(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(existing.size_bytes);
            existing.data = data;
            existing.size_bytes = size_bytes;
            self.memory_bytes += size_bytes;
            self.touch(key);
            return;
        }
        // Make room before inserting, like OpenUtau's evict-then-add. A
        // single entry larger than the capacity is retained even if the
        // byte budget is then exceeded (until it is evicted or
        // invalidated).
        while self.memory_bytes > 0 && self.memory_bytes + size_bytes > self.capacity_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.memory_bytes = self.memory_bytes.saturating_sub(entry.size_bytes);
            }
        }
        self.entries.insert(key, Entry { data, size_bytes });
        self.memory_bytes += size_bytes;
        self.order.push_back(key);
        self.touch(key);
    }

    fn remove_memory(&mut self, key: u64) -> bool {
        if let Some(entry) = self.entries.remove(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(entry.size_bytes);
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
            true
        } else {
            false
        }
    }
}

impl RenderCache<FsStorage> {
    /// Convenience constructor backed by `FsStorage` rooted at `dir`
    /// (e.g. the app's cache directory).
    pub fn new_in_dir(dir: impl Into<std::path::PathBuf>, capacity_bytes: usize) -> Self {
        Self::new(FsStorage::new(dir), capacity_bytes)
    }
}

/// `res-{hash:016x}.bin` — same naming scheme as OpenUtau's `res-{hash}.wav`.
fn file_name(key: u64) -> String {
    format!("res-{key:016x}.bin")
}

/// Encode audio as `MAGIC || count_le_u64 || f32_le * count`.
fn encode(data: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 8 + data.len() * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    for sample in data {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Decode [`encode`] output; `None` for unknown magic or length mismatch.
fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() < 4 + 8 || &bytes[..4] != MAGIC {
        return None;
    }
    let count = u64::from_le_bytes(bytes[4..12].try_into().ok()?) as usize;
    let expected = 4 + 8 + count * std::mem::size_of::<f32>();
    if bytes.len() != expected {
        return None;
    }
    let mut data = Vec::with_capacity(count);
    for chunk in bytes[12..].chunks_exact(4) {
        data.push(f32::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique scratch directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "runtime-cache-test-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn audio(n: usize) -> Vec<f32> {
        (0..n).map(|i| i as f32 * 0.5).collect()
    }

    #[test]
    fn put_get_roundtrip_in_memory() {
        let tmp = TempDir::new("mem");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        let data = audio(100);
        cache.put(42, data.clone()).unwrap();
        assert!(cache.contains(42));
        assert_eq!(cache.get(42), Some(data));
        assert_eq!(cache.memory_bytes(), 100 * 4);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(999), None);
    }

    #[test]
    fn disk_persistence_across_instances() {
        let tmp = TempDir::new("disk");
        let key = 0xdead_beef;
        {
            let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
            cache.put(key, audio(64)).unwrap();
            assert!(cache.disk_path(key).exists());
        }
        // A fresh cache instance (new process) hits the disk file.
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        assert!(!cache.contains(key));
        assert_eq!(cache.get(key), Some(audio(64)));
        assert!(cache.contains(key));
    }

    #[test]
    fn lru_eviction_by_bytes() {
        let tmp = TempDir::new("lru");
        // Capacity: exactly two 100-sample chunks (800 bytes).
        let mut cache = RenderCache::new_in_dir(tmp.path(), 800);
        cache.put(1, audio(100)).unwrap();
        cache.put(2, audio(100)).unwrap();
        assert_eq!(cache.len(), 2);
        // Third insert evicts the least recently used (1).
        cache.put(3, audio(100)).unwrap();
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains(1));
        assert!(cache.contains(2));
        assert!(cache.contains(3));
        assert_eq!(cache.memory_bytes(), 800);
    }

    #[test]
    fn get_touches_lru_order() {
        let tmp = TempDir::new("touch");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 800);
        cache.put(1, audio(100)).unwrap();
        cache.put(2, audio(100)).unwrap();
        // Touch 1 so 2 becomes the oldest.
        let _ = cache.get(1);
        cache.put(3, audio(100)).unwrap();
        assert!(cache.contains(1), "recently used must survive eviction");
        assert!(!cache.contains(2), "oldest must be evicted");
        assert!(cache.contains(3));
    }

    #[test]
    fn oversize_entry_evicts_everything_but_itself() {
        let tmp = TempDir::new("oversize");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 800);
        cache.put(1, audio(100)).unwrap();
        cache.put(2, audio(100)).unwrap();
        cache.put(3, audio(500)).unwrap(); // 2000 bytes > capacity
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(3));
    }

    #[test]
    fn put_same_key_updates_in_place() {
        let tmp = TempDir::new("update");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        cache.put(7, audio(10)).unwrap();
        cache.put(7, audio(20)).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(7), Some(audio(20)));
        assert_eq!(cache.memory_bytes(), 80);
    }

    #[test]
    fn invalidate_removes_memory_and_disk() {
        let tmp = TempDir::new("invalidate");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        cache.put(9, audio(16)).unwrap();
        assert!(cache.disk_path(9).exists());
        assert!(cache.invalidate(9));
        assert!(!cache.contains(9));
        assert!(!cache.disk_path(9).exists());
        assert_eq!(cache.get(9), None);
        // Unknown key: no-op.
        assert!(!cache.invalidate(12345));
    }

    #[test]
    fn clear_keeps_disk_files() {
        let tmp = TempDir::new("clear");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        cache.put(1, audio(8)).unwrap();
        cache.put(2, audio(8)).unwrap();
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.memory_bytes(), 0);
        // Disk files survive, so a later get still resolves them.
        assert_eq!(cache.get(1), Some(audio(8)));
    }

    #[test]
    fn corrupt_or_foreign_file_is_a_miss() {
        let tmp = TempDir::new("corrupt");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        // Garbage with the right name.
        std::fs::create_dir_all(cache.disk_path(1).parent().unwrap()).unwrap();
        std::fs::write(cache.disk_path(1), b"not audio at all").unwrap();
        assert_eq!(cache.get(1), None);
        // Valid magic but truncated payload.
        let mut bad = encode(&audio(4));
        bad.truncate(bad.len() - 2);
        std::fs::write(cache.disk_path(2), &bad).unwrap();
        assert_eq!(cache.get(2), None);
    }

    #[test]
    fn zero_capacity_keeps_only_most_recent_in_memory() {
        let tmp = TempDir::new("zerocap");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 0);
        cache.put(5, audio(4)).unwrap();
        assert_eq!(cache.len(), 1);
        cache.put(6, audio(4)).unwrap();
        // Inserting 6 evicted 5 (memory), but 5 is still on disk.
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(6));
        assert!(!cache.contains(5));
        assert_eq!(cache.get(5), Some(audio(4)), "disk fallback still works");
        assert!(cache.contains(5));
    }

    #[test]
    fn get_or_compute_uses_cache() {
        let tmp = TempDir::new("goc");
        let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
        let mut computed = 0;
        let data = cache.get_or_compute(3, || {
            computed += 1;
            audio(8)
        });
        assert_eq!(data, audio(8));
        let again = cache.get_or_compute(3, || {
            computed += 1;
            audio(8)
        });
        assert_eq!(again, audio(8));
        assert_eq!(computed, 1, "second call must hit the cache");
    }

    #[test]
    fn encode_decode_roundtrip_and_validation() {
        let data = audio(257); // > 256 samples: multi-byte count
        let encoded = encode(&data);
        assert_eq!(decode(&encoded), Some(data.clone()));
        assert_eq!(decode(b""), None);
        assert_eq!(decode(b"XXXX00000000"), None);
        // Wrong magic.
        let mut bytes = encoded;
        bytes[0] = b'X';
        assert_eq!(decode(&bytes), None);
    }

    #[test]
    fn key_files_use_hex_res_pattern() {
        let tmp = TempDir::new("name");
        let cache = RenderCache::new_in_dir(tmp.path(), 100);
        assert_eq!(cache.disk_path(0xabc), tmp.path().join("cache/res-0000000000000abc.bin"));
    }
}
