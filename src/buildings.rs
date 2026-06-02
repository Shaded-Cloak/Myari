use std::collections::HashSet;

use crate::hexgrid::HexCoord;
use crate::map::{is_water, Map, TerrainType};

/// Axial offsets from the anchor hex (bottom-left of the 4-hex lodge sketch).
pub const LODGE_FOOTPRINT: [(i32, i32); 4] = [(0, 0), (1, -1), (0, -1), (1, -2)];

pub fn lodge_coords(anchor: HexCoord) -> [HexCoord; 4] {
    LODGE_FOOTPRINT.map(|(dq, dr)| HexCoord::new(anchor.q + dq, anchor.r + dr))
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

pub fn can_place_lodge(map: &Map, occupied: &HashSet<HexCoord>, anchor: HexCoord) -> bool {
    for coord in lodge_coords(anchor) {
        let Some(tile) = map.tile_at(coord) else {
            return false;
        };
        if is_water(tile.terrain) || is_mountain_terrain(tile.terrain) {
            return false;
        }
        if occupied.contains(&coord) {
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
        let anchor = HexCoord::origin();
        let coords = lodge_coords(anchor);
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
}
