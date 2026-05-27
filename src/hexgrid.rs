use std::ops::{Add, Sub};

use hexx::{Hex, HexLayout, HexOrientation};
use serde::{Deserialize, Serialize};

/// Flat-top hex layout used for pixel ↔ axial conversion.
fn flat_layout(hex_size: f32) -> HexLayout {
    HexLayout {
        orientation: HexOrientation::Flat,
        origin: hexx::Vec2::ZERO,
        hex_size: hexx::Vec2::new(hex_size, hex_size),
        invert_x: false,
        invert_y: true,
    }
}

/// Six corner positions for a hex at the origin (flat-top, Y-up).
pub fn hex_corners_local(size: f32) -> [(f32, f32); 6] {
    let corners = flat_layout(size).hex_corners(Hex::ZERO);
    std::array::from_fn(|i| (corners[i].x, corners[i].y))
}

/// Six corner positions for a hex at axial `(q, r)`.
pub fn hex_corners_at(q: i32, r: i32, size: f32) -> [(f32, f32); 6] {
    let corners = flat_layout(size).hex_corners(Hex::new(q, r));
    std::array::from_fn(|i| (corners[i].x, corners[i].y))
}

/// Axial hex coordinate (q, r) — backed by [`hexx::Hex`] for grid math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Serialize, Deserialize)]
pub struct HexCoord {
    pub q: i32,
    pub r: i32,
}

impl HexCoord {
    pub fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }

    pub fn origin() -> Self {
        Self::new(0, 0)
    }

    fn hex(&self) -> Hex {
        Hex::new(self.q, self.r)
    }

    pub fn distance(&self, other: &HexCoord) -> i32 {
        self.hex().unsigned_distance_to(other.hex()) as i32
    }

    pub fn neighbors(&self) -> [HexCoord; 6] {
        self.hex()
            .all_neighbors()
            .into_iter()
            .map(|h| Self::new(h.x, h.y))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
}

impl Add for HexCoord {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        let h = self.hex() + other.hex();
        Self::new(h.x, h.y)
    }
}

impl Sub for HexCoord {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        let h = self.hex() - other.hex();
        Self::new(h.x, h.y)
    }
}

/// Convert axial coords to pixel center (flat-top).
pub fn axial_to_pixel(q: i32, r: i32, size: f32) -> (f32, f32) {
    let pos = flat_layout(size).hex_to_world_pos(Hex::new(q, r));
    (pos.x, pos.y)
}

/// Convert pixel to hex.
pub fn pixel_to_hex(x: f32, y: f32, size: f32) -> HexCoord {
    let h = flat_layout(size).world_pos_to_hex(hexx::Vec2::new(x, y));
    HexCoord::new(h.x, h.y)
}

/// Axis-aligned world bounds covering every hex in `coords` (flat-top, Y-up).
pub fn hex_world_bounds(coords: impl IntoIterator<Item = HexCoord>, hex_size: f32) -> (f32, f32, f32, f32) {
    let half_w = hex_size * 0.866_025_4; // sqrt(3) / 2
    let half_h = hex_size;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for coord in coords {
        let (cx, cy) = axial_to_pixel(coord.q, coord.r, hex_size);
        min_x = min_x.min(cx - half_w);
        min_y = min_y.min(cy - half_h);
        max_x = max_x.max(cx + half_w);
        max_y = max_y.max(cy + half_h);
    }

    (min_x, min_y, max_x, max_y)
}

/// All hexes in a disk centered on the origin.
pub fn hex_disk(radius: i32) -> Vec<HexCoord> {
    Hex::ZERO
        .range(radius as u32)
        .map(|h| HexCoord::new(h.x, h.y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axial_round_trip_is_stable() {
        let cases = [
            HexCoord::new(0, 0),
            HexCoord::new(3, -2),
            HexCoord::new(-4, 5),
            HexCoord::new(7, 1),
        ];
        for c in cases {
            let (x, y) = axial_to_pixel(c.q, c.r, 28.0);
            let back = pixel_to_hex(x, y, 28.0);
            assert_eq!(back, c);
        }
    }

    #[test]
    fn disk_matches_hexx_range() {
        let disk = hex_disk(3);
        assert_eq!(disk.len(), Hex::ZERO.range(3).count());
    }
}
