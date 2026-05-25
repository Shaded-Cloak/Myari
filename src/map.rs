use std::collections::{HashMap, VecDeque};

use glam::Vec2;

use crate::hexgrid::{hex_disk, HexCoord};
use crate::rng::tile_roll;
use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};
use hexx::{HexLayout, HexOrientation};
use serde::{Deserialize, Serialize};

pub const MAP_RADIUS: i32 = 220;

// Island layout constants (layout-space units ≈ hex widths)
const CENTER_R: f32 = 66.0;
const CRESCENT_R_IN: f32 = 96.0;
const CRESCENT_R_OUT: f32 = 192.0;
const CRESCENT_ARC_HALF: f32 = 44.0; // 88° arc → 32° open ocean between each pair
const COAST_NOISE_AMP: f32 = 14.0;
/// Per-tile wobble (in degrees) applied to the arc half-width so the crescent
/// tips terminate on a ragged curve instead of straight angular rays.
const ARC_WOBBLE_AMP_DEG: f32 = 7.0;

/// Water depth tiers measured in BFS hex distance from any land tile.
const COAST_MAX_DIST: i32 = 1;
const DEEP_OCEAN_MIN_DIST: i32 = 6;

const CRESCENT_ANGLES: [f32; 3] = [90.0, 210.0, 330.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainType {
    DeepOcean,
    Ocean,
    Coast,
    Beach,
    Hills,
    Mountain,
    SnowPeak,
    StonySlope,
    AridPeak,
    GlacialPeak,
    Ashplain,
    Thornveld,
    Deepjungle,
    Steppe,
    Plains,
    Greenfield,
    Oldwood,
    Snowfield,
    Frostmoor,
    Darkpine,
    AncientRuin,
    Corrupted,
    LeyGrove,
    LeyWaste,
    BlightedWaste,
    RuinField,
    SacredGround,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Island {
    Center,
    Crescent(u8),
}

#[derive(Debug, Clone, Copy)]
struct TileGen {
    coord: HexCoord,
    island: Option<Island>,
    terrain: TerrainType,
}

impl TileGen {
    fn is_land(&self) -> bool {
        self.island.is_some()
    }

    fn is_outer_island(&self) -> bool {
        matches!(self.island, Some(Island::Crescent(_)))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HexTile {
    pub coord: HexCoord,
    pub terrain: TerrainType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    pub tiles: Vec<HexTile>,
    #[serde(skip)]
    by_coord: HashMap<HexCoord, usize>,
}

impl Map {
    pub fn generate(radius: i32, seed: u64) -> Self {
        let mut map = Self::from_tiles(generate_tiles(radius, seed));
        map.rebuild_index();
        map
    }

    fn rebuild_index(&mut self) {
        self.by_coord.clear();
        for (i, t) in self.tiles.iter().enumerate() {
            self.by_coord.insert(t.coord, i);
        }
    }

    pub fn from_tiles(tiles: Vec<HexTile>) -> Self {
        let mut by_coord = HashMap::with_capacity(tiles.len());
        for (i, t) in tiles.iter().enumerate() {
            by_coord.insert(t.coord, i);
        }
        Self { tiles, by_coord }
    }

    pub fn tile_at(&self, coord: HexCoord) -> Option<&HexTile> {
        self.by_coord
            .get(&coord)
            .and_then(|&idx| self.tiles.get(idx))
    }
}

fn generate_tiles(radius: i32, seed: u64) -> Vec<HexTile> {
    let noise = NoiseCtx::new(seed);
    let coords = hex_disk(radius);
    let mut gen: Vec<TileGen> = coords
        .iter()
        .map(|&coord| {
            let island = classify_island(&noise, coord.q, coord.r);
            let terrain = if island.is_some() {
                TerrainType::Plains
            } else {
                TerrainType::Ocean
            };
            TileGen {
                coord,
                island,
                terrain,
            }
        })
        .collect();

    let by_coord: HashMap<HexCoord, usize> = gen
        .iter()
        .enumerate()
        .map(|(i, t)| (t.coord, i))
        .collect();

    assign_beaches(&mut gen, seed, &by_coord);
    assign_outer_island_terrain(&mut gen, seed, &by_coord);
    assign_water_depth(&mut gen, &by_coord);
    speckle_cleanup(&mut gen, &by_coord);

    let tiles: Vec<HexTile> = gen
        .into_iter()
        .map(|t| HexTile {
            coord: t.coord,
            terrain: t.terrain,
        })
        .collect();

    log_distribution(&tiles);
    tiles
}

// ── Noise (only the coast wobble is needed now) ──────────────────────────────

struct NoiseCtx {
    coast: FastNoiseLite,
    coast_warp: FastNoiseLite,
    arc_edge: FastNoiseLite,
}

impl NoiseCtx {
    fn new(seed: u64) -> Self {
        Self {
            coast: make_fbm(seed, 1, 4, 0.012),
            coast_warp: make_domain_warp(seed, 2, 22.0, 0.016),
            arc_edge: make_fbm(seed, 3, 3, 0.018),
        }
    }

    fn coast_offset(&self, x: f32, y: f32) -> f32 {
        let (wx, wy) = self.coast_warp.domain_warp_2d(x, y);
        let sample = (self.coast.get_noise_2d(wx, wy) + 1.0) * 0.5;
        (sample - 0.5) * 2.0 * COAST_NOISE_AMP
    }

    /// Signed angular offset in degrees applied to the crescent arc threshold.
    /// Same sample is shared by all three crescents but the (x, y) position is
    /// per-tile, so each tip wobbles independently in practice.
    fn arc_wobble_deg(&self, x: f32, y: f32) -> f32 {
        let sample = (self.arc_edge.get_noise_2d(x, y) + 1.0) * 0.5;
        (sample - 0.5) * 2.0 * ARC_WOBBLE_AMP_DEG
    }
}

fn make_fbm(seed: u64, salt: u32, octaves: i32, frequency: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(crate::rng::seed_to_i32(seed, salt));
    f.set_noise_type(Some(NoiseType::OpenSimplex2));
    f.set_fractal_type(Some(FractalType::FBm));
    f.set_fractal_octaves(Some(octaves));
    f.set_frequency(Some(frequency));
    f.set_fractal_lacunarity(Some(2.0));
    f.set_fractal_gain(Some(0.5));
    f
}

fn make_domain_warp(seed: u64, salt: u32, amp: f32, frequency: f32) -> FastNoiseLite {
    let mut f = FastNoiseLite::with_seed(crate::rng::seed_to_i32(seed, salt));
    f.set_domain_warp_type(Some(DomainWarpType::OpenSimplex2));
    f.set_domain_warp_amp(Some(amp));
    f.set_frequency(Some(frequency));
    f
}

// ── Layout / island mask ─────────────────────────────────────────────────────

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
    let p = flat_layout(1.0).hex_to_world_pos(hexx::Hex::new(q, r));
    Vec2::new(p.x, p.y)
}

fn polar(v: Vec2) -> (f32, f32) {
    (v.length(), v.y.atan2(v.x).to_degrees())
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

/// Central disk + three crescents. Returns which island a tile belongs to (if
/// any). The mask shape is unchanged from before — center disk plus three
/// arcs wobbled radially by the coast noise and angularly by `arc_wobble`.
fn classify_island(noise: &NoiseCtx, q: i32, r: i32) -> Option<Island> {
    let pos = layout_xy(q, r);
    let hex_dist = HexCoord::new(q, r).distance(&HexCoord::origin()) as f32;
    let coast = noise.coast_offset(pos.x, pos.y);
    let arc_wobble = noise.arc_wobble_deg(pos.x, pos.y);
    let (_, tile_angle) = polar(pos);

    if hex_dist < CENTER_R + coast {
        return Some(Island::Center);
    }

    let arc_half = CRESCENT_ARC_HALF + arc_wobble;
    for (i, &base_angle) in CRESCENT_ANGLES.iter().enumerate() {
        if angle_diff(tile_angle, base_angle).abs() > arc_half {
            continue;
        }
        let inner = CRESCENT_R_IN + coast * 0.6;
        let outer = CRESCENT_R_OUT + coast;
        if hex_dist >= inner && hex_dist <= outer {
            return Some(Island::Crescent(i as u8));
        }
    }

    None
}

// ── Beach fringe ─────────────────────────────────────────────────────────────

fn is_water(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::DeepOcean | TerrainType::Ocean | TerrainType::Coast
    )
}

fn assign_beaches(gen: &mut [TileGen], seed: u64, by_coord: &HashMap<HexCoord, usize>) {
    let water_coords: HashMap<HexCoord, ()> = gen
        .iter()
        .filter(|t| is_water(t.terrain))
        .map(|t| (t.coord, ()))
        .collect();

    // Pass 1: every land tile touching water becomes beach (the shoreline ring).
    let fringe: Vec<HexCoord> = gen
        .iter()
        .filter(|t| t.is_land())
        .filter(|t| {
            t.coord
                .neighbors()
                .iter()
                .any(|n| water_coords.contains_key(n))
        })
        .map(|t| t.coord)
        .collect();

    let fringe_set: HashMap<HexCoord, ()> = fringe.iter().map(|&c| (c, ())).collect();

    for coord in &fringe {
        if let Some(&idx) = by_coord.get(coord) {
            gen[idx].terrain = TerrainType::Beach;
        }
    }

    // Pass 2: each fringe hex independently decides whether to extend one step
    // inland. Per-tile rolls break the parallel-ring pattern.
    const EXTEND_CHANCE: f32 = 0.46;
    let mut extensions: Vec<HexCoord> = Vec::new();

    for &coord in &fringe {
        if tile_roll(seed, coord.q, coord.r, 71) > EXTEND_CHANCE {
            continue;
        }
        for n in coord.neighbors() {
            let Some(&idx) = by_coord.get(&n) else {
                continue;
            };
            let inland = &gen[idx];
            if !inland.is_land() {
                continue;
            }
            if fringe_set.contains_key(&n) {
                continue;
            }
            if inland.terrain == TerrainType::Beach {
                continue;
            }
            extensions.push(n);
        }
    }

    for coord in extensions {
        if let Some(&idx) = by_coord.get(&coord) {
            gen[idx].terrain = TerrainType::Beach;
        }
    }
}

// ── Inland distance (per-island BFS from coast) ──────────────────────────────

/// BFS distance from the coastal edge of an island (tiles touching water or the
/// outside of the land set are distance 0). Used as a feature gate (mountains
/// need depth) and a soft suppressor for forests close to the shore.
fn inland_distances(land: &[HexCoord]) -> HashMap<HexCoord, i32> {
    let set: HashMap<HexCoord, ()> = land.iter().map(|&c| (c, ())).collect();
    let mut dist: HashMap<HexCoord, i32> = HashMap::with_capacity(land.len());
    let mut queue: VecDeque<HexCoord> = VecDeque::new();

    for &c in land {
        if c.neighbors().iter().any(|n| !set.contains_key(n)) {
            dist.insert(c, 0);
            queue.push_back(c);
        }
    }

    while let Some(c) = queue.pop_front() {
        let d = dist[&c];
        for n in c.neighbors() {
            if set.contains_key(&n) && !dist.contains_key(&n) {
                dist.insert(n, d + 1);
                queue.push_back(n);
            }
        }
    }
    dist
}

// ── Outer-island terrain ─────────────────────────────────────────────────────

fn collect_outer_islands(gen: &[TileGen]) -> Vec<(u8, Vec<HexCoord>, HashMap<HexCoord, i32>)> {
    let mut out: Vec<(u8, Vec<HexCoord>, HashMap<HexCoord, i32>)> = Vec::with_capacity(3);
    for island_id in 0..3u8 {
        let land: Vec<HexCoord> = gen
            .iter()
            .filter(|t| t.island == Some(Island::Crescent(island_id)))
            .map(|t| t.coord)
            .collect();
        if land.is_empty() {
            continue;
        }
        let inland = inland_distances(&land);
        out.push((island_id, land, inland));
    }
    out
}

fn assign_outer_island_terrain(
    gen: &mut [TileGen],
    seed: u64,
    by_coord: &HashMap<HexCoord, usize>,
) {
    let islands = collect_outer_islands(gen);
    let assignments = crate::outer_islands::classify_all(seed, &islands);
    for (coord, terrain) in assignments {
        if let Some(&idx) = by_coord.get(&coord) {
            if gen[idx].terrain == TerrainType::Beach {
                continue;
            }
            gen[idx].terrain = terrain;
        }
    }
}

// ── Water depth (Coast / Ocean / DeepOcean) ──────────────────────────────────

fn assign_water_depth(gen: &mut [TileGen], by_coord: &HashMap<HexCoord, usize>) {
    let land_coords: HashMap<HexCoord, ()> = gen
        .iter()
        .filter(|t| t.is_land())
        .map(|t| (t.coord, ()))
        .collect();

    let in_map: HashMap<HexCoord, ()> = by_coord.keys().map(|&c| (c, ())).collect();

    let mut dist_to_land: HashMap<HexCoord, i32> = HashMap::new();
    let mut queue: VecDeque<HexCoord> = VecDeque::new();

    for tile in gen.iter().filter(|t| t.is_land()) {
        dist_to_land.insert(tile.coord, 0);
        queue.push_back(tile.coord);
    }

    while let Some(c) = queue.pop_front() {
        let d = dist_to_land[&c];
        for n in c.neighbors() {
            if dist_to_land.contains_key(&n) || land_coords.contains_key(&n) {
                continue;
            }
            if !in_map.contains_key(&n) {
                continue;
            }
            dist_to_land.insert(n, d + 1);
            queue.push_back(n);
        }
    }

    for tile in gen.iter_mut() {
        if tile.is_land() {
            continue;
        }
        let d = dist_to_land
            .get(&tile.coord)
            .copied()
            .unwrap_or(DEEP_OCEAN_MIN_DIST);
        tile.terrain = if d <= COAST_MAX_DIST {
            TerrainType::Coast
        } else if d >= DEEP_OCEAN_MIN_DIST {
            TerrainType::DeepOcean
        } else {
            TerrainType::Ocean
        };
    }
}

// ── Speckle cleanup ──────────────────────────────────────────────────────────

/// Replace any outer-island land tile whose six neighbours contain ZERO of the
/// same terrain type with the modal non-water, non-beach neighbour. Only acts
/// when the tile has at least two real-land neighbours, so coastal peninsulas
/// are left alone.
fn speckle_cleanup(gen: &mut [TileGen], by_coord: &HashMap<HexCoord, usize>) {
    let mut changes: Vec<(HexCoord, TerrainType)> = Vec::new();

    for tile in gen.iter() {
        if !tile.is_outer_island() {
            continue;
        }
        if tile.terrain == TerrainType::Beach {
            continue;
        }

        let mut same = 0;
        let mut land_neighbors = 0;
        let mut counts: HashMap<TerrainType, i32> = HashMap::new();

        for n in tile.coord.neighbors() {
            let Some(&idx) = by_coord.get(&n) else {
                continue;
            };
            let nt = gen[idx].terrain;
            if is_water(nt) || nt == TerrainType::Beach {
                continue;
            }
            land_neighbors += 1;
            if nt == tile.terrain {
                same += 1;
            } else {
                *counts.entry(nt).or_default() += 1;
            }
        }

        if land_neighbors < 2 || same > 0 {
            continue;
        }
        if let Some((replacement, _)) = counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| (a.0 as u8).cmp(&(b.0 as u8))))
        {
            changes.push((tile.coord, replacement));
        }
    }

    for (coord, terrain) in changes {
        if let Some(&idx) = by_coord.get(&coord) {
            gen[idx].terrain = terrain;
        }
    }
}

// ── Helpers / logging ────────────────────────────────────────────────────────

#[cfg(test)]
fn is_land_biome(t: TerrainType) -> bool {
    !matches!(
        t,
        TerrainType::DeepOcean | TerrainType::Ocean | TerrainType::Coast
    )
}

fn terrain_name(t: TerrainType) -> &'static str {
    match t {
        TerrainType::DeepOcean => "DeepOcean",
        TerrainType::Ocean => "Ocean",
        TerrainType::Coast => "Coast",
        TerrainType::Beach => "Beach",
        TerrainType::Hills => "Hills",
        TerrainType::Mountain => "Mountain",
        TerrainType::SnowPeak => "SnowPeak",
        TerrainType::StonySlope => "StonySlope",
        TerrainType::AridPeak => "AridPeak",
        TerrainType::GlacialPeak => "GlacialPeak",
        TerrainType::Ashplain => "Ashplain",
        TerrainType::Thornveld => "Thornveld",
        TerrainType::Deepjungle => "Deepjungle",
        TerrainType::Steppe => "Steppe",
        TerrainType::Plains => "Plains",
        TerrainType::Greenfield => "Greenfield",
        TerrainType::Oldwood => "Oldwood",
        TerrainType::Snowfield => "Snowfield",
        TerrainType::Frostmoor => "Frostmoor",
        TerrainType::Darkpine => "Darkpine",
        TerrainType::AncientRuin => "AncientRuin",
        TerrainType::Corrupted => "Corrupted",
        TerrainType::LeyGrove => "LeyGrove",
        TerrainType::LeyWaste => "LeyWaste",
        TerrainType::BlightedWaste => "BlightedWaste",
        TerrainType::RuinField => "RuinField",
        TerrainType::SacredGround => "SacredGround",
    }
}

fn log_distribution(tiles: &[HexTile]) {
    let total = tiles.len() as f64;
    let mut counts: HashMap<TerrainType, usize> = HashMap::new();
    for tile in tiles {
        *counts.entry(tile.terrain).or_default() += 1;
    }

    let reported = [
        TerrainType::DeepOcean,
        TerrainType::Ocean,
        TerrainType::Coast,
        TerrainType::Beach,
        TerrainType::Plains,
        TerrainType::Greenfield,
        TerrainType::Steppe,
        TerrainType::Oldwood,
        TerrainType::Darkpine,
        TerrainType::Deepjungle,
        TerrainType::Hills,
        TerrainType::StonySlope,
        TerrainType::SnowPeak,
        TerrainType::AridPeak,
    ];

    println!("=== Terrain Distribution ===");
    for t in reported {
        let c = counts.get(&t).copied().unwrap_or(0);
        if c == 0 {
            continue;
        }
        let pct = if total > 0.0 {
            (c as f64 / total) * 100.0
        } else {
            0.0
        };
        println!("  {}: {} ({:.1}%)", terrain_name(t), c, pct);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Steppe, Hills, and AridPeak are intentionally disabled — the generator
    // must not emit them on outer islands until they are re-enabled.
    const ALLOWED_OUTER_LAND: &[TerrainType] = &[
        TerrainType::Beach,
        TerrainType::Plains,
        TerrainType::Greenfield,
        TerrainType::Oldwood,
        TerrainType::Darkpine,
        TerrainType::Deepjungle,
        TerrainType::StonySlope,
        TerrainType::SnowPeak,
    ];

    fn is_outer_forest(t: TerrainType) -> bool {
        matches!(
            t,
            TerrainType::Oldwood | TerrainType::Darkpine | TerrainType::Deepjungle
        )
    }

    fn is_outer_mountain(t: TerrainType) -> bool {
        matches!(
            t,
            TerrainType::Hills
                | TerrainType::StonySlope
                | TerrainType::SnowPeak
                | TerrainType::AridPeak
        )
    }

    fn crescent_land_by_island(map: &Map, seed: u64) -> [Vec<HexCoord>; 3] {
        let noise = NoiseCtx::new(seed);
        let mut out: [Vec<HexCoord>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for tile in &map.tiles {
            if let Some(Island::Crescent(i)) = classify_island(&noise, tile.coord.q, tile.coord.r)
            {
                out[i as usize].push(tile.coord);
            }
        }
        out
    }

    #[test]
    fn outer_islands_use_allowed_palette_only() {
        let map = Map::generate(MAP_RADIUS, 42);
        let by_island = crescent_land_by_island(&map, 42);
        let terrains: HashMap<HexCoord, TerrainType> =
            map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();
        for (i, land) in by_island.iter().enumerate() {
            for &coord in land {
                let t = terrains[&coord];
                assert!(
                    ALLOWED_OUTER_LAND.contains(&t),
                    "crescent {i} tile {:?} has unallowed terrain {:?}",
                    coord,
                    t
                );
            }
        }
    }

    #[test]
    fn shoreline_is_beach() {
        let map = Map::generate(MAP_RADIUS, 42);
        let by_coord: HashMap<HexCoord, TerrainType> =
            map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();
        for tile in &map.tiles {
            if !is_land_biome(tile.terrain) {
                continue;
            }
            let touches_water = tile
                .coord
                .neighbors()
                .iter()
                .any(|n| by_coord.get(n).map_or(false, |t| is_water(*t)));
            if touches_water {
                assert_eq!(
                    tile.terrain,
                    TerrainType::Beach,
                    "land tile at {:?} touches water but is not beach",
                    tile.coord
                );
            }
        }
    }

    #[test]
    fn water_depth_layering() {
        let map = Map::generate(MAP_RADIUS, 42);
        let by_coord: HashMap<HexCoord, TerrainType> =
            map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();

        // Every water tile adjacent to land must be Coast.
        for tile in &map.tiles {
            if !is_water(tile.terrain) {
                continue;
            }
            let touches_land = tile
                .coord
                .neighbors()
                .iter()
                .any(|n| by_coord.get(n).map_or(false, |t| is_land_biome(*t)));
            if touches_land {
                assert_eq!(
                    tile.terrain,
                    TerrainType::Coast,
                    "water tile at {:?} adjacent to land but not Coast",
                    tile.coord
                );
            }
        }

        // Some DeepOcean must exist far from land.
        let deep_count = map
            .tiles
            .iter()
            .filter(|t| t.terrain == TerrainType::DeepOcean)
            .count();
        assert!(
            deep_count > 50,
            "expected substantial DeepOcean, got {deep_count}"
        );
    }

    #[test]
    fn outer_islands_budgets_balanced() {
        for seed in [42u64, 123, 999, 7777] {
            let map = Map::generate(MAP_RADIUS, seed);
            let by_island = crescent_land_by_island(&map, seed);
            let terrains: HashMap<HexCoord, TerrainType> =
                map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();

            let mut forest_pcts = Vec::new();
            let mut mountain_pcts = Vec::new();
            let mut land_counts = Vec::new();
            for land in &by_island {
                assert!(!land.is_empty(), "seed {seed}: crescent must have land");
                let n = land.len() as f32;
                let f = land
                    .iter()
                    .filter(|c| terrains.get(c).is_some_and(|t| is_outer_forest(*t)))
                    .count() as f32
                    / n;
                let m = land
                    .iter()
                    .filter(|c| terrains.get(c).is_some_and(|t| is_outer_mountain(*t)))
                    .count() as f32
                    / n;
                forest_pcts.push(f);
                mountain_pcts.push(m);
                land_counts.push(land.len());
            }

            let f_spread = forest_pcts.iter().cloned().fold(0.0f32, f32::max)
                - forest_pcts.iter().cloned().fold(f32::INFINITY, f32::min);
            let m_spread = mountain_pcts.iter().cloned().fold(0.0f32, f32::max)
                - mountain_pcts.iter().cloned().fold(f32::INFINITY, f32::min);
            assert!(
                f_spread <= 0.04,
                "seed {seed}: forest % spread too wide: {forest_pcts:?}"
            );
            assert!(
                m_spread <= 0.04,
                "seed {seed}: mountain % spread too wide: {mountain_pcts:?}"
            );

            let max_land = *land_counts.iter().max().unwrap();
            let min_land = *land_counts.iter().min().unwrap();
            assert!(
                (max_land as f32) / (min_land as f32) <= 1.10,
                "seed {seed}: land tile counts unbalanced: {land_counts:?}"
            );
        }
    }

    #[test]
    fn outer_islands_each_have_full_palette() {
        // Steppe / AridPeak intentionally excluded — not generated for now.
        let required_bases = [TerrainType::Plains, TerrainType::Greenfield];
        let required_forests = [
            TerrainType::Oldwood,
            TerrainType::Darkpine,
            TerrainType::Deepjungle,
        ];

        for seed in [42u64, 123, 999, 7777] {
            let map = Map::generate(MAP_RADIUS, seed);
            let by_island = crescent_land_by_island(&map, seed);
            let terrains: HashMap<HexCoord, TerrainType> =
                map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();

            for (i, land) in by_island.iter().enumerate() {
                let present: std::collections::HashSet<TerrainType> = land
                    .iter()
                    .filter_map(|c| terrains.get(c).copied())
                    .collect();

                for t in required_bases {
                    assert!(
                        present.contains(&t),
                        "seed {seed} crescent {i} missing base biome {:?}",
                        t
                    );
                }
                for t in required_forests {
                    assert!(
                        present.contains(&t),
                        "seed {seed} crescent {i} missing forest type {:?}",
                        t
                    );
                }
                let has_mountain = present.iter().any(|t| is_outer_mountain(*t));
                assert!(
                    has_mountain,
                    "seed {seed} crescent {i} has no mountain tile"
                );
            }
        }
    }

    #[test]
    fn outer_islands_no_isolated_tiles() {
        let map = Map::generate(MAP_RADIUS, 42);
        let by_island = crescent_land_by_island(&map, 42);
        let terrains: HashMap<HexCoord, TerrainType> =
            map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();

        for (i, land) in by_island.iter().enumerate() {
            for &coord in land {
                let t = terrains[&coord];
                if t == TerrainType::Beach {
                    continue;
                }
                let mut same = 0;
                let mut land_neighbors = 0;
                for n in coord.neighbors() {
                    let Some(&nt) = terrains.get(&n) else {
                        continue;
                    };
                    if is_water(nt) || nt == TerrainType::Beach {
                        continue;
                    }
                    land_neighbors += 1;
                    if nt == t {
                        same += 1;
                    }
                }
                if land_neighbors >= 2 {
                    assert!(
                        same > 0,
                        "isolated tile on crescent {i} at {:?} (terrain {:?})",
                        coord,
                        t
                    );
                }
            }
        }
    }

    #[test]
    fn outer_islands_are_wider_than_before() {
        let map = Map::generate(MAP_RADIUS, 42);
        let by_island = crescent_land_by_island(&map, 42);
        let total: usize = by_island.iter().map(|land| land.len()).sum();
        // The pre-widening (CRESCENT_R_IN = 126, CRESCENT_R_OUT = 180) crescents
        // covered roughly 17k–20k tiles total. Anything substantially above that
        // proves the widening took effect.
        assert!(
            total > 28_000,
            "crescents not wide enough: {total} land tiles across all three"
        );
    }

    #[test]
    fn generate_is_deterministic() {
        let a = Map::generate(MAP_RADIUS, 999);
        let b = Map::generate(MAP_RADIUS, 999);
        assert_eq!(a.tiles.len(), b.tiles.len());
        for (ta, tb) in a.tiles.iter().zip(b.tiles.iter()) {
            assert_eq!(ta.coord, tb.coord);
            assert_eq!(ta.terrain, tb.terrain);
        }
    }

    #[test]
    fn multiple_seeds_generate_valid_maps() {
        for seed in [42u64, 123, 999, 55555] {
            let map = Map::generate(MAP_RADIUS, seed);
            let land = map
                .tiles
                .iter()
                .filter(|t| is_land_biome(t.terrain))
                .count();
            assert!(land > 20_000, "seed {seed}: insufficient land ({land})");
        }
    }
}
