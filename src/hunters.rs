//! Auto hunter units: score nearby game tiles, path, hunt, return, deposit at lodge.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::Resource;

use crate::buildings::{coords_in_lodge_hunt_range, lodge_coords, PlacedLodges, PlacedLodge};
use crate::game::{is_walkable, GameState};
use crate::hexgrid::HexCoord;
use crate::map::Map;
use crate::wildlife::{
    game_stock, has_present_game, is_huntable_terrain, wildlife_cap_for_terrain, WildlifeState,
    DEPLETED_RECOVERY_TURNS,
};

pub const LODGE_HUNTER_COUNT: usize = 3;
pub const HUNTER_MOVES_PER_TURN: i32 = 2;

const SCORE_STOCK_WEIGHT: f32 = 3.0;
const SCORE_BIOME_WEIGHT: f32 = 0.05;
const SCORE_DIST_PENALTY: f32 = 1.5;
const SCORE_RESERVED_PENALTY: f32 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunterState {
    Idle,
    MovingToHunt,
    Hunting,
    Returning,
}

#[derive(Debug, Clone)]
pub struct Hunter {
    pub coord: HexCoord,
    pub lodge_idx: usize,
    pub state: HunterState,
    pub target: Option<HexCoord>,
    pub path: Vec<HexCoord>,
    pub carried_food: i32,
}

#[derive(Debug, Default, Resource)]
pub struct Hunters {
    pub hunters: Vec<Hunter>,
}

impl Hunters {
    pub fn clear(&mut self) {
        self.hunters.clear();
    }

    pub fn count_for_lodge(&self, lodge_idx: usize) -> usize {
        self.hunters
            .iter()
            .filter(|h| h.lodge_idx == lodge_idx)
            .count()
    }

    pub fn status_line(&self, lodge_idx: usize) -> String {
        let mut idle = 0u32;
        let mut en_route = 0u32;
        let mut hunting = 0u32;
        let mut returning = 0u32;
        for h in self.hunters.iter().filter(|h| h.lodge_idx == lodge_idx) {
            match h.state {
                HunterState::Idle => idle += 1,
                HunterState::MovingToHunt => en_route += 1,
                HunterState::Hunting => hunting += 1,
                HunterState::Returning => returning += 1,
            }
        }
        format!("{idle} idle · {en_route} en route · {hunting} hunting · {returning} returning")
    }
}

pub fn score_hunt_tile(
    map: &Map,
    lodge: &PlacedLodge,
    coord: HexCoord,
    reserved: &HashSet<HexCoord>,
    lodges: &PlacedLodges,
) -> Option<f32> {
    if lodges.lodge_index_at(coord).is_some() {
        return None;
    }
    if !coords_in_lodge_hunt_range(lodge).contains(&coord) {
        return None;
    }
    let tile = map.tile_at(coord)?;
    if !is_huntable_terrain(tile.terrain) || !has_present_game(tile.wildlife) {
        return None;
    }
    let stock = game_stock(tile.wildlife)? as f32;
    let dist = min_dist_to_footprint(lodge, coord) as f32;
    let biome = wildlife_cap_for_terrain(tile.terrain) as f32;
    let mut score = stock * SCORE_STOCK_WEIGHT + biome * SCORE_BIOME_WEIGHT - dist * SCORE_DIST_PENALTY;
    if reserved.contains(&coord) {
        score -= SCORE_RESERVED_PENALTY;
    }
    Some(score)
}

fn min_dist_to_footprint(lodge: &PlacedLodge, coord: HexCoord) -> i32 {
    lodge_coords(lodge.anchor, lodge.rotation)
        .iter()
        .map(|foot| coord.distance(foot))
        .min()
        .unwrap_or(i32::MAX)
}

pub fn find_best_hunt_tile(
    map: &Map,
    lodge: &PlacedLodge,
    lodges: &PlacedLodges,
    reserved: &HashSet<HexCoord>,
) -> Option<HexCoord> {
    let mut best: Option<(HexCoord, f32)> = None;
    for coord in coords_in_lodge_hunt_range(lodge) {
        let Some(score) = score_hunt_tile(map, lodge, coord, reserved, lodges) else {
            continue;
        };
        if score < 0.0 {
            continue;
        }
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((coord, score));
        }
    }
    best.map(|(c, _)| c)
}

fn is_path_blocked(
    coord: HexCoord,
    hunter_idx: usize,
    hunters: &Hunters,
    lodges: &PlacedLodges,
    own_lodge_idx: usize,
    gs: &GameState,
) -> bool {
    if gs.unit_at(coord).is_some() {
        return true;
    }
    for (i, h) in hunters.hunters.iter().enumerate() {
        if i != hunter_idx && h.coord == coord {
            return true;
        }
    }
    if let Some(li) = lodges.lodge_index_at(coord) {
        return li != own_lodge_idx;
    }
    false
}

fn is_steppable(coord: HexCoord, map: &Map) -> bool {
    map.tile_at(coord)
        .is_some_and(|t| is_walkable(t.terrain))
}

pub fn hex_path(
    map: &Map,
    from: HexCoord,
    to: HexCoord,
    hunter_idx: usize,
    hunters: &Hunters,
    lodges: &PlacedLodges,
    own_lodge_idx: usize,
    gs: &GameState,
) -> Vec<HexCoord> {
    if from == to {
        return Vec::new();
    }
    let mut queue = VecDeque::from([from]);
    let mut came_from: HashMap<HexCoord, HexCoord> = HashMap::new();
    came_from.insert(from, from);

    while let Some(current) = queue.pop_front() {
        if current == to {
            break;
        }
        for next in current.neighbors() {
            if !is_steppable(next, map) {
                continue;
            }
            if next != to && is_path_blocked(next, hunter_idx, hunters, lodges, own_lodge_idx, gs) {
                continue;
            }
            if came_from.contains_key(&next) {
                continue;
            }
            came_from.insert(next, current);
            queue.push_back(next);
        }
    }

    if !came_from.contains_key(&to) {
        return Vec::new();
    }

    let mut path = Vec::new();
    let mut cur = to;
    while cur != from {
        path.push(cur);
        cur = came_from[&cur];
    }
    path.reverse();
    path
}

fn nearest_lodge_footprint(from: HexCoord, lodge: &PlacedLodge) -> HexCoord {
    lodge_coords(lodge.anchor, lodge.rotation)
        .into_iter()
        .min_by_key(|c| from.distance(c))
        .unwrap_or(lodge.anchor)
}

pub fn hunt_yield(map: &Map, coord: HexCoord) -> i32 {
    let Some(tile) = map.tile_at(coord) else {
        return 0;
    };
    let Some(stock) = game_stock(tile.wildlife) else {
        return 0;
    };
    let cap = wildlife_cap_for_terrain(tile.terrain) as i32;
    ((cap * stock as i32) / 3).max(1)
}

pub fn apply_hunt(map: &mut Map, coord: HexCoord) -> i32 {
    let yield_food = hunt_yield(map, coord);
    if yield_food <= 0 {
        return 0;
    }
    if let Some(tile) = map.tile_at_mut(coord) {
        tile.wildlife = WildlifeState::Depleted {
            turns_until_recovery: DEPLETED_RECOVERY_TURNS,
        };
    }
    yield_food
}

pub fn find_spawn_hexes(
    map: &Map,
    lodges: &PlacedLodges,
    lodge_idx: usize,
    count: usize,
) -> Vec<HexCoord> {
    let lodge = &lodges.lodges[lodge_idx];
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();

    for foot in lodge_coords(lodge.anchor, lodge.rotation) {
        if seen.insert(foot) {
            queue.push_back(foot);
        }
    }

    while let Some(current) = queue.pop_front() {
        let on_other_lodge = lodges
            .lodge_index_at(current)
            .is_some_and(|li| li != lodge_idx);
        if is_steppable(current, map) && !on_other_lodge {
            candidates.push((current, min_dist_to_footprint(lodge, current)));
        }
        for n in current.neighbors() {
            if map.tile_at(n).is_some() && seen.insert(n) {
                queue.push_back(n);
            }
        }
        if seen.len() > 300 {
            break;
        }
    }

    candidates.sort_by_key(|(_, d)| *d);
    let mut spawns = Vec::new();
    for (coord, _) in candidates {
        spawns.push(coord);
        if spawns.len() >= count {
            break;
        }
    }
    spawns
}

pub fn spawn_hunters_for_lodge(
    hunters: &mut Hunters,
    map: &Map,
    lodges: &PlacedLodges,
    lodge_idx: usize,
) {
    let spawns = find_spawn_hexes(map, lodges, lodge_idx, LODGE_HUNTER_COUNT);
    for coord in spawns {
        hunters.hunters.push(Hunter {
            coord,
            lodge_idx,
            state: HunterState::Idle,
            target: None,
            path: Vec::new(),
            carried_food: 0,
        });
    }
}

fn deposit_if_home(hunter: &mut Hunter, lodges: &mut PlacedLodges) {
    if hunter.carried_food <= 0 {
        return;
    }
    if !lodges.is_on_own_footprint(hunter.lodge_idx, hunter.coord) {
        return;
    }
    if let Some(lodge) = lodges.lodges.get_mut(hunter.lodge_idx) {
        lodge.food_stored += hunter.carried_food;
        hunter.carried_food = 0;
        hunter.state = HunterState::Idle;
        hunter.target = None;
        hunter.path.clear();
    }
}

pub fn hunters_turn_tick(
    hunters: &mut Hunters,
    lodges: &mut PlacedLodges,
    map: &mut Map,
    gs: &GameState,
) {
    let mut reserved = HashSet::new();
    for h in &hunters.hunters {
        if matches!(
            h.state,
            HunterState::MovingToHunt | HunterState::Hunting
        ) {
            if let Some(t) = h.target {
                reserved.insert(t);
            }
        }
    }

    // Harvest on tiles marked Hunting.
    for hi in 0..hunters.hunters.len() {
        if hunters.hunters[hi].state != HunterState::Hunting {
            continue;
        }
        let Some(target) = hunters.hunters[hi].target else {
            hunters.hunters[hi].state = HunterState::Idle;
            continue;
        };
        if hunters.hunters[hi].coord != target {
            continue;
        }
        let lodge_idx = hunters.hunters[hi].lodge_idx;
        let yield_food = apply_hunt(map, target);
        if yield_food > 0 {
            hunters.hunters[hi].carried_food = yield_food;
        }
        let lodge = lodges.lodges[lodge_idx].clone();
        let dest = nearest_lodge_footprint(hunters.hunters[hi].coord, &lodge);
        let path = hex_path(
            map,
            hunters.hunters[hi].coord,
            dest,
            hi,
            hunters,
            lodges,
            lodge_idx,
            gs,
        );
        hunters.hunters[hi].path = path;
        hunters.hunters[hi].target = Some(dest);
        hunters.hunters[hi].state = HunterState::Returning;
        reserved.remove(&target);
    }

    // Deposit when standing on own lodge footprint while returning.
    for hi in 0..hunters.hunters.len() {
        if hunters.hunters[hi].state == HunterState::Returning
            && lodges.is_on_own_footprint(hunters.hunters[hi].lodge_idx, hunters.hunters[hi].coord)
        {
            deposit_if_home(&mut hunters.hunters[hi], lodges);
        }
    }

    // Assign idle hunters without cargo.
    for hi in 0..hunters.hunters.len() {
        if hunters.hunters[hi].state != HunterState::Idle || hunters.hunters[hi].carried_food > 0 {
            continue;
        }
        let lodge_idx = hunters.hunters[hi].lodge_idx;
        let Some(lodge) = lodges.lodges.get(lodge_idx).cloned() else {
            continue;
        };
        let Some(target) = find_best_hunt_tile(map, &lodge, lodges, &reserved) else {
            continue;
        };
        let path = hex_path(
            map,
            hunters.hunters[hi].coord,
            target,
            hi,
            hunters,
            lodges,
            lodge_idx,
            gs,
        );
        if path.is_empty() {
            if hunters.hunters[hi].coord == target {
                hunters.hunters[hi].state = HunterState::Hunting;
            }
            continue;
        }
        reserved.insert(target);
        hunters.hunters[hi].target = Some(target);
        hunters.hunters[hi].path = path;
        hunters.hunters[hi].state = HunterState::MovingToHunt;
    }

    // If idle but carrying food, head home.
    for hi in 0..hunters.hunters.len() {
        if hunters.hunters[hi].state != HunterState::Idle || hunters.hunters[hi].carried_food <= 0 {
            continue;
        }
        let lodge_idx = hunters.hunters[hi].lodge_idx;
        let lodge = lodges.lodges[lodge_idx].clone();
        let dest = nearest_lodge_footprint(hunters.hunters[hi].coord, &lodge);
        let path = hex_path(
            map,
            hunters.hunters[hi].coord,
            dest,
            hi,
            hunters,
            lodges,
            lodge_idx,
            gs,
        );
        hunters.hunters[hi].target = Some(dest);
        hunters.hunters[hi].path = path;
        hunters.hunters[hi].state = HunterState::Returning;
    }

    // Move along paths.
    for hi in 0..hunters.hunters.len() {
        let mut moves = HUNTER_MOVES_PER_TURN;
        while moves > 0 {
            let state = hunters.hunters[hi].state;
            if !matches!(state, HunterState::MovingToHunt | HunterState::Returning) {
                break;
            }
            if hunters.hunters[hi].path.is_empty() {
                if state == HunterState::MovingToHunt {
                    hunters.hunters[hi].state = HunterState::Hunting;
                } else if lodges.is_on_own_footprint(hunters.hunters[hi].lodge_idx, hunters.hunters[hi].coord)
                {
                    deposit_if_home(&mut hunters.hunters[hi], lodges);
                }
                break;
            }
            let next = hunters.hunters[hi].path.remove(0);
            hunters.hunters[hi].coord = next;
            moves -= 1;
            if hunters.hunters[hi].path.is_empty() {
                if state == HunterState::MovingToHunt {
                    hunters.hunters[hi].state = HunterState::Hunting;
                } else if lodges.is_on_own_footprint(hunters.hunters[hi].lodge_idx, hunters.hunters[hi].coord)
                {
                    deposit_if_home(&mut hunters.hunters[hi], lodges);
                }
                break;
            }
        }
    }
}
