//! Seeded randomness for procedural generation (`rand` / `StdRng`).

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Mix seed components into a single `u64` for `StdRng`.
pub fn hash_seed(base: u64, a: u32, b: u32, salt: u32) -> u64 {
    let mut z = base
        .wrapping_add(a as u64)
        .wrapping_add((b as u64) << 32)
        .wrapping_add(salt as u64 * 0x9E37_79B9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub fn seed_to_i32(seed: u64, salt: u32) -> i32 {
    hash_seed(seed, salt, 0, 0x5EED) as i32
}

pub fn unit_float(seed: u64) -> f32 {
    StdRng::seed_from_u64(seed).gen::<f32>()
}

/// Deterministic per-hex roll in [0, 1).
pub fn tile_roll(seed: u64, q: i32, r: i32, salt: u32) -> f32 {
    unit_float(hash_seed(seed, q as u32, r as u32, salt))
}

/// Deterministic roll in [0, 1) keyed to (world seed, island id, salt).
pub fn island_roll(seed: u64, island: u8, salt: u32) -> f32 {
    tile_roll(seed, island as i32, salt as i32, 0xC0E5C1)
}

/// Deterministic index in 0..len keyed to (world seed, island id, salt).
pub fn pick_index(seed: u64, island: u8, salt: u32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (island_roll(seed, island, salt) * len as f32) as usize % len
}

