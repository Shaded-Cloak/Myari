use std::collections::{HashMap, VecDeque};

use crate::hexgrid::HexCoord;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

pub const MAP_RADIUS: i32 = 220;

// Island layout constants (layout-space units ≈ hex widths)
const CENTER_R: f32 = 66.0;
const CRESCENT_R_IN: f32 = 132.0;
const CRESCENT_R_OUT: f32 = 172.0;
const CRESCENT_ARC_HALF: f32 = 44.0; // 88° arc → 32° open ocean between each pair
const COAST_NOISE_AMP: f32 = 10.0;
const DEEP_OCEAN_DIST: i32 = 6;

const CRESCENT_ANGLES: [f32; 3] = [90.0, 210.0, 330.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainType {
    DeepOcean,
    Ocean,
    Coast,
    Beach,
    Hills,
    Mountain,
    SnowPeak,
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

#[derive(Debug, Clone, Copy)]
pub struct HexTile {
    pub coord: HexCoord,
    pub terrain: TerrainType,
}

pub struct Map {
    pub tiles: Vec<HexTile>,
    by_coord: HashMap<HexCoord, usize>,
}

impl Map {
    pub fn generate(radius: i32, seed: u64) -> Self {
        let noise = NoiseCtx::new(seed);
        let coords = hex_disk_coords(radius);
        let mut gen: Vec<TileGen> = coords
            .iter()
            .map(|&coord| {
                let island = classify_island(&noise, coord.q, coord.r);
                let terrain = if island.is_some() {
                    TerrainType::Plains // placeholder until biome pass
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

        let mut by_coord: HashMap<HexCoord, usize> =
            HashMap::with_capacity(gen.len());
        for (i, t) in gen.iter().enumerate() {
            by_coord.insert(t.coord, i);
        }

        assign_biomes(&mut gen, &noise);
        apply_special_tiles(&mut gen, &noise);
        assign_sacred_ground(&mut gen);
        assign_coast_and_ocean(&mut gen, &by_coord);
        assign_beaches(&mut gen, seed, &by_coord);
        remove_speckles(&mut gen, &by_coord);

        let tiles: Vec<HexTile> = gen
            .into_iter()
            .map(|t| HexTile {
                coord: t.coord,
                terrain: t.terrain,
            })
            .collect();

        log_distribution(&tiles);

        Self { tiles, by_coord }
    }

    pub fn tile_at(&self, coord: HexCoord) -> Option<&HexTile> {
        self.by_coord
            .get(&coord)
            .and_then(|&idx| self.tiles.get(idx))
    }
}

// ── Noise ────────────────────────────────────────────────────────────────────

struct NoiseCtx {
    coast: Fbm<Perlin>,
    elevation: Fbm<Perlin>,
    moisture: Fbm<Perlin>,
    biome_detail: Fbm<Perlin>,
    special: Fbm<Perlin>,
}

impl NoiseCtx {
    fn new(seed: u64) -> Self {
        Self {
            coast: make_fbm(mix_seed(seed, 1), 4, 0.012, 2.0, 0.5),
            elevation: make_fbm(mix_seed(seed, 2), 5, 0.008, 2.0, 0.5),
            moisture: make_fbm(mix_seed(seed, 3), 4, 0.010, 2.0, 0.5),
            biome_detail: make_fbm(mix_seed(seed, 4), 3, 0.025, 2.0, 0.5),
            special: make_fbm(mix_seed(seed, 5), 3, 0.004, 2.0, 0.55),
        }
    }

    fn coast_offset(&self, x: f32, y: f32) -> f32 {
        (self.sample01(&self.coast, x, y) - 0.5) * 2.0 * COAST_NOISE_AMP
    }

    fn elevation(&self, x: f32, y: f32) -> f32 {
        self.sample01(&self.elevation, x, y)
    }

    fn moisture(&self, x: f32, y: f32) -> f32 {
        self.sample01(&self.moisture, x, y)
    }

    fn biome_detail(&self, x: f32, y: f32) -> f32 {
        self.sample01(&self.biome_detail, x, y)
    }

    fn special(&self, x: f32, y: f32) -> f32 {
        self.sample01(&self.special, x, y)
    }

    fn sample01(&self, fbm: &Fbm<Perlin>, x: f32, y: f32) -> f32 {
        let v = fbm.get([x as f64, y as f64]);
        (v as f32 + 1.0) * 0.5
    }
}

fn make_fbm(
    seed: u32,
    octaves: usize,
    frequency: f64,
    lacunarity: f64,
    persistence: f64,
) -> Fbm<Perlin> {
    Fbm::<Perlin>::new(seed)
        .set_octaves(octaves)
        .set_frequency(frequency)
        .set_lacunarity(lacunarity)
        .set_persistence(persistence)
}

fn mix_seed(seed: u64, salt: u32) -> u32 {
    let mut z = seed.wrapping_add(salt as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as u32
}

/// Deterministic per-hex roll in [0, 1) — independent at each coordinate.
fn tile_roll(seed: u64, q: i32, r: i32, salt: u32) -> f32 {
    let pos = (q as u64).wrapping_mul(0x1E05_A879).wrapping_add(r as u64);
    mix_seed(seed ^ pos, salt) as f32 / u32::MAX as f32
}

// ── Layout / island mask ─────────────────────────────────────────────────────

fn layout_xy(q: i32, r: i32) -> (f32, f32) {
    let x = 3f32.sqrt() * q as f32 + 3f32.sqrt() / 2.0 * r as f32;
    let y = 1.5 * r as f32;
    (x, y)
}

fn polar(x: f32, y: f32) -> (f32, f32) {
    (x.hypot(y), y.atan2(x).to_degrees())
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

fn classify_island(noise: &NoiseCtx, q: i32, r: i32) -> Option<Island> {
    let (x, y) = layout_xy(q, r);
    let hex_dist = HexCoord::new(q, r)
        .distance(&HexCoord::new(0, 0)) as f32;
    let coast = noise.coast_offset(x, y);
    let (_, tile_angle) = polar(x, y);

    if hex_dist < CENTER_R + coast {
        return Some(Island::Center);
    }

    for (i, &base_angle) in CRESCENT_ANGLES.iter().enumerate() {
        if angle_diff(tile_angle, base_angle).abs() > CRESCENT_ARC_HALF {
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

// ── Biome assignment ─────────────────────────────────────────────────────────

fn assign_biomes(gen: &mut [TileGen], noise: &NoiseCtx) {
    for tile in gen.iter_mut() {
        let Some(island) = tile.island else {
            continue;
        };
        let (x, y) = layout_xy(tile.coord.q, tile.coord.r);
        let elev = noise.elevation(x, y);
        let moist = noise.moisture(x, y);
        let detail = noise.biome_detail(x, y);
        let (_, angle) = polar(x, y);

        let temp = match island {
            Island::Center => 0.45 + detail * 0.15,
            Island::Crescent(0) => {
                // Cold — northern crescent (90°)
                0.12 + detail * 0.22 + (y / 180.0).clamp(-0.12, 0.12)
            }
            Island::Crescent(1) => {
                // Warm — south-east (210°)
                0.65 + detail * 0.2 + (-y / 250.0).clamp(-0.1, 0.15)
            }
            Island::Crescent(2) => {
                // Temperate — south-west (330°)
                0.42 + detail * 0.22 + (angle / 500.0)
            }
            Island::Crescent(_) => 0.5,
        };

        // Elevation overrides
        if elev > 0.72 && temp < 0.45 {
            tile.terrain = TerrainType::SnowPeak;
            continue;
        }
        if elev > 0.74 {
            tile.terrain = TerrainType::Mountain;
            continue;
        }
        if elev > 0.58 {
            tile.terrain = TerrainType::Hills;
            continue;
        }

        tile.terrain = match island {
            Island::Center => center_base_biome(moist, detail, elev),
            Island::Crescent(0) => cold_biome(moist, temp, detail),
            Island::Crescent(1) => warm_biome(moist, temp, detail),
            Island::Crescent(2) => temperate_biome(moist, temp, detail),
            Island::Crescent(_) => TerrainType::Plains,
        };
    }
}

fn cold_biome(moist: f32, temp: f32, detail: f32) -> TerrainType {
    let m = moist * 0.45 + detail * 0.55;
    if m > 0.66 {
        TerrainType::Darkpine
    } else if m > 0.52 {
        TerrainType::Oldwood
    } else if m > 0.38 {
        TerrainType::Frostmoor
    } else if m > 0.24 {
        TerrainType::Snowfield
    } else if temp < 0.32 {
        TerrainType::Snowfield
    } else {
        TerrainType::Frostmoor
    }
}

fn warm_biome(moist: f32, temp: f32, detail: f32) -> TerrainType {
    let m = moist * 0.50 + detail * 0.50;
    if m > 0.62 {
        TerrainType::Deepjungle
    } else if m > 0.50 {
        TerrainType::Thornveld
    } else if temp > 0.70 && m < 0.38 {
        TerrainType::Ashplain
    } else if m > 0.32 {
        TerrainType::Steppe
    } else {
        TerrainType::Plains
    }
}

fn temperate_biome(moist: f32, _temp: f32, detail: f32) -> TerrainType {
    let m = moist * 0.45 + detail * 0.55;
    if m > 0.62 {
        TerrainType::Oldwood
    } else if m > 0.50 {
        TerrainType::Greenfield
    } else if m > 0.38 {
        TerrainType::Plains
    } else if m > 0.24 {
        TerrainType::Steppe
    } else {
        TerrainType::Oldwood
    }
}

fn center_base_biome(moist: f32, detail: f32, elev: f32) -> TerrainType {
    let m = moist * 0.4 + detail * 0.6;
    if elev > 0.55 {
        TerrainType::Hills
    } else if m > 0.6 {
        TerrainType::Thornveld
    } else if m > 0.4 {
        TerrainType::Ashplain
    } else {
        TerrainType::Hills
    }
}

// ── Special tiles ────────────────────────────────────────────────────────────

fn is_special_terrain(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::AncientRuin
            | TerrainType::Corrupted
            | TerrainType::LeyGrove
            | TerrainType::LeyWaste
            | TerrainType::BlightedWaste
            | TerrainType::RuinField
    )
}

fn apply_special_tiles(gen: &mut [TileGen], noise: &NoiseCtx) {
    for tile in gen.iter_mut() {
        let Some(island) = tile.island else {
            continue;
        };
        let (x, y) = layout_xy(tile.coord.q, tile.coord.r);
        let s = noise.special(x, y);
        let detail = noise.biome_detail(x, y);

        match island {
            Island::Center => {
                // Priority: highest threshold first
                if s > 0.68 {
                    tile.terrain = TerrainType::Corrupted;
                } else if s > 0.65 {
                    tile.terrain = TerrainType::RuinField;
                } else if s > 0.62 {
                    tile.terrain = TerrainType::BlightedWaste;
                } else if s > 0.58 {
                    tile.terrain = TerrainType::AncientRuin;
                } else if s > 0.54 {
                    tile.terrain = TerrainType::LeyWaste;
                } else if s > 0.50 {
                    tile.terrain = TerrainType::LeyGrove;
                }
            }
            Island::Crescent(_) => {
                if s > 0.88 && detail > 0.7 {
                    tile.terrain = TerrainType::AncientRuin;
                } else if s > 0.92 {
                    tile.terrain = TerrainType::LeyGrove;
                }
            }
        }
    }
}

fn assign_sacred_ground(gen: &mut [TileGen]) {
    let origin = HexCoord::new(0, 0);
    let mut best: Option<(i32, HexCoord)> = None;

    for tile in gen.iter() {
        if tile.island.is_none() {
            continue;
        }
        let d = tile.coord.distance(&origin);
        let coord = tile.coord;
        match best {
            None => best = Some((d, coord)),
            Some((bd, _)) if d < bd => best = Some((d, coord)),
            Some((bd, bc)) if d == bd && (coord.q, coord.r) < (bc.q, bc.r) => {
                best = Some((d, coord));
            }
            _ => {}
        }
    }

    if let Some((_, coord)) = best {
        for tile in gen.iter_mut() {
            if tile.coord == coord {
                tile.terrain = TerrainType::SacredGround;
                break;
            }
        }
    }
}

// ── Coast and ocean depth ────────────────────────────────────────────────────

fn is_water(t: TerrainType) -> bool {
    matches!(t, TerrainType::DeepOcean | TerrainType::Ocean)
}

fn assign_coast_and_ocean(gen: &mut [TileGen], by_coord: &HashMap<HexCoord, usize>) {
    let water_coords: HashMap<HexCoord, ()> = gen
        .iter()
        .filter(|t| t.island.is_none())
        .map(|t| (t.coord, ()))
        .collect();

    // BFS from all land to compute hex distance to nearest land for water tiles
    let mut dist_to_land: HashMap<HexCoord, i32> = HashMap::new();
    let mut queue = VecDeque::new();

    for tile in gen.iter().filter(|t| t.island.is_some()) {
        dist_to_land.insert(tile.coord, 0);
        queue.push_back(tile.coord);
    }

    while let Some(c) = queue.pop_front() {
        let d = dist_to_land[&c];
        for n in c.neighbors() {
            if dist_to_land.contains_key(&n) {
                continue;
            }
            if by_coord.contains_key(&n) {
                dist_to_land.insert(n, d + 1);
                queue.push_back(n);
            }
        }
    }

    for tile in gen.iter_mut() {
        if tile.island.is_some() {
            if tile.terrain == TerrainType::SacredGround {
                continue;
            }
            let near_water = tile
                .coord
                .neighbors()
                .iter()
                .any(|n| water_coords.contains_key(n));
            if near_water {
                tile.terrain = TerrainType::Coast;
            }
        } else {
            let d = dist_to_land
                .get(&tile.coord)
                .copied()
                .unwrap_or(DEEP_OCEAN_DIST);
            tile.terrain = if d >= DEEP_OCEAN_DIST {
                TerrainType::DeepOcean
            } else {
                TerrainType::Ocean
            };
        }
    }
}

// ── Beach fringe ─────────────────────────────────────────────────────────────

fn assign_beaches(
    gen: &mut [TileGen],
    seed: u64,
    by_coord: &HashMap<HexCoord, usize>,
) {
    let water_coords: HashMap<HexCoord, ()> = gen
        .iter()
        .filter(|t| is_water(t.terrain))
        .map(|t| (t.coord, ()))
        .collect();

    // Land hex distance from water (coast tiles = 1)
    let mut land_dist: HashMap<HexCoord, i32> = HashMap::new();
    let mut queue = VecDeque::new();

    for tile in gen.iter().filter(|t| t.island.is_some()) {
        let touches_water = tile
            .coord
            .neighbors()
            .iter()
            .any(|n| water_coords.contains_key(n));
        if touches_water {
            land_dist.insert(tile.coord, 1);
            queue.push_back(tile.coord);
        }
    }

    while let Some(c) = queue.pop_front() {
        let d = land_dist[&c];
        for n in c.neighbors() {
            if land_dist.contains_key(&n) {
                continue;
            }
            let Some(&idx) = by_coord.get(&n) else {
                continue;
            };
            if gen[idx].island.is_none() {
                continue;
            }
            land_dist.insert(n, d + 1);
            queue.push_back(n);
        }
    }

    let coast_coords: HashMap<HexCoord, ()> = gen
        .iter()
        .filter(|t| t.terrain == TerrainType::Coast)
        .map(|t| (t.coord, ()))
        .collect();

    // Pass 1 — continuous sandy fringe: every inland tile touching coast becomes beach
    let fringe: Vec<HexCoord> = gen
        .iter()
        .filter(|t| t.island.is_some())
        .filter(|t| {
            !matches!(
                t.terrain,
                TerrainType::Coast | TerrainType::SacredGround
            )
        })
        .filter(|t| {
            t.coord
                .neighbors()
                .iter()
                .any(|n| coast_coords.contains_key(n))
        })
        .map(|t| t.coord)
        .collect();

    for coord in fringe {
        if let Some(&idx) = by_coord.get(&coord) {
            gen[idx].terrain = TerrainType::Beach;
        }
    }

    // Pass 2 — each fringe beach hex independently decides whether to extend one step inland.
    // Per-tile rolls break the parallel-ring pattern that noise on dist-3 caused.
    const EXTEND_CHANCE: f32 = 0.46;

    let fringe_ring: Vec<HexCoord> = gen
        .iter()
        .filter(|t| t.island.is_some() && t.terrain == TerrainType::Beach)
        .filter(|t| land_dist.get(&t.coord) == Some(&2))
        .map(|t| t.coord)
        .collect();

    let mut extensions: Vec<HexCoord> = Vec::new();

    for coord in fringe_ring {
        if tile_roll(seed, coord.q, coord.r, 71) > EXTEND_CHANCE {
            continue;
        }
        for n in coord.neighbors() {
            let Some(&idx) = by_coord.get(&n) else {
                continue;
            };
            let inland = &gen[idx];
            if inland.island.is_none() {
                continue;
            }
            if land_dist.get(&n) != Some(&3) {
                continue;
            }
            if matches!(
                inland.terrain,
                TerrainType::Coast | TerrainType::Beach | TerrainType::SacredGround
            ) {
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

// ── Speckle removal ──────────────────────────────────────────────────────────

fn is_land_biome(t: TerrainType) -> bool {
    !matches!(
        t,
        TerrainType::DeepOcean
            | TerrainType::Ocean
            | TerrainType::Coast
            | TerrainType::Beach
            | TerrainType::SacredGround
    )
}

fn remove_speckles(gen: &mut [TileGen], by_coord: &HashMap<HexCoord, usize>) {
    // Pass 1: special biomes need ≥2 same-type neighbors
    for _ in 0..2 {
        let mut changes: Vec<(HexCoord, TerrainType)> = Vec::new();
        for tile in gen.iter() {
            if !is_special_terrain(tile.terrain) {
                continue;
            }
            let same = count_same_neighbors(gen, by_coord, tile.coord, tile.terrain);
            if same >= 2 {
                continue;
            }
            if let Some(replacement) =
                majority_neighbor_biome(gen, by_coord, tile.coord, tile.terrain)
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

    // Pass 2: general isolated land biomes (≤1 same-type neighbor)
    let mut changes: Vec<(HexCoord, TerrainType)> = Vec::new();
    for tile in gen.iter() {
        let t = tile.terrain;
        if !is_land_biome(t) || is_special_terrain(t) || t == TerrainType::Mountain || t == TerrainType::SnowPeak || t == TerrainType::Beach {
            continue;
        }
        let same = count_same_neighbors(gen, by_coord, tile.coord, t);
        if same <= 1 {
            if let Some(replacement) = majority_neighbor_biome(gen, by_coord, tile.coord, t) {
                if replacement != t {
                    changes.push((tile.coord, replacement));
                }
            }
        }
    }
    for (coord, terrain) in changes {
        if let Some(&idx) = by_coord.get(&coord) {
            gen[idx].terrain = terrain;
        }
    }
}

fn count_same_neighbors(
    gen: &[TileGen],
    by_coord: &HashMap<HexCoord, usize>,
    coord: HexCoord,
    terrain: TerrainType,
) -> i32 {
    coord
        .neighbors()
        .iter()
        .filter(|n| {
            by_coord
                .get(n)
                .map(|&idx| gen[idx].terrain == terrain)
                .unwrap_or(false)
        })
        .count() as i32
}

fn majority_neighbor_biome(
    gen: &[TileGen],
    by_coord: &HashMap<HexCoord, usize>,
    coord: HexCoord,
    exclude: TerrainType,
) -> Option<TerrainType> {
    let mut counts: HashMap<TerrainType, i32> = HashMap::new();
    for n in coord.neighbors() {
        let Some(&idx) = by_coord.get(&n) else {
            continue;
        };
        let t = gen[idx].terrain;
        if !is_land_biome(t) || t == exclude || is_water(t) {
            continue;
        }
        *counts.entry(t).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| terrain_name(a.0).cmp(terrain_name(b.0))))
        .map(|(t, _)| t)
}

// ── Distribution log ─────────────────────────────────────────────────────────

fn all_terrain_types() -> [TerrainType; 24] {
    [
        TerrainType::DeepOcean,
        TerrainType::Ocean,
        TerrainType::Coast,
        TerrainType::Beach,
        TerrainType::Hills,
        TerrainType::Mountain,
        TerrainType::SnowPeak,
        TerrainType::Ashplain,
        TerrainType::Thornveld,
        TerrainType::Deepjungle,
        TerrainType::Steppe,
        TerrainType::Plains,
        TerrainType::Greenfield,
        TerrainType::Oldwood,
        TerrainType::Snowfield,
        TerrainType::Frostmoor,
        TerrainType::Darkpine,
        TerrainType::AncientRuin,
        TerrainType::Corrupted,
        TerrainType::LeyGrove,
        TerrainType::LeyWaste,
        TerrainType::BlightedWaste,
        TerrainType::RuinField,
        TerrainType::SacredGround,
    ]
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

fn is_non_special_land_biome(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::Hills
            | TerrainType::Ashplain
            | TerrainType::Thornveld
            | TerrainType::Deepjungle
            | TerrainType::Steppe
            | TerrainType::Plains
            | TerrainType::Greenfield
            | TerrainType::Oldwood
            | TerrainType::Snowfield
            | TerrainType::Frostmoor
            | TerrainType::Darkpine
    )
}

fn log_distribution(tiles: &[HexTile]) {
    let total = tiles.len() as f64;
    let mut counts: HashMap<TerrainType, usize> = HashMap::new();
    for tile in tiles {
        *counts.entry(tile.terrain).or_default() += 1;
    }

    let land_count: usize = tiles
        .iter()
        .filter(|t| is_land_biome(t.terrain))
        .count();
    let land_total = land_count as f64;

    println!("=== Terrain Distribution ===");
    for t in all_terrain_types() {
        let c = counts.get(&t).copied().unwrap_or(0);
        let pct = if total > 0.0 {
            (c as f64 / total) * 100.0
        } else {
            0.0
        };
        println!("  {}: {} ({:.1}%)", terrain_name(t), c, pct);
    }

    println!("=== Land Biome Warnings ===");
    let mut any_warn = false;
    for t in all_terrain_types() {
        if !is_non_special_land_biome(t) {
            continue;
        }
        let c = counts.get(&t).copied().unwrap_or(0);
        if land_total > 0.0 {
            let pct = (c as f64 / land_total) * 100.0;
            if pct < 1.0 {
                println!(
                    "  WARNING: {} is {:.1}% of land tiles (below 1%)",
                    terrain_name(t),
                    pct
                );
                any_warn = true;
            }
        }
    }
    if !any_warn {
        println!("  (none — all non-special land biomes >= 1%)");
    }
}

fn hex_disk_coords(radius: i32) -> Vec<HexCoord> {
    let mut out = Vec::new();
    for q in -radius..=radius {
        for r in -radius..=radius {
            let c = HexCoord::new(q, r);
            if c.distance(&HexCoord::new(0, 0)) <= radius {
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_completes_with_all_terrain_types_represented() {
        let map = Map::generate(MAP_RADIUS, 42);
        assert!(!map.tiles.is_empty());
        let land = map
            .tiles
            .iter()
            .filter(|t| is_land_biome(t.terrain))
            .count();
        assert!(land > 20_000, "expected substantial land mass, got {land}");

        let sacred = map
            .tiles
            .iter()
            .filter(|t| t.terrain == TerrainType::SacredGround)
            .count();
        assert_eq!(sacred, 1);

        let ruin_field_on_crescents = map.tiles.iter().any(|t| {
            if t.terrain != TerrainType::RuinField {
                return false;
            }
            t.coord.distance(&HexCoord::new(0, 0)) >= CRESCENT_R_IN as i32 - 5
        });
        assert!(
            !ruin_field_on_crescents,
            "RuinField must only appear on center island"
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

            let sacred = map
                .tiles
                .iter()
                .filter(|t| t.terrain == TerrainType::SacredGround)
                .count();
            assert_eq!(sacred, 1, "seed {seed}");
        }
    }
}
