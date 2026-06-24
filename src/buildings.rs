use std::collections::{HashMap, HashSet};

use bevy::prelude::Resource;
use hexx::Hex;

use crate::hexgrid::{hex_disk, HexCoord};
use crate::map::{is_water, Map, TerrainType};

pub const HUNTING_ASSIGN_RANGE: i32 = 5;

#[derive(Debug, Clone)]
pub struct PlacedLodge {
    pub anchor: HexCoord,
    pub rotation: u8,
    pub food_stored: i32,
}

#[derive(Debug, Default, Resource)]
pub struct PlacedLodges {
    pub lodges: Vec<PlacedLodge>,
    occupied: HashMap<HexCoord, usize>,
}

impl PlacedLodges {
    pub fn register_lodge(&mut self, anchor: HexCoord, rotation: u8) {
        let idx = self.lodges.len();
        self.lodges.push(PlacedLodge {
            anchor,
            rotation,
            food_stored: 0,
        });
        for coord in lodge_coords(anchor, rotation) {
            self.occupied.insert(coord, idx);
        }
    }

    pub fn lodge_index_at(&self, coord: HexCoord) -> Option<usize> {
        self.occupied.get(&coord).copied()
    }

    pub fn footprint_hexes(&self, lodge_idx: usize) -> [HexCoord; 4] {
        let lodge = &self.lodges[lodge_idx];
        lodge_coords(lodge.anchor, lodge.rotation)
    }

    pub fn is_on_own_footprint(&self, lodge_idx: usize, coord: HexCoord) -> bool {
        self.footprint_hexes(lodge_idx).contains(&coord)
    }
}

/// All map coords within hunt range of any footprint hex of this lodge.
pub fn coords_in_lodge_hunt_range(lodge: &PlacedLodge) -> HashSet<HexCoord> {
    let mut out = HashSet::new();
    for foot in lodge_coords(lodge.anchor, lodge.rotation) {
        for offset in hex_disk(HUNTING_ASSIGN_RANGE) {
            out.insert(HexCoord::new(foot.q + offset.q, foot.r + offset.r));
        }
    }
    out
}

/// Axial offsets from the anchor hex (bottom-left of the 4-hex sketch; hover hex while placing).
pub const LODGE_FOOTPRINT: [(i32, i32); 4] = [(0, 0), (1, -1), (0, -1), (1, -2)];

pub fn rotate_footprint_offset(dq: i32, dr: i32, steps: u8) -> (i32, i32) {
    let h = Hex::new(dq, dr).rotate_cw(steps as u32);
    (h.x, h.y)
}

pub fn lodge_coords(anchor: HexCoord, rotation: u8) -> [HexCoord; 4] {
    LODGE_FOOTPRINT.map(|(dq, dr)| {
        let (rq, rr) = rotate_footprint_offset(dq, dr, rotation);
        HexCoord::new(anchor.q + rq, anchor.r + rr)
    })
}

pub fn is_mountain_terrain(t: TerrainType) -> bool {
    matches!(
        t,
        TerrainType::Hills
            | TerrainType::Mountain
            | TerrainType::SnowPeak
            | TerrainType::StonySlope
            | TerrainType::AridPeak
            | TerrainType::GlacialPeak
    )
}

pub fn can_place_lodge(map: &Map, placed: &PlacedLodges, anchor: HexCoord, rotation: u8) -> bool {
    for coord in lodge_coords(anchor, rotation) {
        let Some(tile) = map.tile_at(coord) else {
            return false;
        };
        if is_water(tile.terrain) || is_mountain_terrain(tile.terrain) {
            return false;
        }
        if placed.occupied.contains_key(&coord) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lodge_footprint_is_connected() {
        let coords = lodge_coords(HexCoord::origin(), 0);
        for i in 0..4 {
            let dists: Vec<i32> = (0..4)
                .filter(|&j| j != i)
                .map(|j| coords[i].distance(&coords[j]))
                .collect();
            assert!(
                dists.iter().any(|&d| d == 1),
                "hex {:?} should touch another footprint hex",
                coords[i]
            );
        }
    }

    #[test]
    fn rotation_preserves_footprint_size() {
        for rot in 0..6u8 {
            assert_eq!(lodge_coords(HexCoord::new(3, -2), rot).len(), 4);
        }
    }
}
