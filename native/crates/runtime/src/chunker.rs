//! Chunking of render inputs into hash-addressed batches.
//!
//! The renderer consumes work in *chunks*: groups of `chunk_size` phonemes
//! (or notes) with `overlap` trailing items carried into the next chunk so
//! the renderer has phrase-boundary context (coarticulation). Each chunk
//! gets a deterministic XXH64 hash over the canonical byte encoding of its
//! items — the cache key. The same input always produces the same chunk
//! layout and hashes, across runs, platforms and processes.

use std::io::{self, Write};
use std::marker::PhantomData;

use crate::hash::Xxh64;

/// A type that can stream a canonical, self-delimiting byte encoding of
/// itself for hashing.
///
/// Implementations must be deterministic and unambiguous: two values must
/// produce different byte streams, and concatenating encodings of multiple
/// values must be parseable back into the same sequence (variable-length
/// types therefore length-prefix themselves). Implemented for the common
/// primitives, `String`/`&str` and `Vec<T>`/`&[T]`; other render-input
/// types implement it by writing their fields in a fixed order.
///
/// The concrete encoding of an item may change between releases, which
/// changes chunk hashes and therefore invalidates the persistent render
/// cache — acceptable during development.
pub trait HashKey {
    /// Write this value's canonical bytes to `out`.
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()>;
}

/// A batch of work items plus its cache key.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderChunk<T> {
    /// Sequential chunk index (`0`, `1`, …), assigned in split order.
    pub id: u64,
    /// The items in this chunk (already includes overlap items).
    pub items: Vec<T>,
    /// XXH64 cache key over the items' canonical encoding.
    pub hash: u64,
}

/// Splits a slice of work items into overlapping chunks.
///
/// `chunk_size` is the maximum items per chunk, `overlap` the number of
/// trailing items of chunk *n* that are repeated as leading items of chunk
/// *n+1* (for phrase-boundary context). Overlap must be `< chunk_size`.
#[derive(Clone, Debug)]
pub struct Chunker<T> {
    chunk_size: usize,
    overlap: usize,
    marker: PhantomData<T>,
}

impl<T: Clone + HashKey> Chunker<T> {
    /// Create a chunker.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size == 0` or `overlap >= chunk_size`.
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        assert!(chunk_size > 0, "chunk_size must be >= 1");
        assert!(overlap < chunk_size, "overlap must be < chunk_size");
        Self {
            chunk_size,
            overlap,
            marker: PhantomData,
        }
    }

    /// Maximum items per chunk.
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Items shared between consecutive chunks.
    pub fn overlap(&self) -> usize {
        self.overlap
    }

    /// Number of chunks `split` would produce for `items_len` items.
    ///
    /// Useful for progress reporting (e.g. `progress = done / chunk_count`).
    pub fn chunk_count(&self, items_len: usize) -> usize {
        if items_len == 0 {
            0
        } else {
            let stride = self.chunk_size - self.overlap;
            (items_len - 1) / stride + 1
        }
    }

    /// Split `items` into chunks.
    ///
    /// Chunk *n* covers items `[n*stride, n*stride + chunk_size)` where
    /// `stride = chunk_size - overlap`; the final chunk may be shorter.
    /// Returns an empty vec for an empty input.
    pub fn split(&self, items: &[T]) -> Vec<RenderChunk<T>> {
        if items.is_empty() {
            return Vec::new();
        }
        let stride = self.chunk_size - self.overlap;
        let mut chunks = Vec::with_capacity(self.chunk_count(items.len()));
        let mut start = 0usize;
        let mut id = 0u64;
        while start < items.len() {
            let end = (start + self.chunk_size).min(items.len());
            let chunk_items = items[start..end].to_vec();
            let hash = Self::hash_items(&chunk_items);
            chunks.push(RenderChunk {
                id,
                items: chunk_items,
                hash,
            });
            id += 1;
            start += stride;
        }
        chunks
    }

    /// XXH64 over the canonical encoding of `items`.
    ///
    /// Encoding: `u64` little-endian item count, then each item's
    /// [`HashKey`] bytes. Stable across processes (seed `0`, no salt).
    /// Verified against `xxhsum -H1` in tests.
    pub fn hash_items(items: &[T]) -> u64 {
        let mut hasher = Xxh64::new(0);
        hasher.update(&(items.len() as u64).to_le_bytes());
        for item in items {
            // write_hash never fails: Xxh64's Write impl is infallible.
            let _ = item.write_hash(&mut hasher);
        }
        hasher.digest()
    }
}

macro_rules! impl_hashkey_le {
    ($($t:ty),* $(,)?) => {
        $(
            impl HashKey for $t {
                fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
                    out.write_all(&self.to_le_bytes())
                }
            }
        )*
    };
}

impl_hashkey_le!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
// usize is platform-width; hash as u64 so keys are portable.
impl HashKey for usize {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(&(*self as u64).to_le_bytes())
    }
}

impl HashKey for str {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(&(self.len() as u64).to_le_bytes())?;
        out.write_all(self.as_bytes())
    }
}

impl HashKey for String {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        self.as_str().write_hash(out)
    }
}

impl<T: HashKey> HashKey for Vec<T> {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        self.as_slice().write_hash(out)
    }
}

impl<T: HashKey> HashKey for [T] {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(&(self.len() as u64).to_le_bytes())?;
        for item in self {
            item.write_hash(out)?;
        }
        Ok(())
    }
}

impl<T: HashKey> HashKey for &T {
    fn write_hash(&self, out: &mut dyn Write) -> io::Result<()> {
        (*self).write_hash(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors verified against the system `xxhsum -H1` on byte sequences
    /// built with the exact encoding above:
    ///
    /// ```text
    /// $ python3 - <<'EOF'
    /// import struct
    /// def enc_strings(items):
    ///     b = struct.pack('<Q', len(items))
    ///     for s in items:
    ///         sb = s.encode(); b += struct.pack('<Q', len(sb)) + sb
    ///     return b
    /// open('vec.bin','wb').write(enc_strings(['a','b','hello']))
    /// EOF
    /// $ xxhsum -H1 vec.bin   # e852cbd24f232b1f
    /// ```
    #[test]
    fn hash_stability_verified_against_xxhsum() {
        let strings = vec!["a".to_string(), "b".to_string(), "hello".to_string()];
        assert_eq!(Chunker::<String>::hash_items(&strings), 0xe852_cbd2_4f23_2b1f);

        // u32 items: count LE + raw 4-byte LE values.
        let u32s = vec![1u32, 2, 3, 1000];
        assert_eq!(Chunker::<u32>::hash_items(&u32s), 0x4211_7d14_81de_c93e);

        // 4 x 10-byte strings = 48 bytes of payload: exercises the
        // >= 32-byte accumulation path.
        let four = vec!["x".repeat(10); 4];
        assert_eq!(Chunker::<String>::hash_items(&four), 0x5eec_985f_1721_3485);
    }

    #[test]
    fn hash_is_deterministic_and_order_sensitive() {
        let a = vec!["a".to_string(), "b".to_string()];
        let b = vec!["a".to_string(), "b".to_string()];
        let c = vec!["b".to_string(), "a".to_string()];
        assert_eq!(Chunker::<String>::hash_items(&a), Chunker::<String>::hash_items(&b));
        assert_ne!(Chunker::<String>::hash_items(&a), Chunker::<String>::hash_items(&c));
        // Different lengths must not collide either.
        let d = vec!["ab".to_string()];
        assert_ne!(Chunker::<String>::hash_items(&a), Chunker::<String>::hash_items(&d));
    }

    #[test]
    fn plain_partition_without_overlap() {
        let chunker = Chunker::<u32>::new(4, 0);
        let items: Vec<u32> = (0..10).collect();
        let chunks = chunker.split(&items);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].items, vec![0, 1, 2, 3]);
        assert_eq!(chunks[1].items, vec![4, 5, 6, 7]);
        assert_eq!(chunks[2].items, vec![8, 9]);
        assert_eq!(chunks[0].id, 0);
        assert_eq!(chunks[1].id, 1);
        assert_eq!(chunks[2].id, 2);
        // Every item appears in exactly one chunk.
        let total: usize = chunks.iter().map(|c| c.items.len()).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn overlap_carries_phrase_boundary_context() {
        let chunker = Chunker::<u32>::new(4, 1);
        let items: Vec<u32> = (0..10).collect();
        let chunks = chunker.split(&items);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].items, vec![0, 1, 2, 3]);
        assert_eq!(chunks[1].items, vec![3, 4, 5, 6]);
        assert_eq!(chunks[2].items, vec![6, 7, 8, 9]);
        assert_eq!(chunks[3].items, vec![9]);
        // First items of each chunk repeat the previous chunk's tail.
        assert_eq!(chunks[1].items[0], chunks[0].items[3]);
        assert_eq!(chunks[2].items[0], chunks[1].items[3]);
    }

    #[test]
    fn larger_overlap() {
        let chunker = Chunker::<u32>::new(5, 2);
        let items: Vec<u32> = (0..12).collect();
        let chunks = chunker.split(&items);
        assert_eq!(chunks[0].items, vec![0, 1, 2, 3, 4]);
        assert_eq!(chunks[1].items, vec![3, 4, 5, 6, 7]);
        assert_eq!(chunks[2].items, vec![6, 7, 8, 9, 10]);
        assert_eq!(chunks[3].items, vec![9, 10, 11]);
    }

    #[test]
    fn single_chunk_and_empty() {
        let chunker = Chunker::<u32>::new(8, 0);
        assert!(chunker.split(&[]).is_empty());
        let chunks = chunker.split(&[1, 2]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].items, vec![1, 2]);
        assert_eq!(chunks[0].hash, Chunker::<u32>::hash_items(&[1, 2]));
    }

    #[test]
    fn chunk_count_matches_split() {
        for len in 0..=17usize {
            for (size, overlap) in [(4usize, 1usize), (4, 0), (8, 2), (1, 0)] {
                let chunker = Chunker::<u32>::new(size, overlap);
                let items: Vec<u32> = (0..len as u32).collect();
                assert_eq!(
                    chunker.chunk_count(len),
                    chunker.split(&items).len(),
                    "len={len} size={size} overlap={overlap}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "chunk_size")]
    fn rejects_zero_chunk_size() {
        let _ = Chunker::<u32>::new(0, 0);
    }

    #[test]
    #[should_panic(expected = "overlap")]
    fn rejects_overlap_ge_chunk_size() {
        let _ = Chunker::<u32>::new(4, 4);
    }

    #[test]
    fn hashkey_encodings_are_self_delimiting() {
        // "ab" + "c" must not hash like "a" + "bc" (length prefixes).
        let ab_c = vec!["ab".to_string(), "c".to_string()];
        let a_bc = vec!["a".to_string(), "bc".to_string()];
        assert_ne!(
            Chunker::<String>::hash_items(&ab_c),
            Chunker::<String>::hash_items(&a_bc)
        );
        // Vec<T> and &[T] agree.
        let v: Vec<u32> = vec![1, 2, 3];
        assert_eq!(
            Chunker::<u32>::hash_items(&v),
            Chunker::<u32>::hash_items(v.as_slice())
        );
    }
}
