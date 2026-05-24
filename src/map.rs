use std::collections::HashMap;

use crate::hexgrid::HexCoord;

pub const MAP_RADIUS: i32 = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainType {
    DeepOcean,
    Ocean,
    Coast,
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
    pub fn generate(radius: i32, _seed: u64) -> Self {
        let coords = hex_disk_coords(radius);
        let mut tiles = Vec::with_capacity(coords.len());
        let mut by_coord = HashMap::with_capacity(coords.len());

        for coord in coords {
            let idx = tiles.len();
            tiles.push(HexTile {
                coord,
                terrain: TerrainType::Ocean,
            });
            by_coord.insert(coord, idx);
        }

        Self { tiles, by_coord }
    }

    pub fn tile_at(&self, coord: HexCoord) -> Option<&HexTile> {
        self.by_coord.get(&coord).and_then(|&idx| self.tiles.get(idx))
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
