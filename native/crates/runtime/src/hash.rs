//! XXH64 hashing for render-cache keys.
//!
//! Thin wrapper around the `xxhash-rust` crate that additionally implements
//! [`std::io::Write`], so any type can stream its canonical byte encoding
//! into the hasher (this is what [`crate::chunker::HashKey`] builds on).
//!
//! XXH64 is the same algorithm OpenUtau uses for its `res-{hash}` render
//! cache; it is fast, deterministic across platforms/processes (no random
//! seed), and stable across program versions — exactly what a persistent
//! file cache key needs.

use std::io::{self, Write};

/// Streaming XXH64 hasher with a [`Write`] adapter.
///
/// ```
/// use std::io::Write;
/// use runtime::hash::Xxh64;
///
/// let mut h = Xxh64::new(0);
/// h.write_all(b"abc").unwrap();
/// assert_eq!(h.digest(), 0x44bc_2cf5_ad77_0999);
/// ```
pub struct Xxh64 {
    inner: xxhash_rust::xxh64::Xxh64,
}

impl Xxh64 {
    /// Create a hasher with `seed` (use `0` for cache keys).
    pub fn new(seed: u64) -> Self {
        Self {
            inner: xxhash_rust::xxh64::Xxh64::new(seed),
        }
    }

    /// Feed `bytes` into the hash state.
    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// Finalize and return the 64-bit digest. The hasher can keep being
    /// updated after this call; the digest reflects all bytes seen so far.
    pub fn digest(&self) -> u64 {
        self.inner.digest()
    }
}

impl Write for Xxh64 {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One-shot XXH64 of `data` with `seed`.
pub fn xxh64(data: &[u8], seed: u64) -> u64 {
    xxhash_rust::xxh64::xxh64(data, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors verified against the system `xxhsum -H1` (xxHash 0.8.x):
    ///
    /// ```text
    /// $ printf ''                                | xxhsum -H1   # ef46db3751d8e999
    /// $ printf 'abc'                             | xxhsum -H1   # 44bc2cf5ad770999
    /// $ printf 'abcdefghijklmnopqrstuvwxyz0123456789' | xxhsum -H1  # 64f23ecf1609b766
    /// $ head -c 100 /dev/zero | tr '\0' x        | xxhsum -H1   # 92f0de5a88a3c094
    /// ```
    #[test]
    fn official_vectors() {
        assert_eq!(xxh64(b"", 0), 0xef46_db37_51d8_e999);
        assert_eq!(xxh64(b"abc", 0), 0x44bc_2cf5_ad77_0999);
        // 32..63 bytes: exercises the seeded v1..v4 accumulation path.
        assert_eq!(
            xxh64(b"abcdefghijklmnopqrstuvwxyz0123456789", 0),
            0x64f2_3ecf_1609_b766
        );
        // >= 64 bytes: exercises multi-stripe accumulation.
        assert_eq!(xxh64(&[b'x'; 100], 0), 0x92f0_de5a_88a3_c094);
    }

    #[test]
    fn streaming_matches_oneshot() {
        let data = b"the quick brown fox jumps over the lazy dog, twice!";
        let mut h = Xxh64::new(0);
        h.update(data);
        assert_eq!(h.digest(), xxh64(data, 0));

        // Splitting the input at every possible boundary must not change
        // the digest (the whole point of a streaming hash).
        for split in 0..=data.len() {
            let mut h = Xxh64::new(0);
            h.update(&data[..split]);
            h.update(&data[split..]);
            assert_eq!(h.digest(), xxh64(data, 0), "split at {split}");
        }
    }

    #[test]
    fn write_adapter_feeds_bytes() {
        use std::io::Write;
        let mut h = Xxh64::new(0);
        h.write_all(b"ab").unwrap();
        h.write_all(b"c").unwrap();
        assert_eq!(h.digest(), xxh64(b"abc", 0));
        // Flush is a no-op but must not error.
        h.flush().unwrap();
    }

    #[test]
    fn seed_changes_digest() {
        assert_ne!(xxh64(b"abc", 0), xxh64(b"abc", 1));
    }
}
