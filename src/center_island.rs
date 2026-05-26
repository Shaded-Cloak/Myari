//! Base-layer terrain for the central Old Empire landmass only.
//!
//! Large organic zones of Cinderfield and Rootfield — same noise/threshold
//! approach as crescent base biomes, without mountains, forests, lakes, or
//! special tiles.

use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};
use glam::Vec2;
use hexx::{Hex, HexLayout, HexOrientation};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::hexgrid::HexCoord;
use crate::map::TerrainType;
use crate::rng::{hash_seed, seed_to_i32};

/// Must match [`crate::outer_islands::CENTER_ISLAND_ID`] for lake/terrain routing.
pub const CENTER_ISLAND_ID: u8 = 3;

const ZONE_FREQ: f32 = 0.0060;
const WOBBLE_FREQ: f32 = 0.0180;
const DOMAIN_WARP_FREQ: f32 = 0.0080;
const DOMAIN_WARP_AMP: f32 = 38.0;

/// Split between Cinderfield (below) and Rootfield (above), with wobble on the
/// boundary for an organic edge. 0.50 ≈ even coverage after per-island z-norm.
const ROOTFIELD_BAND: f32 = 0.50;

const TARGET_STDEV: f32 = 0.15;

struct Fields {
    zone: f32,
    wobble: f32,
}

struct CenterNoise {
    zone: FastNoiseLite,
    wobble: FastNoiseLite,
    warp: FastNoiseLite,
}

impl CenterNoise {
    fn new(world_seed: u64) -> Self {
        let seed = hash_seed(world_seed, CENTER_ISLAND_ID as u32, 0xC3_17, 0xB4);
        Self {
            zone: make_fbm(seed, 10, 3, ZONE_FREQ),
            wobble: make_fbm(seed, 11, 2, WOBBLE_FREQ),
            warp: make_domain_warp(seed, 12, DOMAIN_WARP_AMP, DOMAIN_WARP_FREQ),
        }
    }

    fn sample(&self, lx: f32, ly: f32) -> Fields {
        let (wx, wy) = self.warp.domain_warp_2d(lx, ly);
        Fields {
            zone: unit01(&self.zone, wx, wy),
            wobble: unit01(&self.wobble, wx + 113.7, wy + 73.1) - 0.5,
        }
    }
}

fn unit01(gen: &FastNoiseLite, x: f32, y: f32) -> f32 {
    ((gen.get_noise_2d(x, y) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn make_fbm(seed: u64, salt: u32, octaves: i32, frequency: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(seed_to_i32(seed, salt));
    f.set_noise_type(Some(NoiseType::OpenSimplex2));
    f.set_fractal_type(Some(FractalType::FBm));
    f.set_fractal_octaves(Some(octaves));
    f.set_frequency(Some(frequency));
    f.set_fractal_lacunarity(Some(2.0));
    f.set_fractal_gain(Some(0.5));
    f
}

fn make_domain_warp(seed: u64, salt: u32, amp: f32, frequency: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(seed_to_i32(seed, salt));
    f.set_domain_warp_type(Some(DomainWarpType::OpenSimplex2));
    f.set_domain_warp_amp(Some(amp));
    f.set_frequency(Some(frequency));
    f
}

fn flat_layout(hex_size: f32) -> HexLayout {
    HexLayout {
        orientation: HexOrientation::Flat,
        origin: hexx::Vec2::ZERO,
        hex_size: hexx::Vec2::new(hex_size, hex_size),
        invert_x: false,
        invert_y: false,
    }
}

fn layout_xy(q: i32, r: i32) -> Vec2 {
    let p = flat_layout(1.0).hex_to_world_pos(Hex::new(q, r));
    Vec2::new(p.x, p.y)
}

fn znorm(value: f32, mean: f32, stdev: f32) -> f32 {
    let z = (value - mean) / stdev.max(1e-3);
    (z * TARGET_STDEV + 0.5).clamp(0.0, 1.0)
}

fn base_terrain(zone_score: f32) -> TerrainType {
    if zone_score < ROOTFIELD_BAND {
        TerrainType::Cinderfield
    } else {
        TerrainType::Rootfield
    }
}

/// Classify every supplied center land tile (beach and water are excluded by the caller).
pub fn classify(world_seed: u64, land: &[HexCoord]) -> FxHashMap<HexCoord, TerrainType> {
    if land.is_empty() {
        return FxHashMap::default();
    }

    let noise = CenterNoise::new(world_seed);

    let raw: Vec<(HexCoord, Fields)> = land
        .par_iter()
        .map(|&coord| {
            let p = layout_xy(coord.q, coord.r);
            (coord, noise.sample(p.x, p.y))
        })
        .collect();

    let n = raw.len().max(1) as f32;
    let mut z_sum = 0.0;
    let mut z_sq = 0.0;
    for (_, f) in &raw {
        z_sum += f.zone;
        z_sq += f.zone * f.zone;
    }
    let z_mean = z_sum / n;
    let z_std = ((z_sq / n - z_mean * z_mean).max(0.0)).sqrt();

    let mut out: FxHashMap<HexCoord, TerrainType> =
        FxHashMap::with_capacity_and_hasher(land.len(), Default::default());
    for (coord, f) in raw {
        let zone = znorm(f.zone, z_mean, z_std);
        let score = zone + f.wobble * 0.04;
        out.insert(coord, base_terrain(score));
    }
    out
}
