//! Footprint union outline, constant-distance inset, and fill triangulation.

use std::collections::{HashMap, HashSet};

use bevy::prelude::Vec2;

use crate::hexgrid::{axial_to_pixel, hex_corners_at, HexCoord};

const VERTEX_QUANT: f32 = 16.0;
const MITER_LIMIT: f32 = 4.0;

struct BorderEdge {
    a: Vec2,
    b: Vec2,
}

pub fn mesh_vertex_key(v: Vec2) -> (i32, i32) {
    ((v.x * VERTEX_QUANT).round() as i32, (v.y * VERTEX_QUANT).round() as i32)
}

fn exterior_edge_outward(tile_center: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let mid = (a + b) * 0.5;
    let edge = b - a;
    let len = edge.length();
    if len < 1e-6 {
        return Vec2::ZERO;
    }
    let tangent = edge / len;
    let normal = Vec2::new(tangent.y, -tangent.x);
    let to_out = mid - tile_center;
    if normal.dot(to_out) > 0.0 {
        normal.normalize_or_zero()
    } else {
        (-normal).normalize_or_zero()
    }
}

fn build_footprint_border_edges(coords: &[HexCoord], hex_size: f32) -> Vec<BorderEdge> {
    let set: HashSet<HexCoord> = coords.iter().copied().collect();
    let mut edges = Vec::new();
    for coord in coords {
        let (cx, cy) = axial_to_pixel(coord.q, coord.r, hex_size);
        let center = Vec2::new(cx, cy);
        let corners = hex_corners_at(coord.q, coord.r, hex_size);
        let neighbors = coord.neighbors();
        for i in 0..6 {
            if set.contains(&neighbors[i]) {
                continue;
            }
            let a = Vec2::new(corners[i].0, corners[i].1);
            let b = Vec2::new(corners[(i + 1) % 6].0, corners[(i + 1) % 6].1);
            let _ = exterior_edge_outward(center, a, b);
            edges.push(BorderEdge { a, b });
        }
    }
    edges
}

/// Ordered CCW boundary of the footprint hex union.
pub fn footprint_boundary_loop(coords: &[HexCoord], hex_size: f32) -> Vec<Vec2> {
    let edges = build_footprint_border_edges(coords, hex_size);
    if edges.is_empty() {
        return Vec::new();
    }

    let mut adj: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    let mut pos_of: HashMap<(i32, i32), Vec2> = HashMap::new();
    for e in &edges {
        let ka = mesh_vertex_key(e.a);
        let kb = mesh_vertex_key(e.b);
        pos_of.insert(ka, e.a);
        pos_of.insert(kb, e.b);
        adj.entry(ka).or_default().push(kb);
        adj.entry(kb).or_default().push(ka);
    }

    let start = mesh_vertex_key(edges[0].a);
    let mut loop_verts = vec![pos_of[&start]];
    let mut prev = start;
    let mut curr = adj[&start][0];

    for _ in 0..=edges.len() {
        if curr == start && loop_verts.len() > 2 {
            break;
        }
        loop_verts.push(pos_of[&curr]);
        let next = adj[&curr]
            .iter()
            .copied()
            .find(|&k| k != prev)
            .unwrap_or(start);
        prev = curr;
        curr = next;
    }

    ensure_ccw(&mut loop_verts);
    loop_verts
}

fn polygon_signed_area(verts: &[Vec2]) -> f32 {
    let n = verts.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    sum * 0.5
}

fn ensure_ccw(verts: &mut Vec<Vec2>) {
    if polygon_signed_area(verts) < 0.0 {
        verts.reverse();
    }
}

fn line_intersection(p0: Vec2, d0: Vec2, p1: Vec2, d1: Vec2) -> Option<Vec2> {
    let cross = d0.x * d1.y - d0.y * d1.x;
    if cross.abs() < 1e-8 {
        return None;
    }
    let diff = p1 - p0;
    let t = (diff.x * d1.y - diff.y * d1.x) / cross;
    Some(p0 + d0 * t)
}

/// Inset a CCW polygon inward by a constant world-space distance.
pub fn inset_polygon(verts: &[Vec2], margin: f32) -> Vec<Vec2> {
    let n = verts.len();
    if n < 3 || margin <= 0.0 {
        return verts.to_vec();
    }

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = verts[(i + n - 1) % n];
        let curr = verts[i];
        let next = verts[(i + 1) % n];

        let d0 = curr - prev;
        let d1 = next - curr;
        let len0 = d0.length();
        let len1 = d1.length();
        if len0 < 1e-6 || len1 < 1e-6 {
            out.push(curr);
            continue;
        }

        let e0 = d0 / len0;
        let e1 = d1 / len1;
        // CCW: interior lies to the left of each boundary edge.
        let in0 = Vec2::new(-e0.y, e0.x);
        let in1 = Vec2::new(-e1.y, e1.x);

        let p0 = curr + in0 * margin;
        let p1 = curr + in1 * margin;

        let mut corner = line_intersection(p0, e0, p1, e1).unwrap_or((p0 + p1) * 0.5);
        let miter_len = corner.distance(curr);
        if miter_len > margin * MITER_LIMIT {
            corner = (p0 + p1) * 0.5;
        }
        out.push(corner);
    }
    out
}

fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let s1 = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let s2 = (c.x - b.x) * (p.y - b.y) - (c.y - b.y) * (p.x - b.x);
    let s3 = (a.x - c.x) * (p.y - c.y) - (a.y - c.y) * (p.x - c.x);
    let has_neg = (s1 < 0.0) || (s2 < 0.0) || (s3 < 0.0);
    let has_pos = (s1 > 0.0) || (s2 > 0.0) || (s3 > 0.0);
    !(has_neg && has_pos)
}

fn is_convex_vertex(verts: &[Vec2], prev: usize, curr: usize, next: usize) -> bool {
    let a = verts[prev];
    let b = verts[curr];
    let c = verts[next];
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) >= 0.0
}

fn is_ear(verts: &[Vec2], indices: &[usize], prev: usize, curr: usize, next: usize) -> bool {
    if !is_convex_vertex(verts, prev, curr, next) {
        return false;
    }
    let a = verts[prev];
    let b = verts[curr];
    let c = verts[next];
    indices.iter().all(|&i| {
        i == prev || i == curr || i == next || !point_in_triangle(verts[i], a, b, c)
    })
}

fn triangulate_polygon(verts: &[Vec2]) -> Vec<u32> {
    let n = verts.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![0, 1, 2];
    }

    let mut indices: Vec<usize> = (0..n).collect();
    let mut tris = Vec::new();

    let mut guard = 0u32;
    while indices.len() > 3 && guard < (n * n) as u32 {
        guard += 1;
        let len = indices.len();
        let mut clipped = false;
        for i in 0..len {
            let i0 = indices[(i + len - 1) % len];
            let i1 = indices[i];
            let i2 = indices[(i + 1) % len];
            if is_ear(verts, &indices, i0, i1, i2) {
                tris.extend([i0 as u32, i1 as u32, i2 as u32]);
                indices.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }

    if indices.len() == 3 {
        tris.extend(indices.iter().map(|&i| i as u32));
    } else if !indices.is_empty() {
        let root = indices[0] as u32;
        for i in 1..indices.len() - 1 {
            tris.extend([root, indices[i] as u32, indices[i + 1] as u32]);
        }
    }
    tris
}

pub fn footprint_centroid(coords: &[HexCoord], hex_size: f32) -> Vec2 {
    let mut sum = Vec2::ZERO;
    for c in coords {
        let (x, y) = axial_to_pixel(c.q, c.r, hex_size);
        sum += Vec2::new(x, y);
    }
    sum / coords.len().max(1) as f32
}

pub fn make_footprint_fill_mesh(
    coords: &[HexCoord],
    hex_size: f32,
    origin: Vec2,
    color: [f32; 4],
    margin: f32,
) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let boundary = footprint_boundary_loop(coords, hex_size);
    if boundary.len() < 3 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let inner = inset_polygon(&boundary, margin);
    if inner.len() < 3 || polygon_signed_area(&inner) <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let tris = triangulate_polygon(&inner);
    let positions: Vec<[f32; 3]> = inner
        .iter()
        .map(|v| [v.x - origin.x, v.y - origin.y, 0.0])
        .collect();
    let colors = vec![color; positions.len()];
    (positions, colors, tris)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::lodge_coords;
    use crate::hexgrid::HexCoord;

    fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
        let mut inside = false;
        let n = poly.len();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            if ((a.y > p.y) != (b.y > p.y))
                && (p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y + 1e-8) + a.x)
            {
                inside = !inside;
            }
        }
        inside
    }

    #[test]
    fn boundary_loop_is_closed_for_all_rotations() {
        for rot in 0..6u8 {
            let coords = lodge_coords(HexCoord::origin(), rot);
            let boundary = footprint_boundary_loop(&coords, 28.0);
            assert!(boundary.len() >= 6, "rot {rot}");
            let area = polygon_signed_area(&boundary);
            assert!(area > 0.0, "rot {rot} should be CCW");
        }
    }

    #[test]
    fn inset_stays_inside_boundary() {
        let margin = 7.0;
        for rot in 0..6u8 {
            let coords = lodge_coords(HexCoord::origin(), rot);
            let boundary = footprint_boundary_loop(&coords, 28.0);
            let inner = inset_polygon(&boundary, margin);
            assert_eq!(inner.len(), boundary.len());
            for p in &inner {
                assert!(
                    point_in_polygon(*p, &boundary),
                    "inset vertex outside boundary at rot {rot}"
                );
            }
        }
    }
}
