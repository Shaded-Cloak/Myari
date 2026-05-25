use crate::hexgrid::HexCoord;
use crate::map::{HexTile, Map, TerrainType};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Yields from a tile: Food, Production, Gold
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Yields {
    pub food: i32,
    pub production: i32,
    pub gold: i32,
}

impl std::ops::Add for Yields {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Yields {
            food: self.food + rhs.food,
            production: self.production + rhs.production,
            gold: self.gold + rhs.gold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub name: String,
    pub coord: HexCoord,
    pub population: i32,
    pub claimed: Vec<HexCoord>,
    pub buildings: Vec<String>,
    pub production_queue: Vec<String>,
    pub production_progress: i32,
}

impl City {
    pub fn new(name: &str, coord: HexCoord, map: &Map) -> Self {
        let mut claimed = vec![coord];
        for n in coord.neighbors() {
            if map.tile_at(n).is_some() {
                claimed.push(n);
            }
        }
        City {
            name: name.to_string(),
            coord,
            population: 1,
            claimed,
            buildings: vec![],
            production_queue: vec![],
            production_progress: 0,
        }
    }

    pub fn compute_yields(&self, map: &Map) -> Yields {
        let mut total = Yields::default();
        for &c in &self.claimed {
            if let Some(tile) = map.tile_at(c) {
                let y = base_yields(tile.terrain);
                total = total + y;
            }
        }
        total
    }

    /// Food needed for next population: 10 + (5 * current_pop)
    pub fn growth_threshold(&self) -> i32 {
        10 + 5 * self.population
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub coord: HexCoord,
    pub owner: CivId,
    pub moves: i32,
    pub max_moves: i32,
}

impl Unit {
    pub fn new(coord: HexCoord, owner: CivId) -> Self {
        Unit {
            coord,
            owner,
            moves: 2,
            max_moves: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Civilization {
    pub name: String,
    pub color: (u8, u8, u8),
    pub cities: Vec<City>,
    pub units: Vec<Unit>,
    pub gold: i32,
    pub food: i32,
    pub production: i32,
}

impl Civilization {
    pub fn new(name: &str, color: (u8, u8, u8)) -> Self {
        Civilization {
            name: name.to_string(),
            color,
            cities: vec![],
            units: vec![],
            gold: 10,
            food: 0,
            production: 0,
        }
    }

    pub fn found_city(&mut self, name: &str, coord: HexCoord, map: &Map) {
        let city = City::new(name, coord, map);
        self.cities.push(city);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CivId {
    IronThrone,
    ArcaneConclave,
    ThornwoodClans,
}

impl CivId {
    pub fn all() -> [CivId; 3] {
        [CivId::IronThrone, CivId::ArcaneConclave, CivId::ThornwoodClans]
    }

    pub fn name(&self) -> &str {
        match self {
            CivId::IronThrone => "Iron Throne",
            CivId::ArcaneConclave => "Arcane Conclave",
            CivId::ThornwoodClans => "Thornwood Clans",
        }
    }

    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            CivId::IronThrone => (180, 60, 50),
            CivId::ArcaneConclave => (80, 100, 200),
            CivId::ThornwoodClans => (50, 130, 70),
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub turn: i32,
    pub civs: Vec<Civilization>,
}

impl GameState {
    pub fn new(map: &Map) -> Self {
        let tiles: std::collections::HashMap<HexCoord, &HexTile> =
            map.tiles.iter().map(|t| (t.coord, t)).collect();

        let min_civ_dist = 6;
        let mut civs = Vec::new();
        let mut chosen: Vec<HexCoord> = Vec::new();

        for id in CivId::all() {
            let mut civ = Civilization::new(id.name(), id.color());

            if let Some(start) = find_best_start(&map.tiles, &tiles, &chosen, min_civ_dist) {
                let parts: Vec<&str> = id.name().split_whitespace().collect();
                let short = parts.first().copied().unwrap_or("City");
                civ.found_city(&format!("{short}-throne"), start, map);

                // Place scout on nearest valid neighbor
                let scout_pos = start
                    .neighbors()
                    .iter()
                    .find(|n| {
                        tiles
                            .get(n)
                            .is_some_and(|t| is_walkable(t.terrain))
                    })
                    .copied()
                    .unwrap_or(start);
                civ.units.push(Unit::new(scout_pos, id));
                chosen.push(start);
            }

            civs.push(civ);
        }

        GameState { turn: 1, civs }
    }

    /// Player-controlled civ index (Iron Throne).
    pub const PLAYER_CIV: usize = 0;

    pub fn unit_at(&self, coord: HexCoord) -> Option<(usize, usize)> {
        for (ci, civ) in self.civs.iter().enumerate() {
            for (ui, unit) in civ.units.iter().enumerate() {
                if unit.coord == coord {
                    return Some((ci, ui));
                }
            }
        }
        None
    }

    pub fn try_move_unit(
        &mut self,
        civ_idx: usize,
        unit_idx: usize,
        dest: HexCoord,
        map: &Map,
    ) -> bool {
        if civ_idx >= self.civs.len() {
            return false;
        }
        let dest_tile = match map.tile_at(dest) {
            Some(t) => t,
            None => return false,
        };
        if !is_walkable(dest_tile.terrain) {
            return false;
        }
        if self.unit_at(dest).is_some() {
            return false;
        }

        let civ = &mut self.civs[civ_idx];
        let unit = match civ.units.get_mut(unit_idx) {
            Some(u) => u,
            None => return false,
        };
        if unit.moves <= 0 || unit.coord.distance(&dest) != 1 {
            return false;
        }
        unit.coord = dest;
        unit.moves -= 1;
        true
    }

    pub fn next_turn(&mut self, map: &Map) {
        self.turn += 1;
        for civ in &mut self.civs {
            for city in &mut civ.cities {
                let yields = city.compute_yields(map);
                civ.food += yields.food;
                civ.gold += yields.gold;
                civ.production += yields.production;

                // Population growth
                if civ.food >= city.growth_threshold() {
                    city.population += 1;
                    civ.food -= city.growth_threshold();
                }
            }
            for unit in &mut civ.units {
                unit.moves = unit.max_moves;
            }
        }
    }
}

/// Find best starting position for a civ, avoiding other civs by `min_dist`.
fn find_best_start(
    tiles: &[HexTile],
    tile_map: &std::collections::HashMap<HexCoord, &HexTile>,
    chosen: &[HexCoord],
    min_dist: i32,
) -> Option<HexCoord> {
    let mut best: Option<(HexCoord, i32)> = None;

    for tile in tiles {
        let c = tile.coord;

        // Must be settleable terrain. All water tiles (ocean depth tiers and
        // inland Freshwater) and impassable peaks are skipped.
        match tile.terrain {
            TerrainType::DeepOcean | TerrainType::Ocean | TerrainType::Coast
            | TerrainType::Freshwater | TerrainType::Mountain | TerrainType::SnowPeak
            | TerrainType::AridPeak | TerrainType::GlacialPeak => continue,
            _ => {}
        }

        // Must keep distance from other civs
        if chosen.iter().any(|oc| oc.distance(&c) < min_dist) {
            continue;
        }

        let mut score = match tile.terrain {
            TerrainType::Greenfield => 11,
            TerrainType::Plains => 10,
            TerrainType::Oldwood => 8,
            TerrainType::Steppe => 8,
            TerrainType::Deepjungle => 7,
            TerrainType::Darkpine => 7,
            TerrainType::Thornveld => 6,
            TerrainType::Hills => 6,
            TerrainType::Ashplain => 3,
            TerrainType::Frostmoor => 4,
            TerrainType::Snowfield => 3,
            TerrainType::Coast => 4,
            TerrainType::Beach => 3,
            _ => 0,
        };

        // Bonus for coast proximity (check within 2 hexes)
        for dq in -2..=2 {
            for dr in -2..=2 {
                if dq == 0 && dr == 0 {
                    continue;
                }
                let n = HexCoord::new(c.q + dq, c.r + dr);
                if n.distance(&c) > 2 {
                    continue;
                }
                if let Some(nt) = tile_map.get(&n) {
                    if nt.terrain == TerrainType::Coast {
                        score += 2;
                    }
                }
            }
        }

        if best.map_or(true, |(_, s)| score > s) {
            best = Some((c, score));
        }
    }

    best.map(|(c, _)| c)
}

/// Whether a scout can enter this terrain. Coast and Freshwater are water (one
/// ocean-side, one inland lake), so both are intentionally excluded.
pub fn is_walkable(terrain: TerrainType) -> bool {
    matches!(
        terrain,
        TerrainType::Steppe
            | TerrainType::Plains
            | TerrainType::Greenfield
            | TerrainType::Oldwood
            | TerrainType::Ashplain
            | TerrainType::Thornveld
            | TerrainType::Deepjungle
            | TerrainType::Snowfield
            | TerrainType::Frostmoor
            | TerrainType::Darkpine
            | TerrainType::Hills
            | TerrainType::StonySlope
            | TerrainType::Beach
            | TerrainType::AncientRuin
            | TerrainType::LeyGrove
            | TerrainType::RuinField
            | TerrainType::SacredGround
    )
}

/// Base terrain yields (Food, Production, Gold).
pub fn terrain_yields(terrain: TerrainType) -> Yields {
    match terrain {
        TerrainType::DeepOcean | TerrainType::Ocean => Yields::default(),
        TerrainType::Coast => Yields { food: 1, production: 0, gold: 2 },
        TerrainType::Freshwater => Yields { food: 2, production: 0, gold: 1 },
        TerrainType::Beach => Yields { food: 1, production: 0, gold: 1 },
        TerrainType::Ashplain => Yields { food: 1, production: 0, gold: 0 },
        TerrainType::Thornveld => Yields { food: 1, production: 0, gold: 0 },
        TerrainType::Deepjungle => Yields { food: 2, production: 0, gold: 1 },
        TerrainType::Steppe => Yields { food: 1, production: 1, gold: 0 },
        TerrainType::Plains => Yields { food: 2, production: 1, gold: 0 },
        TerrainType::Greenfield => Yields { food: 3, production: 0, gold: 0 },
        TerrainType::Oldwood => Yields { food: 1, production: 2, gold: 0 },
        TerrainType::Snowfield => Yields::default(),
        TerrainType::Frostmoor => Yields { food: 0, production: 1, gold: 0 },
        TerrainType::Darkpine => Yields { food: 1, production: 1, gold: 0 },
        TerrainType::Hills => Yields { food: 0, production: 2, gold: 0 },
        TerrainType::StonySlope => Yields { food: 0, production: 2, gold: 0 },
        TerrainType::Mountain | TerrainType::SnowPeak => Yields { food: 0, production: 1, gold: 1 },
        TerrainType::AridPeak | TerrainType::GlacialPeak => Yields { food: 0, production: 1, gold: 1 },
        TerrainType::AncientRuin => Yields { food: 0, production: 0, gold: 3 },
        TerrainType::Corrupted | TerrainType::BlightedWaste => Yields::default(),
        TerrainType::LeyGrove => Yields { food: 1, production: 1, gold: 2 },
        TerrainType::LeyWaste => Yields { food: 0, production: 2, gold: 1 },
        TerrainType::RuinField => Yields { food: 0, production: 1, gold: 2 },
        TerrainType::SacredGround => Yields { food: 2, production: 2, gold: 2 },
    }
}

fn base_yields(terrain: TerrainType) -> Yields {
    terrain_yields(terrain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coast_has_some_value() {
        let y = terrain_yields(TerrainType::Coast);
        assert!(y.food > 0 || y.gold > 0 || y.production > 0);
    }

    #[test]
    fn deep_ocean_is_empty() {
        let y = terrain_yields(TerrainType::DeepOcean);
        assert_eq!(y.food, 0);
        assert_eq!(y.production, 0);
        assert_eq!(y.gold, 0);
    }
}

