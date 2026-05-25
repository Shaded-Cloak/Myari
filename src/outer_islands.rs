//! Noise-driven terrain classifier for the three crescent outer islands.
//!
//! Every decision is a soft, wobble-perturbed threshold on smooth, domain
//! warped noise fields. There is no patch growing, no fixed seeding, and no
//! minimum-component enforcement — the natural look comes from the noise
//! itself plus per-tile threshold wobble.
//!
//! Each crescent uses the full allowed palette. Personality presets only
//! introduce small biases on heat and moisture and small phase offsets on the
//! zone / forest sampling positions. Three presets are assigned across the
//! three islands as a permutation, so no two islands share a preset.

use std::collections::VecDeque;

use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};
use glam::Vec2;
use hexx::{Hex, HexLayout, HexOrientation};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::hexgrid::HexCoord;
use crate::map::TerrainType;
use crate::rng::{hash_seed, pick_index, seed_to_i32};

const CRESCENT_ANGLES: [f32; 3] = [90.0, 210.0, 330.0];

/// Minimum BFS distance from the coast a tile must have before mountains can
/// form on it. Beach + the next two rings are always non-mountain.
const MOUNTAIN_MIN_INLAND: i32 = 3;

// Frequencies tuned so each island's footprint covers at least one full
// oscillation of every macro-scale field. Lower-frequency fields would let one
// crescent sit in a "wet half" of the noise wave while another sits in the
// "dry half", which is what produced large per-island forest coverage
// imbalances in earlier iterations.
const ELEVATION_LOW_FREQ: f32 = 0.0050;
const ELEVATION_RIDGE_FREQ: f32 = 0.0140;
const MOISTURE_FREQ: f32 = 0.0120;
const HEAT_FREQ: f32 = 0.0055;
const ZONE_FREQ: f32 = 0.0060;
const WOBBLE_FREQ: f32 = 0.0180;
const DOMAIN_WARP_FREQ: f32 = 0.0080;
const DOMAIN_WARP_AMP: f32 = 38.0;

// Single soft band splitting base-biome tiles between Plains (below) and
// Greenfield (above). `zone` is z-score normalized to N(0.5, 0.15) and the
// `heat * 0.3` contribution adds ~0.15 to the mean, so zone_score sits near
// ~0.65 on average — putting the band at 0.66 gives roughly 60 % Plains /
// 40 % Greenfield. Steppe and AridPeak are intentionally disabled for now.
const PLAINS_BAND: f32 = 0.66;

#[derive(Debug, Clone, Copy)]
struct Personality {
    /// Shifts forest type bands and base biome warmth. Does not directly
    /// affect coverage.
    heat_bias: f32,
    /// Target forest fraction of land tiles. Differing values are what give
    /// each personality its "wetter / drier" feel while keeping the spread
    /// inside the contract (max 4 pp across the three personalities).
    forest_target: f32,
    zone_phase: (f32, f32),
    forest_phase: (f32, f32),
}

const PRESETS: [Personality; 3] = [
    // A — Mildtemperate
    Personality {
        heat_bias: 0.00,
        forest_target: 0.22,
        zone_phase: (0.0, 0.0),
        forest_phase: (0.0, 0.0),
    },
    // B — Cooler-Wetter (slightly more forest)
    Personality {
        heat_bias: -0.06,
        forest_target: 0.235,
        zone_phase: (137.0, -61.0),
        forest_phase: (-93.0, 211.0),
    },
    // C — Warmer-Drier (slightly less forest)
    Personality {
        heat_bias: 0.06,
        forest_target: 0.205,
        zone_phase: (-204.0, 145.0),
        forest_phase: (79.0, -188.0),
    },
];

/// Pre-dilation. The post-pass adds one ring of StonySlope around every kept
/// mountain core, so effective coverage roughly doubles after cleanup.
const MOUNTAIN_UPPER_TARGET: f32 = 0.010;
const MOUNTAIN_LOWER_TARGET: f32 = 0.05;

/// Minimum number of tiles a forest patch must have to survive cleanup.
/// Anything smaller gets reverted to its natural base biome — kills the 1-5
/// tile speckle the noise threshold occasionally produces without touching
/// any real forest.
const MIN_FOREST_PATCH_SIZE: usize = 6;

const PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// Deterministically assigns one of three personality presets to each of the
/// three crescents such that no two islands share a preset.
fn assign_personalities(world_seed: u64) -> [Personality; 3] {
    let perm = PERMUTATIONS[pick_index(world_seed, 99, 7, PERMUTATIONS.len())];
    [PRESETS[perm[0]], PRESETS[perm[1]], PRESETS[perm[2]]]
}

#[derive(Debug, Clone, Copy)]
struct Fields {
    elevation_low: f32,
    elevation_ridge: f32,
    moisture: f32,
    heat: f32,
    zone: f32,
    /// Signed in roughly [-0.5, 0.5]. Added at small amplitude to scores and
    /// thresholds to break the otherwise too-clean iso-contour edges.
    wobble: f32,
}

struct IslandNoise {
    elevation_low: FastNoiseLite,
    elevation_ridge: FastNoiseLite,
    moisture: FastNoiseLite,
    heat: FastNoiseLite,
    zone: FastNoiseLite,
    wobble: FastNoiseLite,
    warp: FastNoiseLite,
}

impl IslandNoise {
    fn for_island(world_seed: u64, island_id: u8) -> Self {
        let seed = hash_seed(world_seed, island_id as u32, 0x15_1A, 0xC1);
        Self {
            elevation_low: make_fbm(seed, 10, 4, ELEVATION_LOW_FREQ),
            elevation_ridge: make_ridge(seed, 11, 3, ELEVATION_RIDGE_FREQ),
            moisture: make_fbm(seed, 12, 3, MOISTURE_FREQ),
            heat: make_fbm(seed, 13, 3, HEAT_FREQ),
            zone: make_fbm(seed, 14, 3, ZONE_FREQ),
            wobble: make_fbm(seed, 15, 2, WOBBLE_FREQ),
            warp: make_domain_warp(seed, 16, DOMAIN_WARP_AMP, DOMAIN_WARP_FREQ),
        }
    }

    fn sample(&self, lx: f32, ly: f32, p: &Personality) -> Fields {
        let (wx, wy) = self.warp.domain_warp_2d(lx, ly);
        let zone_x = wx + p.zone_phase.0;
        let zone_y = wy + p.zone_phase.1;
        let forest_x = wx + p.forest_phase.0;
        let forest_y = wy + p.forest_phase.1;
        Fields {
            elevation_low: unit01(&self.elevation_low, wx, wy),
            elevation_ridge: unit01(&self.elevation_ridge, wx, wy),
            moisture: unit01(&self.moisture, forest_x, forest_y),
            heat: unit01(&self.heat, wx + 31.4, wy - 17.6),
            zone: unit01(&self.zone, zone_x, zone_y),
            wobble: unit01(&self.wobble, wx + 113.7, wy + 73.1) - 0.5,
        }
    }
}

fn unit01(gen: &FastNoiseLite, x: f32, y: f32) -> f32 {
    ((gen.get_noise_2d(x, y) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn make_fbm(seed: u64, salt: u32, octaves: i32, freq: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(seed_to_i32(seed, salt));
    f.set_noise_type(Some(NoiseType::OpenSimplex2));
    f.set_fractal_type(Some(FractalType::FBm));
    f.set_fractal_octaves(Some(octaves));
    f.set_frequency(Some(freq));
    f.set_fractal_lacunarity(Some(2.0));
    f.set_fractal_gain(Some(0.5));
    f
}

fn make_ridge(seed: u64, salt: u32, octaves: i32, freq: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(seed_to_i32(seed, salt));
    f.set_noise_type(Some(NoiseType::OpenSimplex2));
    f.set_fractal_type(Some(FractalType::Ridged));
    f.set_fractal_octaves(Some(octaves));
    f.set_frequency(Some(freq));
    f.set_fractal_lacunarity(Some(2.0));
    f.set_fractal_gain(Some(0.5));
    f
}

fn make_domain_warp(seed: u64, salt: u32, amp: f32, freq: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(seed_to_i32(seed, salt));
    f.set_domain_warp_type(Some(DomainWarpType::OpenSimplex2));
    f.set_domain_warp_amp(Some(amp));
    f.set_frequency(Some(freq));
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

/// Per-island local frame rotated so the crescent's radial axis aligns with
/// the local Y axis. Sampling in this frame keeps noise patterns consistent
/// relative to each island's shape regardless of where it sits on the map.
fn local_frame(coord: HexCoord, island_id: u8) -> (f32, f32) {
    let pos = layout_xy(coord.q, coord.r);
    let theta = CRESCENT_ANGLES[island_id as usize].to_radians();
    let lx = pos.x * theta.cos() + pos.y * theta.sin();
    let ly = -pos.x * theta.sin() + pos.y * theta.cos();
    (lx, ly)
}

fn coast_penalty(inland: i32) -> f32 {
    match inland {
        0 => 0.40,
        1 => 0.25,
        2 => 0.12,
        _ => 0.0,
    }
}

/// Mountain "elevation score": ridge fractal still dominates so mountains
/// form along branching ridge lines, but the broad-mass `elevation_low` term
/// is larger now so the score has a wider "shoulder" around each ridge peak
/// — that's what gives StonySlope flanks visible thickness instead of a
/// single-tile-wide spine. Wobble breaks the iso-contour so range edges
/// stay ragged.
fn mountain_score(fields: &Fields) -> f32 {
    fields.elevation_ridge * 0.55 + fields.elevation_low * 0.45 + fields.wobble * 0.05
}

/// Forest score: higher moisture and lower elevation favour forest, with a
/// coast penalty that suppresses forests right behind the beach. Wobble breaks
/// the contour. Returns a raw signed score; comparison is against a per-island
/// percentile threshold.
fn forest_score(fields: &Fields, inland: i32) -> f32 {
    fields.moisture
        - fields.elevation_low * 0.30
        - coast_penalty(inland)
        + fields.wobble * 0.05
}

/// Pick the score value such that exactly `top_frac` of the slice is >= it.
/// Returns `+inf` for an empty slice (meaning "no tiles qualify").
fn percentile_top(scores: &[f32], top_frac: f32) -> f32 {
    if scores.is_empty() {
        return f32::INFINITY;
    }
    let mut sorted: Vec<f32> = scores.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let raw = (sorted.len() as f32 * top_frac).floor() as usize;
    let idx = raw.min(sorted.len() - 1);
    sorted[idx]
}

fn upper_mountain(_fields: &Fields, _p: &Personality) -> TerrainType {
    // Upper-tier mountains are always SnowPeak — AridPeak stays disabled.
    TerrainType::SnowPeak
}

fn lower_mountain(_fields: &Fields) -> TerrainType {
    // Lower-tier mountains are always StonySlope — Hills stays disabled.
    TerrainType::StonySlope
}

fn forest_type(fields: &Fields, p: &Personality) -> TerrainType {
    let biased_heat = (fields.heat + p.heat_bias).clamp(0.0, 1.0);
    if biased_heat < 0.38 {
        TerrainType::Darkpine
    } else if biased_heat < 0.66 {
        TerrainType::Oldwood
    } else {
        TerrainType::Deepjungle
    }
}

fn base_biome(fields: &Fields, p: &Personality) -> TerrainType {
    // Steppe intentionally disabled — base biomes split between Plains (drier)
    // and Greenfield (wetter) only, with personality `heat_bias` shifting the
    // line subtly so each crescent leans differently.
    let zone_score = fields.zone + (fields.heat + p.heat_bias) * 0.3 + fields.wobble * 0.04;
    if zone_score < PLAINS_BAND {
        TerrainType::Plains
    } else {
        TerrainType::Greenfield
    }
}

/// Target stdev for z-score-normalized fields. Roughly matches the natural
/// spread of FBM noise mapped to [0, 1].
const TARGET_STDEV: f32 = 0.15;

/// Z-score-normalize a raw value to the target distribution centred on 0.5
/// with `TARGET_STDEV`. Floors stdev to avoid division by zero on degenerate
/// (effectively constant) island fields.
fn znorm(value: f32, mean: f32, stdev: f32) -> f32 {
    let z = (value - mean) / stdev.max(1e-3);
    (z * TARGET_STDEV + 0.5).clamp(0.0, 1.0)
}

struct FieldStats {
    moisture_mean: f32,
    moisture_std: f32,
    elevation_mean: f32,
    elevation_std: f32,
    heat_mean: f32,
    heat_std: f32,
    zone_mean: f32,
    zone_std: f32,
    ridge_mean: f32,
    ridge_std: f32,
}

fn compute_stats_from_raw(raw: &[(HexCoord, Fields, i32)]) -> FieldStats {
    let n = raw.len().max(1) as f32;
    let mut m_sum = 0.0;
    let mut m_sq = 0.0;
    let mut e_sum = 0.0;
    let mut e_sq = 0.0;
    let mut h_sum = 0.0;
    let mut h_sq = 0.0;
    let mut z_sum = 0.0;
    let mut z_sq = 0.0;
    let mut r_sum = 0.0;
    let mut r_sq = 0.0;
    for (_, f, _) in raw {
        m_sum += f.moisture;
        m_sq += f.moisture * f.moisture;
        e_sum += f.elevation_low;
        e_sq += f.elevation_low * f.elevation_low;
        h_sum += f.heat;
        h_sq += f.heat * f.heat;
        z_sum += f.zone;
        z_sq += f.zone * f.zone;
        r_sum += f.elevation_ridge;
        r_sq += f.elevation_ridge * f.elevation_ridge;
    }
    let m_mean = m_sum / n;
    let e_mean = e_sum / n;
    let h_mean = h_sum / n;
    let z_mean = z_sum / n;
    let r_mean = r_sum / n;
    FieldStats {
        moisture_mean: m_mean,
        moisture_std: ((m_sq / n - m_mean * m_mean).max(0.0)).sqrt(),
        elevation_mean: e_mean,
        elevation_std: ((e_sq / n - e_mean * e_mean).max(0.0)).sqrt(),
        heat_mean: h_mean,
        heat_std: ((h_sq / n - h_mean * h_mean).max(0.0)).sqrt(),
        zone_mean: z_mean,
        zone_std: ((z_sq / n - z_mean * z_mean).max(0.0)).sqrt(),
        ridge_mean: r_mean,
        ridge_std: ((r_sq / n - r_mean * r_mean).max(0.0)).sqrt(),
    }
}

fn normalize(fields: Fields, stats: &FieldStats) -> Fields {
    Fields {
        moisture: znorm(fields.moisture, stats.moisture_mean, stats.moisture_std),
        elevation_low: znorm(
            fields.elevation_low,
            stats.elevation_mean,
            stats.elevation_std,
        ),
        elevation_ridge: znorm(fields.elevation_ridge, stats.ridge_mean, stats.ridge_std),
        heat: znorm(fields.heat, stats.heat_mean, stats.heat_std),
        zone: znorm(fields.zone, stats.zone_mean, stats.zone_std),
        // Wobble stays as raw signed value — its only job is to perturb
        // thresholds; normalizing it would defeat the purpose.
        wobble: fields.wobble,
    }
}

/// Classify every land tile of every outer (crescent) island.
///
/// `islands` is a slice of `(island_id, land_tiles, inland_distances)` tuples,
/// one entry per non-empty crescent. The returned map covers every tile
/// supplied in `land_tiles`; callers are expected to skip applying the
/// classification to beach tiles (the shoreline strip is already assigned).
///
/// Within each island the gates for mountain / forest are **per-island
/// percentile thresholds**: a target fraction of inland-eligible tiles become
/// mountain, and a target fraction of non-mountain tiles become forest. This
/// guarantees budget balance across the three crescents (test contract ≤ 4 pp)
/// while leaving the noise patterns — *which* tiles are picked — fully
/// distinct per island.
pub fn classify_all(
    world_seed: u64,
    islands: &[(u8, Vec<HexCoord>, FxHashMap<HexCoord, i32>)],
) -> FxHashMap<HexCoord, TerrainType> {
    let personalities = assign_personalities(world_seed);
    let total_land: usize = islands.iter().map(|(_, l, _)| l.len()).sum();
    let mut out: FxHashMap<HexCoord, TerrainType> =
        FxHashMap::with_capacity_and_hasher(total_land, Default::default());

    for (island_id, land, inland) in islands {
        let id = *island_id as usize;
        if id >= 3 {
            continue;
        }
        let p = personalities[id];
        let noise = IslandNoise::for_island(world_seed, *island_id);

        // Pass 1: sample raw fields and inland distance for each tile.
        // Parallel because each tile only reads the shared (immutable) noise
        // context — this is the largest single hotspot in the generator.
        let raw: Vec<(HexCoord, Fields, i32)> = land
            .par_iter()
            .map(|&coord| {
                let (lx, ly) = local_frame(coord, *island_id);
                let f = noise.sample(lx, ly, &p);
                let d = inland.get(&coord).copied().unwrap_or(0);
                (coord, f, d)
            })
            .collect();

        // Pass 2: compute per-island distribution statistics and normalize.
        // Normalization mostly serves to keep the SCORE numerics consistent
        // across islands; the actual budget balance is delivered by percentile
        // thresholding below.
        let stats = compute_stats_from_raw(&raw);
        let normalized: Vec<(HexCoord, Fields, i32)> = raw
            .into_par_iter()
            .map(|(c, f, d)| (c, normalize(f, &stats), d))
            .collect();

        // Pass 3: per-island percentile thresholds for mountain and forest.
        // The forest pool excludes tiles that will end up as mountain so its
        // percentile threshold matches the actual non-mountain land area.
        let mtn_scores: Vec<f32> = normalized
            .iter()
            .filter(|(_, _, d)| *d >= MOUNTAIN_MIN_INLAND)
            .map(|(_, f, _)| mountain_score(f))
            .collect();
        let mtn_upper_thresh = percentile_top(&mtn_scores, MOUNTAIN_UPPER_TARGET);
        let mtn_lower_thresh = percentile_top(&mtn_scores, MOUNTAIN_LOWER_TARGET);

        let forest_pool: Vec<f32> = normalized
            .iter()
            .filter(|(_, f, d)| {
                let s = mountain_score(f);
                !(*d >= MOUNTAIN_MIN_INLAND && s > mtn_lower_thresh)
            })
            .map(|(_, f, d)| forest_score(f, *d))
            .collect();
        let forest_thresh = percentile_top(&forest_pool, p.forest_target);

        // Pass 4: classify each tile with the per-island thresholds. The
        // wobble term on the score itself (not on the threshold here) is what
        // keeps the iso-contour ragged. Lower-tier mountains are always
        // StonySlope and upper-tier are always SnowPeak — no Hills/AridPeak
        // variants for now.
        for (coord, fields, d) in &normalized {
            let terrain = if *d >= MOUNTAIN_MIN_INLAND {
                let m = mountain_score(fields);
                if m > mtn_upper_thresh {
                    upper_mountain(fields, &p)
                } else if m > mtn_lower_thresh {
                    lower_mountain(fields)
                } else {
                    classify_non_mountain(fields, *d, forest_thresh, &p)
                }
            } else {
                classify_non_mountain(fields, *d, forest_thresh, &p)
            };
            out.insert(*coord, terrain);
        }

        // Pass 5: collapse each connected forest patch to its modal type so a
        // single patch never shows two forest tile types at once. The noise
        // still decides where forests go and where their type boundaries sit;
        // this only smooths the boundary inside each patch.
        unify_forest_patches(land, &mut out);

        // Pass 6: drop forest patches below `MIN_FOREST_PATCH_SIZE` back to
        // their natural base biome (Plains / Greenfield). The forest noise +
        // threshold occasionally produces 1-5 tile speckle that reads as
        // accidental rather than designed; this pass removes only those.
        drop_small_forest_patches(land, &mut out, &normalized, &p);

        // Pass 7: mountain cleanup.
        //  - Drop any connected mountain component that has no SnowPeak (no
        //    standalone StonySlope ranges leading nowhere).
        //  - Dilate the surviving cores by one hex of StonySlope, which both
        //    thickens the bases and guarantees every SnowPeak is ringed by
        //    StonySlope on all sides.
        cleanup_mountains(land, &mut out, &normalized, &p);
    }

    out
}

fn is_forest(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::Oldwood | TerrainType::Darkpine | TerrainType::Deepjungle
    )
}

/// Find each connected forest component within `land` (any of the three forest
/// tile types counts as one group) and replace all its tiles with the modal
/// type for that component. Ties break deterministically by enum order.
fn unify_forest_patches(land: &[HexCoord], terrains: &mut FxHashMap<HexCoord, TerrainType>) {
    let mut land_set: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    for &c in land {
        land_set.insert(c);
    }
    let mut visited: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());

    let mut starts: Vec<HexCoord> = land.to_vec();
    starts.sort_by_key(|c| (c.q, c.r));

    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let Some(&t0) = terrains.get(&start) else {
            continue;
        };
        if !is_forest(t0) {
            continue;
        }

        let mut component: Vec<HexCoord> = Vec::new();
        let mut counts: FxHashMap<TerrainType, usize> =
            FxHashMap::with_capacity_and_hasher(4, Default::default());
        let mut queue: VecDeque<HexCoord> = VecDeque::from([start]);
        visited.insert(start);

        while let Some(c) = queue.pop_front() {
            let Some(&t) = terrains.get(&c) else {
                continue;
            };
            component.push(c);
            *counts.entry(t).or_default() += 1;
            for n in c.neighbors() {
                if !land_set.contains(&n) || visited.contains(&n) {
                    continue;
                }
                let Some(&nt) = terrains.get(&n) else {
                    continue;
                };
                if !is_forest(nt) {
                    continue;
                }
                visited.insert(n);
                queue.push_back(n);
            }
        }

        let modal = counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| (a.0 as u8).cmp(&(b.0 as u8))))
            .map(|(t, _)| t);
        if let Some(modal) = modal {
            for c in component {
                terrains.insert(c, modal);
            }
        }
    }
}

/// Walk each connected forest patch; any patch smaller than
/// `MIN_FOREST_PATCH_SIZE` is reverted tile-by-tile to its natural base biome
/// (same `base_biome` call the original classifier would have made). This
/// removes the occasional 1-5 tile speckle without changing where real
/// forests sit or what type they are.
fn drop_small_forest_patches(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    normalized: &[(HexCoord, Fields, i32)],
    p: &Personality,
) {
    let mut land_set: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    for &c in land {
        land_set.insert(c);
    }
    let mut field_by_coord: FxHashMap<HexCoord, Fields> =
        FxHashMap::with_capacity_and_hasher(normalized.len(), Default::default());
    for (c, f, _) in normalized {
        field_by_coord.insert(*c, *f);
    }
    let mut visited: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());

    let mut starts: Vec<HexCoord> = land.to_vec();
    starts.sort_by_key(|c| (c.q, c.r));

    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let Some(&t0) = terrains.get(&start) else {
            continue;
        };
        if !is_forest(t0) {
            continue;
        }

        let mut component: Vec<HexCoord> = Vec::new();
        let mut queue: VecDeque<HexCoord> = VecDeque::from([start]);
        visited.insert(start);

        while let Some(c) = queue.pop_front() {
            component.push(c);
            for n in c.neighbors() {
                if !land_set.contains(&n) || visited.contains(&n) {
                    continue;
                }
                let Some(&nt) = terrains.get(&n) else {
                    continue;
                };
                if !is_forest(nt) {
                    continue;
                }
                visited.insert(n);
                queue.push_back(n);
            }
        }

        if component.len() < MIN_FOREST_PATCH_SIZE {
            for c in component {
                if let Some(fields) = field_by_coord.get(&c) {
                    terrains.insert(c, base_biome(fields, p));
                }
            }
        }
    }
}

fn is_mountain(t: TerrainType) -> bool {
    matches!(t, TerrainType::StonySlope | TerrainType::SnowPeak)
}

/// Post-process mountains:
///   1. Walk each connected mountain component (StonySlope + SnowPeak grouped
///      together). Any component that contains zero SnowPeak tiles is reverted
///      to base biome — no "orphan" ranges that don't lead to a peak.
///   2. For each surviving mountain tile, convert non-mountain land neighbours
///      to StonySlope. That dilates the bases by one ring so they read as
///      proper mountain mass instead of single-tile spines, and guarantees
///      every SnowPeak ends up surrounded by StonySlope on all six sides.
///      Beach and Freshwater are protected from the dilation.
fn cleanup_mountains(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    normalized: &[(HexCoord, Fields, i32)],
    p: &Personality,
) {
    let mut land_set: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    for &c in land {
        land_set.insert(c);
    }
    let mut field_by_coord: FxHashMap<HexCoord, Fields> =
        FxHashMap::with_capacity_and_hasher(normalized.len(), Default::default());
    for (c, f, _) in normalized {
        field_by_coord.insert(*c, *f);
    }

    // Step 1: find connected mountain components, drop orphans.
    let mut visited: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    let mut kept: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len() / 8, Default::default());

    let mut starts: Vec<HexCoord> = land.to_vec();
    starts.sort_by_key(|c| (c.q, c.r));

    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let Some(&t0) = terrains.get(&start) else {
            continue;
        };
        if !is_mountain(t0) {
            continue;
        }

        let mut component: Vec<HexCoord> = Vec::new();
        let mut has_peak = false;
        let mut queue: VecDeque<HexCoord> = VecDeque::from([start]);
        visited.insert(start);

        while let Some(c) = queue.pop_front() {
            let Some(&t) = terrains.get(&c) else {
                continue;
            };
            component.push(c);
            if t == TerrainType::SnowPeak {
                has_peak = true;
            }
            for n in c.neighbors() {
                if !land_set.contains(&n) || visited.contains(&n) {
                    continue;
                }
                let Some(&nt) = terrains.get(&n) else {
                    continue;
                };
                if !is_mountain(nt) {
                    continue;
                }
                visited.insert(n);
                queue.push_back(n);
            }
        }

        if has_peak {
            for c in &component {
                kept.insert(*c);
            }
        } else {
            for c in component {
                if let Some(fields) = field_by_coord.get(&c) {
                    terrains.insert(c, base_biome(fields, p));
                }
            }
        }
    }

    // Step 2: dilate kept mountain cores by one ring of StonySlope. Beach and
    // freshwater are off-limits — those are deliberate edges of the land.
    let mut to_thicken: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(kept.len(), Default::default());
    for &c in &kept {
        for n in c.neighbors() {
            if !land_set.contains(&n) || kept.contains(&n) {
                continue;
            }
            let Some(&nt) = terrains.get(&n) else {
                continue;
            };
            if matches!(nt, TerrainType::Beach | TerrainType::Freshwater) {
                continue;
            }
            to_thicken.insert(n);
        }
    }
    for c in to_thicken {
        terrains.insert(c, TerrainType::StonySlope);
    }
}

fn classify_non_mountain(
    fields: &Fields,
    inland: i32,
    forest_thresh: f32,
    p: &Personality,
) -> TerrainType {
    if forest_score(fields, inland) > forest_thresh {
        forest_type(fields, p)
    } else {
        base_biome(fields, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personality_permutation_is_complete() {
        for seed in [1u64, 42, 999, 12345, 99999] {
            let ps = assign_personalities(seed);
            let mut biases: Vec<i32> = ps
                .iter()
                .map(|p| (p.heat_bias * 1000.0).round() as i32)
                .collect();
            biases.sort();
            assert_eq!(biases, vec![-60, 0, 60], "seed {seed}: each preset must appear exactly once");
        }
    }

    #[test]
    fn island_noise_is_deterministic() {
        let a = IslandNoise::for_island(42, 1);
        let b = IslandNoise::for_island(42, 1);
        let p = PRESETS[0];
        let fa = a.sample(10.0, 5.0, &p);
        let fb = b.sample(10.0, 5.0, &p);
        assert_eq!(fa.elevation_low, fb.elevation_low);
        assert_eq!(fa.moisture, fb.moisture);
        assert_eq!(fa.heat, fb.heat);
    }

    #[test]
    fn different_islands_produce_different_fields() {
        let a = IslandNoise::for_island(42, 0);
        let b = IslandNoise::for_island(42, 1);
        let p = PRESETS[0];
        let fa = a.sample(10.0, 5.0, &p);
        let fb = b.sample(10.0, 5.0, &p);
        let differ = (fa.elevation_low - fb.elevation_low).abs()
            + (fa.moisture - fb.moisture).abs()
            + (fa.heat - fb.heat).abs();
        assert!(differ > 0.01, "per-island seeds must produce different fields");
    }
}
