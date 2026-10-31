//! Hashing shared by the build script and the static lookup tables.

/// FNV-1a over the key's bytes, finalized with a multiplicative mix so the
/// high bits used for slot selection are well distributed.
pub fn hash(key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in key.as_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// First slot probed for `key` in a power-of-two table of `len` slots.
pub fn slot(key: &str, len: usize) -> usize {
    (hash(key) >> (64 - len.trailing_zeros())) as usize
}

/// Marks an empty slot.
pub const EMPTY: u32 = u32::MAX;
