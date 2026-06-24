//! Per-tile game on huntable forest hexes; auto hunters pick targets in lodge range.

use serde::{Deserialize, Serialize};

use crate::buildings::{coords_in_lodge_hunt_range, PlacedLodges};
use crate::map::{Map, TerrainType};
use crate::rng::tile_roll;
use std::collections::HashSet;

/// Fraction of huntable forest tiles that start with game on map generation.
pub const WILDLIFE_TILE_DENSITY: f32 = 0.28;
/// Chance per turn that an empty huntable tile near a lodge spawns game.
pub const LODGE_NEARBY_SPAWN_CHANCE: f32 = 0.15;
pub const GAME_STOCK_MIN: u8 = 1;
pub const GAME_STOCK_MAX: u8 = 3;
pub const DEPLETED_RECOVERY_TURNS: u8 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WildlifeState {
    #[default]
    None,
    Present { stock: u8 },
    Depleted { turns_until_recovery: u8 },
}

/// Forest biomes that can hold game (matches Examiner "Forest" category).
pub fn is_forest_terrain(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::Oldwood | TerrainType::Darkpine | TerrainType::Deepjungle
    )
}

pub fn is_huntable_terrain(t: TerrainType) -> bool {
    is_forest_terrain(t)
}

/// Abstract stock ceiling per biome (yield tuning for hunters).
pub fn wildlife_cap_for_terrain(t: TerrainType) -> u8 {
    match t {
        TerrainType::Oldwood => 18,
        TerrainType::Darkpine => 12,
        TerrainType::Deepjungle => 8,
        _ => 0,
    }
}

pub fn has_present_game(state: WildlifeState) -> bool {
    matches!(
        state,
        WildlifeState::Present { stock } if (GAME_STOCK_MIN..=GAME_STOCK_MAX).contains(&stock)
    )
}

pub fn game_stock(state: WildlifeState) -> Option<u8> {
    match state {
        WildlifeState::Present { stock }
            if (GAME_STOCK_MIN..=GAME_STOCK_MAX).contains(&stock) =>
        {
            Some(stock)
        }
        _ => None,
    }
}

pub fn examiner_game_line(terrain: TerrainType, state: WildlifeState) -> String {
    if !is_forest_terrain(terrain) {
        return "—".to_string();
    }
    match state {
        WildlifeState::None => "None".to_string(),
        WildlifeState::Present { stock } => format!("Present ({stock})"),
        WildlifeState::Depleted {
            turns_until_recovery,
        } => format!("Depleted ({turns_until_recovery})"),
    }
}

fn clamp_stock(stock: u8) -> u8 {
    stock.clamp(GAME_STOCK_MIN, GAME_STOCK_MAX)
}

/// Uniform 1, 2, or 3 from a deterministic roll.
pub fn roll_game_stock(seed: u64, q: i32, r: i32) -> u8 {
    let roll = tile_roll(seed, q, r, 0x574D);
    GAME_STOCK_MIN + ((roll * 3.0).floor() as u8).min(2)
}

/// Strip game from non-forest tiles (e.g. old saves or bad data).
pub fn normalize_wildlife(map: &mut Map) {
    for tile in map.tiles.iter_mut() {
        if !is_forest_terrain(tile.terrain) {
            tile.wildlife = WildlifeState::None;
            continue;
        }
        if let WildlifeState::Present { stock } = tile.wildlife {
            tile.wildlife = WildlifeState::Present {
                stock: clamp_stock(stock),
            };
        }
        if let WildlifeState::Depleted {
            turns_until_recovery,
        } = tile.wildlife
        {
            if turns_until_recovery > DEPLETED_RECOVERY_TURNS {
                tile.wildlife = WildlifeState::Depleted {
                    turns_until_recovery: DEPLETED_RECOVERY_TURNS,
                };
            }
        }
    }
}

pub fn seed_wildlife(map: &mut Map, seed: u64) {
    normalize_wildlife(map);
    for tile in map.tiles.iter_mut() {
        if !is_forest_terrain(tile.terrain) {
            continue;
        }
        if tile_roll(seed, tile.coord.q, tile.coord.r, 0x574C) < WILDLIFE_TILE_DENSITY {
            tile.wildlife = WildlifeState::Present {
                stock: roll_game_stock(seed, tile.coord.q, tile.coord.r),
            };
        }
    }
}

/// Regenerate depleted tiles and roll spawns near lodges.
pub fn wildlife_turn_tick(map: &mut Map, lodges: &PlacedLodges, seed: u64, turn: i32) {
    for tile in map.tiles.iter_mut() {
        if let WildlifeState::Depleted {
            turns_until_recovery,
        } = tile.wildlife
        {
            if turns_until_recovery <= 1 {
                tile.wildlife = WildlifeState::None;
            } else {
                tile.wildlife = WildlifeState::Depleted {
                    turns_until_recovery: turns_until_recovery - 1,
                };
            }
        }
    }

    let turn_seed = seed.wrapping_add(turn.max(0) as u64);
    let mut spawn_coords = HashSet::new();
    for lodge in &lodges.lodges {
        spawn_coords.extend(coords_in_lodge_hunt_range(lodge));
    }

    for coord in spawn_coords {
        let Some(tile) = map.tile_at_mut(coord) else {
            continue;
        };
        if !is_huntable_terrain(tile.terrain) || tile.wildlife != WildlifeState::None {
            continue;
        }
        if tile_roll(turn_seed, coord.q, coord.r, 0x534E) < LODGE_NEARBY_SPAWN_CHANCE {
            tile.wildlife = WildlifeState::Present { stock: 1 };
        }
    }
}
