//! Terrain for the central Old Empire landmass only.
//!
//! Base: large organic Cinderfield / Rootfield zones.
//! Forest: magic woods (Duskwood, Frostpine, Ashgrove) via the same noise +
//! percentile + patch-unify pipeline as the crescent islands.

use std::collections::VecDeque;

use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};
use glam::Vec2;
use hexx::{Hex, HexLayout, HexOrientation};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::hexgrid::HexCoord;
use crate::map::TerrainType;
use crate::rng::{hash_seed, seed_to_i32};

/// Must match [`crate::outer_islands::CENTER_ISLAND_ID`] for lake/terrain routing.
pub const CENTER_ISLAND_ID: u8 = 3;

const ZONE_FREQ: f32 = 0.0060;
const DOMAIN_WARP_FREQ: f32 = 0.0080;
const DOMAIN_WARP_AMP: f32 = 38.0;

/// Macro moisture field — wide enough for a few blobs, tight enough for 3–5 patches.
const FOREST_MOISTURE_FREQ: f32 = 0.0065;
const FOREST_ELEVATION_FREQ: f32 = 0.0050;
const FOREST_HEAT_FREQ: f32 = 0.0055;
const FOREST_WOBBLE_FREQ: f32 = 0.0180;

/// Split between Cinderfield (below) and Rootfield (above).
const ROOTFIELD_BAND: f32 = 0.50;

/// ~20–25 % of center land becomes forest; low moisture frequency yields ~3–5 patches.
const FOREST_TARGET: f32 = 0.23;

const TARGET_STDEV: f32 = 0.15;

const MIN_FOREST_PATCH_SIZE: usize = 10;
const ISOLATED_FOREST_MAX_SIZE: usize = 18;
const ISOLATED_FOREST_MIN_GAP: usize = 5;

#[derive(Clone, Copy)]
struct Fields {
    zone: f32,
    moisture: f32,
    elevation_low: f32,
    heat: f32,
    wobble: f32,
}

struct FieldStats {
    zone_mean: f32,
    zone_std: f32,
    moisture_mean: f32,
    moisture_std: f32,
    elevation_mean: f32,
    elevation_std: f32,
    heat_mean: f32,
    heat_std: f32,
}

struct CenterNoise {
    zone: FastNoiseLite,
    warp: FastNoiseLite,
    moisture: FastNoiseLite,
    elevation_low: FastNoiseLite,
    heat: FastNoiseLite,
    forest_wobble: FastNoiseLite,
}

impl CenterNoise {
    fn new(world_seed: u64) -> Self {
        let seed = hash_seed(world_seed, CENTER_ISLAND_ID as u32, 0xC3_17, 0xB4);
        Self {
            zone: make_fbm(seed, 10, 3, ZONE_FREQ),
            warp: make_domain_warp(seed, 12, DOMAIN_WARP_AMP, DOMAIN_WARP_FREQ),
            moisture: make_fbm(seed, 20, 3, FOREST_MOISTURE_FREQ),
            elevation_low: make_fbm(seed, 21, 4, FOREST_ELEVATION_FREQ),
            heat: make_fbm(seed, 22, 3, FOREST_HEAT_FREQ),
            forest_wobble: make_fbm(seed, 23, 2, FOREST_WOBBLE_FREQ),
        }
    }

    fn sample(&self, lx: f32, ly: f32) -> Fields {
        let (wx, wy) = self.warp.domain_warp_2d(lx, ly);
        Fields {
            zone: unit01(&self.zone, wx, wy),
            moisture: unit01(&self.moisture, wx + 41.0, wy - 29.0),
            elevation_low: unit01(&self.elevation_low, wx, wy),
            heat: unit01(&self.heat, wx + 31.4, wy - 17.6),
            wobble: unit01(&self.forest_wobble, wx + 113.7, wy + 73.1) - 0.5,
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

fn compute_stats(raw: &[Fields]) -> FieldStats {
    let n = raw.len().max(1) as f32;
    let mut z_sum = 0.0;
    let mut z_sq = 0.0;
    let mut m_sum = 0.0;
    let mut m_sq = 0.0;
    let mut e_sum = 0.0;
    let mut e_sq = 0.0;
    let mut h_sum = 0.0;
    let mut h_sq = 0.0;
    for f in raw {
        z_sum += f.zone;
        z_sq += f.zone * f.zone;
        m_sum += f.moisture;
        m_sq += f.moisture * f.moisture;
        e_sum += f.elevation_low;
        e_sq += f.elevation_low * f.elevation_low;
        h_sum += f.heat;
        h_sq += f.heat * f.heat;
    }
    let z_mean = z_sum / n;
    let m_mean = m_sum / n;
    let e_mean = e_sum / n;
    let h_mean = h_sum / n;
    FieldStats {
        zone_mean: z_mean,
        zone_std: ((z_sq / n - z_mean * z_mean).max(0.0)).sqrt(),
        moisture_mean: m_mean,
        moisture_std: ((m_sq / n - m_mean * m_mean).max(0.0)).sqrt(),
        elevation_mean: e_mean,
        elevation_std: ((e_sq / n - e_mean * e_mean).max(0.0)).sqrt(),
        heat_mean: h_mean,
        heat_std: ((h_sq / n - h_mean * h_mean).max(0.0)).sqrt(),
    }
}

fn normalize(fields: Fields, stats: &FieldStats) -> Fields {
    Fields {
        zone: znorm(fields.zone, stats.zone_mean, stats.zone_std),
        moisture: znorm(fields.moisture, stats.moisture_mean, stats.moisture_std),
        elevation_low: znorm(fields.elevation_low, stats.elevation_mean, stats.elevation_std),
        heat: znorm(fields.heat, stats.heat_mean, stats.heat_std),
        wobble: fields.wobble,
    }
}

fn base_terrain(zone_score: f32) -> TerrainType {
    if zone_score < ROOTFIELD_BAND {
        TerrainType::Cinderfield
    } else {
        TerrainType::Rootfield
    }
}

fn base_from_fields(fields: &Fields) -> TerrainType {
    let score = fields.zone + fields.wobble * 0.04;
    base_terrain(score)
}

/// Lighter than crescent shores so magic woods still reach the interior.
fn coast_penalty(inland: i32) -> f32 {
    match inland {
        0 => 0.18,
        1 => 0.08,
        _ => 0.0,
    }
}

fn forest_score(fields: &Fields, inland: i32) -> f32 {
    fields.moisture
        - fields.elevation_low * 0.30
        - coast_penalty(inland)
        + fields.wobble * 0.05
}

fn forest_type(fields: &Fields) -> TerrainType {
    if fields.heat < 0.38 {
        TerrainType::Frostpine
    } else if fields.heat < 0.66 {
        TerrainType::Duskwood
    } else {
        TerrainType::Ashgrove
    }
}

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

fn is_forest(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::Duskwood | TerrainType::Frostpine | TerrainType::Ashgrove
    )
}

fn inland_distances(land: &[HexCoord]) -> FxHashMap<HexCoord, i32> {
    let mut set: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    for &c in land {
        set.insert(c);
    }
    let mut dist: FxHashMap<HexCoord, i32> =
        FxHashMap::with_capacity_and_hasher(land.len(), Default::default());
    let mut queue: VecDeque<HexCoord> = VecDeque::with_capacity(land.len() / 4);

    for &c in land {
        if c.neighbors().iter().any(|n| !set.contains(n)) {
            dist.insert(c, 0);
            queue.push_back(c);
        }
    }

    while let Some(c) = queue.pop_front() {
        let d = dist[&c];
        for n in c.neighbors() {
            if set.contains(&n) && !dist.contains_key(&n) {
                dist.insert(n, d + 1);
                queue.push_back(n);
            }
        }
    }
    dist
}

fn collect_forest_patches(
    land: &[HexCoord],
    terrains: &FxHashMap<HexCoord, TerrainType>,
    field_by_coord: &FxHashMap<HexCoord, Fields>,
) -> Vec<(Vec<HexCoord>, f32)> {
    let mut land_set: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    for &c in land {
        land_set.insert(c);
    }
    let mut visited: FxHashSet<HexCoord> =
        FxHashSet::with_capacity_and_hasher(land.len(), Default::default());
    let mut patches: Vec<(Vec<HexCoord>, f32)> = Vec::new();

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
        let mut heat_sum = 0.0;
        let mut queue: VecDeque<HexCoord> = VecDeque::from([start]);
        visited.insert(start);

        while let Some(c) = queue.pop_front() {
            component.push(c);
            if let Some(f) = field_by_coord.get(&c) {
                heat_sum += f.heat;
            }
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
        let mean_heat = heat_sum / component.len().max(1) as f32;
        patches.push((component, mean_heat));
    }
    patches
}

fn count_forest_patches(
    land: &[HexCoord],
    terrains: &FxHashMap<HexCoord, TerrainType>,
    field_by_coord: &FxHashMap<HexCoord, Fields>,
) -> usize {
    collect_forest_patches(land, terrains, field_by_coord).len()
}

/// One magic forest type per connected patch. With 3+ patches, coldest →
/// Frostpine, warmest → Ashgrove, and the next → Duskwood so all three appear.
fn assign_forest_patch_types(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    field_by_coord: &FxHashMap<HexCoord, Fields>,
) {
    let mut patches = collect_forest_patches(land, terrains, field_by_coord);
    if patches.is_empty() {
        return;
    }
    patches.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    for (i, (component, mean)) in patches.iter().enumerate() {
        let patch_type = if patches.len() >= 3 {
            if i == 0 {
                TerrainType::Frostpine
            } else if i == 1 {
                TerrainType::Duskwood
            } else if i == patches.len() - 1 {
                TerrainType::Ashgrove
            } else {
                forest_type(&Fields {
                    zone: 0.5,
                    moisture: 0.5,
                    elevation_low: 0.5,
                    heat: *mean,
                    wobble: 0.0,
                })
            }
        } else if patches.len() == 2 {
            if i == 0 {
                TerrainType::Frostpine
            } else {
                TerrainType::Ashgrove
            }
        } else {
            forest_type(&Fields {
                zone: 0.5,
                moisture: 0.5,
                elevation_low: 0.5,
                heat: *mean,
                wobble: 0.0,
            })
        };
        for &c in component {
            terrains.insert(c, patch_type);
        }
    }
}

/// Raise the forest threshold until at least three disconnected patches form.
fn ensure_min_forest_patches(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    normalized: &[(HexCoord, Fields, i32)],
    field_by_coord: &FxHashMap<HexCoord, Fields>,
    mut thresh: f32,
) {
    const MIN_PATCHES: usize = 3;
    const MAX_STEPS: usize = 20;

    for _ in 0..MAX_STEPS {
        if count_forest_patches(land, terrains, field_by_coord) >= MIN_PATCHES {
            return;
        }
        thresh += 0.012;
        for (coord, fields, inland) in normalized {
            let t = if forest_score(fields, *inland) > thresh {
                forest_type(fields)
            } else {
                base_from_fields(fields)
            };
            terrains.insert(*coord, t);
        }
        assign_forest_patch_types(land, terrains, field_by_coord);
    }
}

fn drop_small_forest_patches(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    field_by_coord: &FxHashMap<HexCoord, Fields>,
) {
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
                    terrains.insert(c, base_from_fields(fields));
                }
            }
        }
    }
}

fn drop_isolated_forest_copses(
    land: &[HexCoord],
    terrains: &mut FxHashMap<HexCoord, TerrainType>,
    field_by_coord: &FxHashMap<HexCoord, Fields>,
) {
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

        if component.len() > ISOLATED_FOREST_MAX_SIZE {
            continue;
        }

        let component_set: FxHashSet<HexCoord> = component.iter().copied().collect();
        let other_forest: Vec<HexCoord> = land
            .iter()
            .filter(|&&c| {
                terrains.get(&c).is_some_and(|&t| is_forest(t)) && !component_set.contains(&c)
            })
            .copied()
            .collect();
        if other_forest.is_empty() {
            continue;
        }

        let mut dist: FxHashMap<HexCoord, usize> =
            FxHashMap::with_capacity_and_hasher(land.len() / 4, Default::default());
        let mut bfs: VecDeque<HexCoord> = VecDeque::new();
        for s in other_forest {
            dist.insert(s, 0);
            bfs.push_back(s);
        }
        let mut min_to_other = usize::MAX;
        while let Some(c) = bfs.pop_front() {
            let d = dist[&c];
            if d >= ISOLATED_FOREST_MIN_GAP {
                continue;
            }
            if component_set.contains(&c) {
                min_to_other = min_to_other.min(d);
                if min_to_other == 0 {
                    break;
                }
                continue;
            }
            for n in c.neighbors() {
                if !land_set.contains(&n) || dist.contains_key(&n) {
                    continue;
                }
                dist.insert(n, d + 1);
                bfs.push_back(n);
            }
        }

        if min_to_other > ISOLATED_FOREST_MIN_GAP {
            for c in component {
                if let Some(fields) = field_by_coord.get(&c) {
                    terrains.insert(c, base_from_fields(fields));
                }
            }
        }
    }
}

/// Classify every supplied center land tile (beach and water are excluded by the caller).
pub fn classify(world_seed: u64, land: &[HexCoord]) -> FxHashMap<HexCoord, TerrainType> {
    if land.is_empty() {
        return FxHashMap::default();
    }

    let noise = CenterNoise::new(world_seed);
    let inland = inland_distances(land);

    let raw: Vec<(HexCoord, Fields)> = land
        .par_iter()
        .map(|&coord| {
            let p = layout_xy(coord.q, coord.r);
            (coord, noise.sample(p.x, p.y))
        })
        .collect();

    let field_vec: Vec<Fields> = raw.iter().map(|(_, f)| *f).collect();
    let stats = compute_stats(&field_vec);
    let normalized: Vec<(HexCoord, Fields, i32)> = raw
        .into_iter()
        .map(|(coord, f)| {
            let d = inland.get(&coord).copied().unwrap_or(0);
            (coord, normalize(f, &stats), d)
        })
        .collect();

    let forest_pool: Vec<f32> = normalized
        .iter()
        .map(|(_, f, d)| forest_score(f, *d))
        .collect();
    let forest_thresh = percentile_top(&forest_pool, FOREST_TARGET);

    let mut field_by_coord: FxHashMap<HexCoord, Fields> =
        FxHashMap::with_capacity_and_hasher(normalized.len(), Default::default());
    let mut out: FxHashMap<HexCoord, TerrainType> =
        FxHashMap::with_capacity_and_hasher(land.len(), Default::default());

    for (coord, fields, d) in &normalized {
        field_by_coord.insert(*coord, *fields);
        let terrain = if forest_score(fields, *d) > forest_thresh {
            forest_type(fields)
        } else {
            base_from_fields(fields)
        };
        out.insert(*coord, terrain);
    }

    assign_forest_patch_types(land, &mut out, &field_by_coord);
    drop_small_forest_patches(land, &mut out, &field_by_coord);
    drop_isolated_forest_copses(land, &mut out, &field_by_coord);
    ensure_min_forest_patches(land, &mut out, &normalized, &field_by_coord, forest_thresh);
    drop_small_forest_patches(land, &mut out, &field_by_coord);
    drop_isolated_forest_copses(land, &mut out, &field_by_coord);
    assign_forest_patch_types(land, &mut out, &field_by_coord);

    out
}

