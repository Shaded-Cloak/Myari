use std::collections::VecDeque;

use glam::Vec2;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::hexgrid::{hex_disk, HexCoord};
use crate::rng::{hash_seed, tile_roll};
use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};
use hexx::{HexLayout, HexOrientation};
use serde::{Deserialize, Serialize};

/// Hex disk radius. Island layout uses fixed radii below — raising this only
/// adds open ocean around the outside without moving land.
pub const MAP_RADIUS: i32 = 360;

// Island layout constants (layout-space units ≈ hex widths)
const CENTER_R: f32 = 70.0;
const CRESCENT_R_IN: f32 = 140.0;
const CRESCENT_R_OUT: f32 = 240.0;
const CRESCENT_ARC_HALF: f32 = 44.0; // 88° arc → 32° open ocean between each pair
const COAST_NOISE_AMP: f32 = 14.0;
/// Per-tile wobble (in degrees) applied to the arc half-width so the crescent
/// tips terminate on a ragged curve instead of straight angular rays.
const ARC_WOBBLE_AMP_DEG: f32 = 7.0;

/// Water depth tiers measured in BFS hex distance from any land tile.
const COAST_MAX_DIST: i32 = 1;
const DEEP_OCEAN_MIN_DIST: i32 = 6;

/// Minimum BFS distance from the coast at which a tile can become an inland
/// lake. Lower values would let lakes spawn right behind the beach, blending
/// into the shore.
const LAKE_MIN_INLAND: i32 = 3;
/// Fraction of inland-eligible tiles that become Freshwater per island. Tuned
/// alongside `LAKE_NOISE_FREQ` so each crescent ends up with exactly one big
/// lake of ~1000-1200 hexes instead of two smaller ones.
const LAKE_TARGET_FRAC: f32 = 0.07;
/// Lake noise frequency. A period of ~90 hexes is comfortably wider than the
/// crescent's footprint, so the noise has only one strong maximum per island
/// — that locks each crescent to a single connected lake which absorbs the
/// full per-island water budget. Higher octaves in the FBM still give the
/// shoreline organic wobble.
const LAKE_NOISE_FREQ: f32 = 0.011;

const CRESCENT_ANGLES: [f32; 3] = [90.0, 210.0, 330.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainType {
    DeepOcean,
    Ocean,
    Coast,
    /// Inland lake water — on a land-island tile but counted as water
    /// for the beach fringe pass. Does NOT participate in the Coast / Ocean /
    /// DeepOcean depth gradient.
    Freshwater,
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

    fn uses_procedural_terrain(&self) -> bool {
        self.island.is_some()
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
    by_coord: FxHashMap<HexCoord, usize>,
}

impl Map {
    pub fn generate(radius: i32, seed: u64) -> Self {
        let mut map = Self::from_tiles(generate_tiles(radius, seed));
        map.rebuild_index();
        map
    }

    fn rebuild_index(&mut self) {
        self.by_coord.clear();
        self.by_coord.reserve(self.tiles.len());
        for (i, t) in self.tiles.iter().enumerate() {
            self.by_coord.insert(t.coord, i);
        }
    }

    pub fn from_tiles(tiles: Vec<HexTile>) -> Self {
        let mut by_coord: FxHashMap<HexCoord, usize> =
            FxHashMap::with_capacity_and_hasher(tiles.len(), Default::default());
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

/// Dense `(q, r) -> tile index` table. Replaces `HashMap<HexCoord, usize>`
/// during generation for O(1) lookup with no hashing. The disk has radius
/// `MAP_RADIUS`, so q and r each live in `[-radius, radius]` — we allocate a
/// `(2R+1)^2` flat `Vec<i32>` where `-1` marks an out-of-disk slot. Memory cost
/// is ~1 MB at the current radius (one i32 per slot), trivial next to the perf win.
struct TileIndex {
    radius: i32,
    stride: i32,
    indices: Vec<i32>,
}

impl TileIndex {
    fn new(radius: i32, tiles: &[TileGen]) -> Self {
        let stride = 2 * radius + 1;
        let mut indices = vec![-1i32; (stride * stride) as usize];
        for (i, t) in tiles.iter().enumerate() {
            let key = ((t.coord.q + radius) * stride + (t.coord.r + radius)) as usize;
            indices[key] = i as i32;
        }
        Self {
            radius,
            stride,
            indices,
        }
    }

    #[inline]
    fn get(&self, c: HexCoord) -> Option<usize> {
        if c.q < -self.radius || c.q > self.radius || c.r < -self.radius || c.r > self.radius {
            return None;
        }
        let key = ((c.q + self.radius) * self.stride + (c.r + self.radius)) as usize;
        let idx = self.indices[key];
        if idx < 0 {
            None
        } else {
            Some(idx as usize)
        }
    }
}

fn generate_tiles(radius: i32, seed: u64) -> Vec<HexTile> {
    let total_start = std::time::Instant::now();

    let noise = NoiseCtx::new(seed);
    let coords = hex_disk(radius);

    // Parallel initial classify — each tile is independent of every other and
    // only reads the shared (immutable) noise context.
    let classify_start = std::time::Instant::now();
    let mut gen: Vec<TileGen> = coords
        .par_iter()
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
    let classify_ms = classify_start.elapsed().as_secs_f32() * 1000.0;

    let index_start = std::time::Instant::now();
    let by_coord = TileIndex::new(radius, &gen);
    let index_ms = index_start.elapsed().as_secs_f32() * 1000.0;

    let holes_start = std::time::Instant::now();
    fix_inland_holes(&mut gen, &by_coord);
    let holes_ms = holes_start.elapsed().as_secs_f32() * 1000.0;

    let lakes_start = std::time::Instant::now();
    assign_lakes(&mut gen, seed, &by_coord);
    let lakes_ms = lakes_start.elapsed().as_secs_f32() * 1000.0;

    let beach_start = std::time::Instant::now();
    assign_beaches(&mut gen, seed, &by_coord);
    let beach_ms = beach_start.elapsed().as_secs_f32() * 1000.0;

    let outer_start = std::time::Instant::now();
    assign_island_terrain(&mut gen, seed, &by_coord);
    let outer_ms = outer_start.elapsed().as_secs_f32() * 1000.0;

    let depth_start = std::time::Instant::now();
    assign_water_depth(&mut gen, &by_coord);
    let depth_ms = depth_start.elapsed().as_secs_f32() * 1000.0;

    let cleanup_start = std::time::Instant::now();
    speckle_cleanup(&mut gen, &by_coord);
    let cleanup_ms = cleanup_start.elapsed().as_secs_f32() * 1000.0;

    let tiles: Vec<HexTile> = gen
        .into_iter()
        .map(|t| HexTile {
            coord: t.coord,
            terrain: t.terrain,
        })
        .collect();

    let total_ms = total_start.elapsed().as_secs_f32() * 1000.0;
    println!(
        "=== Generation timing (ms) === total={total_ms:.1} | mask={classify_ms:.1} index={index_ms:.1} holes={holes_ms:.1} lakes={lakes_ms:.1} beach={beach_ms:.1} outer={outer_ms:.1} depth={depth_ms:.1} cleanup={cleanup_ms:.1}",
    );

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

// Layout-space conversions in the noise-classification path use unit-size hexes
// and are called per-tile, so we keep a single shared instance instead of
// rebuilding the layout struct each call.
thread_local! {
    static UNIT_LAYOUT: HexLayout = flat_layout(1.0);
}

fn layout_xy(q: i32, r: i32) -> Vec2 {
    UNIT_LAYOUT.with(|layout| {
        let p = layout.hex_to_world_pos(hexx::Hex::new(q, r));
        Vec2::new(p.x, p.y)
    })
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

// ── Inland-hole cleanup ──────────────────────────────────────────────────────

/// Fix water tiles that the wobbly coast mask accidentally stranded *inside*
/// an island. We flood-fill from the map's outer edge through every water
/// tile; any water tile not reachable from the edge is a hole. Each hole tile
/// is promoted to whichever crescent's land surrounds it most, then the rest
/// of the pipeline classifies it like any other land tile.
fn fix_inland_holes(gen: &mut [TileGen], by_coord: &TileIndex) {
    let n = gen.len();
    let mut reachable: Vec<bool> = vec![false; n];
    let mut queue: VecDeque<usize> = VecDeque::with_capacity(n / 8);

    // Seed BFS from every water tile that has an off-map neighbour. Those are
    // by definition tiles at the outer rim, i.e. the real open ocean.
    for (i, tile) in gen.iter().enumerate() {
        if tile.island.is_some() {
            continue;
        }
        let touches_edge = tile
            .coord
            .neighbors()
            .iter()
            .any(|n| by_coord.get(*n).is_none());
        if touches_edge {
            reachable[i] = true;
            queue.push_back(i);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let coord = gen[idx].coord;
        for n_coord in coord.neighbors() {
            let Some(nidx) = by_coord.get(n_coord) else {
                continue;
            };
            if reachable[nidx] || gen[nidx].island.is_some() {
                continue;
            }
            reachable[nidx] = true;
            queue.push_back(nidx);
        }
    }

    // Promote any unreachable water tile to land of the surrounding crescent.
    for i in 0..n {
        if gen[i].island.is_some() || reachable[i] {
            continue;
        }
        let mut crescent_count = [0u32; 3];
        let mut center_count = 0u32;
        for n in gen[i].coord.neighbors() {
            if let Some(nidx) = by_coord.get(n) {
                match gen[nidx].island {
                    Some(Island::Crescent(c)) => crescent_count[c as usize] += 1,
                    Some(Island::Center) => center_count += 1,
                    None => {}
                }
            }
        }
        let max_crescent = crescent_count.iter().copied().max().unwrap_or(0);
        let promoted = if max_crescent >= center_count {
            let idx = crescent_count
                .iter()
                .position(|&c| c == max_crescent)
                .unwrap_or(0);
            Island::Crescent(idx as u8)
        } else {
            Island::Center
        };
        gen[i].island = Some(promoted);
        gen[i].terrain = TerrainType::Plains;
    }
}

// ── Inland lakes ─────────────────────────────────────────────────────────────

/// Drop Freshwater lakes on each land island (center + crescents). Runs BEFORE
/// `assign_beaches` so the beach pass naturally wraps every lake with sand.
///
/// Algorithm: per island, sample a high-frequency FBM seeded uniquely for
/// (world_seed, island_id). Tiles at `inland >= LAKE_MIN_INLAND` are eligible.
/// We then take the top `LAKE_TARGET_FRAC` of eligible tiles by noise score —
/// percentile, so coverage is constant across islands regardless of how the
/// noise distribution shifts. A final pass drops any 1-tile lake (isolated
/// Freshwater with no Freshwater neighbour) back to Plains.
fn assign_lakes(gen: &mut [TileGen], seed: u64, by_coord: &TileIndex) {
    let lake_islands: [(Island, u8); 4] = [
        (Island::Center, crate::outer_islands::CENTER_ISLAND_ID),
        (Island::Crescent(0), 0),
        (Island::Crescent(1), 1),
        (Island::Crescent(2), 2),
    ];

    for (island, island_id) in lake_islands {
        let land: Vec<HexCoord> = gen
            .iter()
            .filter(|t| t.island == Some(island))
            .map(|t| t.coord)
            .collect();
        if land.is_empty() {
            continue;
        }

        let inland = inland_distances(&land);
        let lake_seed = hash_seed(seed, island_id as u32, 0x1A_4E, 0xF0);
        let noise = make_fbm(lake_seed, 0, 3, LAKE_NOISE_FREQ);

        let eligible: Vec<(HexCoord, f32)> = land
            .par_iter()
            .filter(|c| inland.get(c).copied().unwrap_or(0) >= LAKE_MIN_INLAND)
            .map(|&c| {
                let p = layout_xy(c.q, c.r);
                (c, noise.get_noise_2d(p.x, p.y))
            })
            .collect();
        if eligible.is_empty() {
            continue;
        }

        let mut scores: Vec<f32> = eligible.iter().map(|(_, s)| *s).collect();
        scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let cutoff_idx = ((scores.len() as f32 * LAKE_TARGET_FRAC) as usize)
            .min(scores.len().saturating_sub(1));
        let threshold = scores[cutoff_idx];

        for (coord, score) in eligible {
            if score >= threshold {
                if let Some(idx) = by_coord.get(coord) {
                    gen[idx].terrain = TerrainType::Freshwater;
                }
            }
        }
    }

    // Drop 1-tile lakes — they read as speckle, not as a lake. Revert them to
    // Plains so the outer-island classifier picks something natural for the
    // tile.
    let mut to_revert: Vec<usize> = Vec::new();
    for (i, tile) in gen.iter().enumerate() {
        if tile.terrain != TerrainType::Freshwater {
            continue;
        }
        let has_neighbour_lake = tile.coord.neighbors().iter().any(|n| {
            by_coord
                .get(*n)
                .map(|idx| gen[idx].terrain == TerrainType::Freshwater)
                .unwrap_or(false)
        });
        if !has_neighbour_lake {
            to_revert.push(i);
        }
    }
    for idx in to_revert {
        gen[idx].terrain = TerrainType::Plains;
    }
}

// ── Beach fringe ─────────────────────────────────────────────────────────────

pub fn is_water(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::DeepOcean
            | TerrainType::Ocean
            | TerrainType::Coast
            | TerrainType::Freshwater
    )
}

fn assign_beaches(gen: &mut [TileGen], seed: u64, by_coord: &TileIndex) {
    // Compact water mask is faster than a hash set for the inner `any` check.
    let mut is_water_tile: Vec<bool> = vec![false; gen.len()];
    for (i, t) in gen.iter().enumerate() {
        is_water_tile[i] = is_water(t.terrain);
    }

    // Pass 1: every land tile touching water becomes beach (the shoreline ring).
    // Freshwater tiles are geographically on an island but have water terrain,
    // so we filter them out here — only "land terrain" tiles can become Beach.
    let fringe: Vec<HexCoord> = gen
        .iter()
        .filter(|t| t.is_land() && !is_water(t.terrain))
        .filter(|t| {
            t.coord.neighbors().iter().any(|n| {
                by_coord
                    .get(*n)
                    .map(|idx| is_water_tile[idx])
                    .unwrap_or(false)
            })
        })
        .map(|t| t.coord)
        .collect();

    let mut is_fringe: Vec<bool> = vec![false; gen.len()];
    for coord in &fringe {
        if let Some(idx) = by_coord.get(*coord) {
            is_fringe[idx] = true;
            gen[idx].terrain = TerrainType::Beach;
        }
    }

    // Pass 2: each ocean-adjacent fringe hex independently decides whether to
    // extend one step inland. Per-tile rolls break the parallel-ring pattern.
    // Lake-only fringe tiles (those bordering Freshwater but no ocean tile)
    // are intentionally excluded so every lake stays ringed by exactly one
    // tile of beach.
    const EXTEND_CHANCE: f32 = 0.46;
    let mut extensions: Vec<HexCoord> = Vec::new();

    for &coord in &fringe {
        let touches_ocean = coord.neighbors().iter().any(|n| {
            by_coord
                .get(*n)
                .map(|idx| {
                    matches!(
                        gen[idx].terrain,
                        TerrainType::DeepOcean | TerrainType::Ocean | TerrainType::Coast
                    )
                })
                .unwrap_or(false)
        });
        if !touches_ocean {
            continue;
        }
        if tile_roll(seed, coord.q, coord.r, 71) > EXTEND_CHANCE {
            continue;
        }
        for n in coord.neighbors() {
            let Some(idx) = by_coord.get(n) else {
                continue;
            };
            let inland = &gen[idx];
            if !inland.is_land() {
                continue;
            }
            if is_fringe[idx] {
                continue;
            }
            if is_water(inland.terrain) {
                // Skip freshwater (it's "land" geographically but water in terrain).
                continue;
            }
            if inland.terrain == TerrainType::Beach {
                continue;
            }
            extensions.push(n);
        }
    }

    for coord in extensions {
        if let Some(idx) = by_coord.get(coord) {
            gen[idx].terrain = TerrainType::Beach;
        }
    }

    // Pass 3: fill in single-tile beach pockets. When pass 2 extends some
    // fringe tiles but not others, the un-extended slot in ring 2 can end up
    // surrounded by beach (or beach + water) on most sides. Those slots read
    // as awkward inland green patches stuck between sand and ocean — convert
    // any non-beach land tile with at least 4 beach (or water) neighbours
    // into beach as well, smoothing the fringe.
    let mut pocket_fills: Vec<usize> = Vec::new();
    for (i, tile) in gen.iter().enumerate() {
        if !tile.is_land() {
            continue;
        }
        if tile.terrain == TerrainType::Beach || is_water(tile.terrain) {
            continue;
        }
        let mut beachy_neighbors = 0;
        for n in tile.coord.neighbors() {
            let Some(idx) = by_coord.get(n) else {
                continue;
            };
            let t = gen[idx].terrain;
            if t == TerrainType::Beach || is_water(t) {
                beachy_neighbors += 1;
            }
        }
        if beachy_neighbors >= 4 {
            pocket_fills.push(i);
        }
    }
    for idx in pocket_fills {
        gen[idx].terrain = TerrainType::Beach;
    }
}

// ── Inland distance (per-island BFS from coast) ──────────────────────────────

/// BFS distance from the coastal edge of an island (tiles touching water or the
/// outside of the land set are distance 0). Used as a feature gate (mountains
/// need depth) and a soft suppressor for forests close to the shore.
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

// ── Procedural island terrain (center + crescents) ───────────────────────────

fn collect_classified_islands(
    gen: &[TileGen],
) -> Vec<(u8, Vec<HexCoord>, FxHashMap<HexCoord, i32>)> {
    let mut out: Vec<(u8, Vec<HexCoord>, FxHashMap<HexCoord, i32>)> = Vec::with_capacity(4);

    // Freshwater is excluded so forest/mountain budgets ignore lake tiles.
    let mut center: Vec<HexCoord> = Vec::new();
    let mut buckets: [Vec<HexCoord>; 3] = Default::default();
    for t in gen {
        if t.terrain == TerrainType::Freshwater {
            continue;
        }
        match t.island {
            Some(Island::Center) => center.push(t.coord),
            Some(Island::Crescent(i)) => buckets[i as usize].push(t.coord),
            None => {}
        }
    }
    if !center.is_empty() {
        let inland = inland_distances(&center);
        out.push((crate::outer_islands::CENTER_ISLAND_ID, center, inland));
    }
    for (i, land) in buckets.into_iter().enumerate() {
        if land.is_empty() {
            continue;
        }
        let inland = inland_distances(&land);
        out.push((i as u8, land, inland));
    }
    out
}

fn assign_island_terrain(gen: &mut [TileGen], seed: u64, by_coord: &TileIndex) {
    let islands = collect_classified_islands(gen);
    let assignments = crate::outer_islands::classify_all(seed, &islands);

    for (coord, terrain) in assignments {
        if let Some(idx) = by_coord.get(coord) {
            // Beach (shoreline ring + lake fringes) and Freshwater (lake water)
            // are already in place — the noise-driven biome classifier never
            // overwrites either.
            if gen[idx].terrain == TerrainType::Beach || is_water(gen[idx].terrain) {
                continue;
            }
            gen[idx].terrain = terrain;
        }
    }
}

// ── Water depth (Coast / Ocean / DeepOcean) ──────────────────────────────────

fn assign_water_depth(gen: &mut [TileGen], by_coord: &TileIndex) {
    // BFS distance from land for every water tile, but stored in a flat
    // `Vec<i32>` indexed by `gen` index instead of a hash map. `-1` = "not yet
    // visited". Land is also marked 0 so we can skip the land-vs-water check
    // entirely while traversing neighbours.
    let n = gen.len();
    let mut dist_to_land: Vec<i32> = vec![-1; n];
    let mut queue: VecDeque<usize> = VecDeque::with_capacity(n / 8);

    for (i, tile) in gen.iter().enumerate() {
        if tile.is_land() {
            dist_to_land[i] = 0;
            queue.push_back(i);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let d = dist_to_land[idx];
        let coord = gen[idx].coord;
        for n_coord in coord.neighbors() {
            let Some(nidx) = by_coord.get(n_coord) else {
                continue;
            };
            if dist_to_land[nidx] >= 0 {
                continue;
            }
            dist_to_land[nidx] = d + 1;
            queue.push_back(nidx);
        }
    }

    for (i, tile) in gen.iter_mut().enumerate() {
        if tile.is_land() {
            continue;
        }
        let d = if dist_to_land[i] < 0 {
            DEEP_OCEAN_MIN_DIST
        } else {
            dist_to_land[i]
        };
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

/// Replace any procedurally-classified land tile whose neighbours contain ZERO
/// of the same terrain type with the modal non-water, non-beach neighbour.
fn speckle_cleanup(gen: &mut [TileGen], by_coord: &TileIndex) {
    let mut changes: Vec<(usize, TerrainType)> = Vec::new();
    let mut counts: FxHashMap<TerrainType, i32> =
        FxHashMap::with_capacity_and_hasher(8, Default::default());

    for (i, tile) in gen.iter().enumerate() {
        if !tile.uses_procedural_terrain() {
            continue;
        }
        // Beach and Freshwater are leave-alone surfaces.
        if tile.terrain == TerrainType::Beach || is_water(tile.terrain) {
            continue;
        }
        // Mountains are shaped in outer_islands cleanup; speckle would erase
        // isolated SnowPeak tiles ringed by StonySlope.
        if matches!(
            tile.terrain,
            TerrainType::StonySlope | TerrainType::SnowPeak
        ) {
            continue;
        }
        let mut same = 0;
        let mut land_neighbors = 0;
        counts.clear();

        for n in tile.coord.neighbors() {
            let Some(idx) = by_coord.get(n) else {
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
        if let Some((replacement, _)) = counts.iter().max_by(|a, b| {
            a.1.cmp(b.1)
                .then_with(|| (*a.0 as u8).cmp(&(*b.0 as u8)))
        }) {
            changes.push((i, *replacement));
        }
    }

    for (idx, terrain) in changes {
        gen[idx].terrain = terrain;
    }
}

// ── Helpers / logging ────────────────────────────────────────────────────────

#[cfg(test)]
fn is_land_biome(t: TerrainType) -> bool {
    !is_water(t)
}

fn terrain_name(t: TerrainType) -> &'static str {
    match t {
        TerrainType::DeepOcean => "DeepOcean",
        TerrainType::Ocean => "Ocean",
        TerrainType::Coast => "Coast",
        TerrainType::Freshwater => "Freshwater",
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
    let mut counts: FxHashMap<TerrainType, usize> =
        FxHashMap::with_capacity_and_hasher(32, Default::default());
    for tile in tiles {
        *counts.entry(tile.terrain).or_default() += 1;
    }

    let reported = [
        TerrainType::DeepOcean,
        TerrainType::Ocean,
        TerrainType::Coast,
        TerrainType::Freshwater,
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
    use std::collections::HashMap;

    // Steppe, Hills, and AridPeak are intentionally disabled — the generator
    // never emits them. Freshwater is the new inland-lake tile; it appears on
    // outer islands at the centre of lakes, surrounded by Beach.
    // must not emit them on outer islands until they are re-enabled.
    const ALLOWED_OUTER_LAND: &[TerrainType] = &[
        TerrainType::Beach,
        TerrainType::Freshwater,
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

    fn center_land(map: &Map, seed: u64) -> Vec<HexCoord> {
        let noise = NoiseCtx::new(seed);
        map.tiles
            .iter()
            .filter(|t| {
                classify_island(&noise, t.coord.q, t.coord.r) == Some(Island::Center)
            })
            .map(|t| t.coord)
            .collect()
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
        for &coord in &center_land(&map, 42) {
            let t = terrains[&coord];
            assert!(
                ALLOWED_OUTER_LAND.contains(&t),
                "center tile {:?} has unallowed terrain {:?}",
                coord,
                t
            );
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

        // Every ocean-side water tile adjacent to land must be Coast.
        // Freshwater (inland lakes) is water too but lives on land and is not
        // part of the ocean depth gradient — skip it here.
        for tile in &map.tiles {
            if !is_water(tile.terrain) {
                continue;
            }
            if tile.terrain == TerrainType::Freshwater {
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
        // Per-island contract: both active base biomes plus at least one
        // forest and one mountain tile. Specific forest types are NOT required
        // per island — patch-unification can drop a minority type from a
        // crescent. That's exercised at the map-wide level below.
        // Steppe / AridPeak / Hills are intentionally not generated.
        let required_bases = [TerrainType::Plains, TerrainType::Greenfield];
        let all_forest_types = [
            TerrainType::Oldwood,
            TerrainType::Darkpine,
            TerrainType::Deepjungle,
        ];

        for seed in [42u64, 123, 999, 7777] {
            let map = Map::generate(MAP_RADIUS, seed);
            let by_island = crescent_land_by_island(&map, seed);
            let terrains: HashMap<HexCoord, TerrainType> =
                map.tiles.iter().map(|t| (t.coord, t.terrain)).collect();

            let mut map_wide_forests: std::collections::HashSet<TerrainType> =
                std::collections::HashSet::new();

            let islands: [(&str, Vec<HexCoord>); 4] = [
                ("crescent 0", by_island[0].clone()),
                ("crescent 1", by_island[1].clone()),
                ("crescent 2", by_island[2].clone()),
                ("center", center_land(&map, seed)),
            ];
            for (label, land) in islands {
                let present: std::collections::HashSet<TerrainType> = land
                    .iter()
                    .filter_map(|c| terrains.get(c).copied())
                    .collect();

                for t in required_bases {
                    assert!(
                        present.contains(&t),
                        "seed {seed} {label} missing base biome {:?}",
                        t
                    );
                }
                let has_forest = present.iter().any(|t| is_outer_forest(*t));
                assert!(has_forest, "seed {seed} {label} has no forest");
                let has_mountain = present.iter().any(|t| is_outer_mountain(*t));
                assert!(
                    has_mountain,
                    "seed {seed} {label} has no mountain tile"
                );

                for t in all_forest_types {
                    if present.contains(&t) {
                        map_wide_forests.insert(t);
                    }
                }
            }

            // Across the three crescents combined, every forest type should
            // show up somewhere so the world isn't missing a flavour entirely.
            for t in all_forest_types {
                assert!(
                    map_wide_forests.contains(&t),
                    "seed {seed}: no crescent has forest type {:?}",
                    t
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

        let mut all_land: Vec<(String, Vec<HexCoord>)> = by_island
            .iter()
            .enumerate()
            .map(|(i, land)| (format!("crescent {i}"), land.clone()))
            .collect();
        all_land.push(("center".into(), center_land(&map, 42)));

        for (label, land) in all_land {
            for &coord in &land {
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
                        "isolated tile on {label} at {:?} (terrain {:?})",
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
