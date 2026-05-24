use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Serialize, Deserialize)]
pub struct HexCoord {
    pub q: i32,
    pub r: i32,
}

impl HexCoord {
    pub fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }

    pub fn s(&self) -> i32 {
        -self.q - self.r
    }

    pub fn distance(&self, other: &HexCoord) -> i32 {
        ((self.q - other.q).abs()
            + (self.r - other.r).abs()
            + (self.s() - other.s()).abs())
            / 2
    }

    pub fn neighbors(&self) -> [HexCoord; 6] {
        let dirs = [
            (1, 0), (1, -1), (0, -1),
            (-1, 0), (-1, 1), (0, 1),
        ];
        dirs.map(|(dq, dr)| HexCoord::new(self.q + dq, self.r + dr))
    }
}

impl Add for HexCoord {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.q + other.q, self.r + other.r)
    }
}

impl Sub for HexCoord {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.q - other.q, self.r - other.r)
    }
}

/// Convert axial coords to pixel center (flat-top)
pub fn axial_to_pixel(q: i32, r: i32, size: f32) -> (f32, f32) {
    let x = size * (3.0_f32.sqrt() * q as f32 + 3.0_f32.sqrt() / 2.0 * r as f32);
    let y = size * (3.0 / 2.0 * r as f32);
    (x, y)
}

/// Convert pixel coords to axial (fractional)
pub fn pixel_to_axial_frac(x: f32, y: f32, size: f32) -> (f32, f32) {
    let q = (3.0_f32.sqrt() / 3.0 * x - 1.0 / 3.0 * y) / size;
    let r = (2.0 / 3.0 * y) / size;
    (q, r)
}

/// Round fractional axial coords to nearest hex
pub fn axial_round(q: f32, r: f32) -> HexCoord {
    let s = -q - r;
    let mut rq = q.round();
    let mut rr = r.round();
    let rs = s.round();
    let dq = (rq - q).abs();
    let dr = (rr - r).abs();
    let ds = (rs - s).abs();
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    HexCoord::new(rq as i32, rr as i32)
}

/// Convert pixel to hex
pub fn pixel_to_hex(x: f32, y: f32, size: f32) -> HexCoord {
    let (q, r) = pixel_to_axial_frac(x, y, size);
    axial_round(q, r)
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
}
