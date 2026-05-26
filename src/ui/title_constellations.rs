//! Drifting ✦ stars for the title screen backdrop, linked by gold connector lines.

use bevy::hierarchy::{ChildBuilder, Parent};
use bevy::prelude::*;
use rand::Rng;
use std::collections::{HashMap, HashSet};

use crate::ui::{UiTheme, GOLD, GOLD_DIM, STAR};

const TARGET_STAR_COUNT: usize = 12;
const MIN_STAR_SIZE: f32 = 15.0;
const MAX_STAR_SIZE: f32 = 48.0;
const MIN_RESPAWN_DELAY: f32 = 0.0;
const MAX_RESPAWN_DELAY: f32 = 10.0;
const LINE_HEIGHT: f32 = 1.5;
const PULSE_WIDTH: f32 = 10.0;
const PULSE_SPEED: f32 = 28.0;
const SPRING_STRENGTH: f32 = 2.8;
const REPULSION_STRENGTH: f32 = 4.0;
const OVERLAP_REPULSION_STRENGTH: f32 = 14.0;
const OVERLAP_GAP_PX: f32 = 8.0;
const OVERLAP_RESOLVE_ITERATIONS: usize = 3;
const HOME_ANCHOR_STRENGTH: f32 = 0.35;
const MIN_STAR_SEPARATION_PCT: f32 = 11.0;
const SPARSE_ENTRY_CANDIDATES: usize = 12;
const QUADRANT_BONUS: f32 = 40.0;
const THIRD_LINK_RATIO: f32 = 1.4;
const CENTER_X_MIN: f32 = 28.0;
const CENTER_X_MAX: f32 = 72.0;
const CENTER_Y_MIN: f32 = 22.0;
const CENTER_Y_MAX: f32 = 78.0;
const MIN_CONSTELLATIONS: u8 = 2;
const MAX_CONSTELLATIONS: u8 = 4;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConstellationGroup(pub u8);

#[derive(Component)]
pub struct TitleConstellationLayer;

#[derive(Component)]
pub struct TitleConstellationLinksRoot;

#[derive(Resource)]
pub struct TitleConstellationState {
    pub pending_respawns: Vec<f32>,
    pub target_count: usize,
}

impl Default for TitleConstellationState {
    fn default() -> Self {
        Self {
            pending_respawns: Vec::new(),
            target_count: TARGET_STAR_COUNT,
        }
    }
}

#[derive(Component)]
pub(crate) struct FloatingStar {
    pos: Vec2,
    start_pct: Vec2,
    size: f32,
    initialized: bool,
    seen_on_screen: bool,
    velocity: Vec2,
    spin: f32,
    angle: f32,
}

#[derive(Component)]
pub(crate) struct StarGlyph;

#[derive(Component)]
pub(crate) struct StarLinkLine {
    end_a: Entity,
    end_b: Entity,
    length: f32,
    rest_length: f32,
}

#[derive(Component)]
pub(crate) struct StarLinkPulse {
    phase: f32,
}

struct StarSpawn {
    left_pct: f32,
    top_pct: f32,
    size: f32,
    velocity: Vec2,
    spin: f32,
    group: u8,
}

pub fn spawn_title_constellations(parent: &mut ChildBuilder, theme: &UiTheme) {
    let mut rng = rand::thread_rng();
    let placements = generate_poisson_stars(TARGET_STAR_COUNT, &mut rng);
    let groups = cluster_stars(&placements, &mut rng);
    let mut specs = Vec::with_capacity(placements.len());
    for (pct, group) in placements.into_iter().zip(groups) {
        let (size, velocity, spin) = random_star_motion(&mut rng);
        specs.push(StarSpawn {
            left_pct: pct.x,
            top_pct: pct.y,
            size,
            velocity,
            spin,
            group,
        });
    }

    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            TitleConstellationLayer,
        ))
        .with_children(|layer| {
            let mut star_entities = Vec::with_capacity(specs.len());
            let mut star_centers = Vec::with_capacity(specs.len());
            let mut star_groups = Vec::with_capacity(specs.len());

            for spec in &specs {
                let entity = spawn_floating_star(layer, theme, spec);
                star_entities.push(entity);
                star_centers.push(Vec2::new(spec.left_pct, spec.top_pct));
                star_groups.push(spec.group);
            }

            let edges = build_knn_edges(&star_centers, &star_entities, &star_groups);
            layer
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::ZERO,
                        top: Val::ZERO,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    TitleConstellationLinksRoot,
                    ZIndex(0),
                ))
                .with_children(|links| {
                    for (a, b) in edges {
                        spawn_link_line(links, a, b, rng.gen_range(0.0..200.0));
                    }
                });
        });
}

fn spawn_floating_star(parent: &mut ChildBuilder, theme: &UiTheme, spec: &StarSpawn) -> Entity {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(spec.left_pct),
                top: Val::Percent(spec.top_pct),
                width: Val::Px(spec.size),
                height: Val::Px(spec.size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ZIndex(1),
            ConstellationGroup(spec.group),
            FloatingStar {
                pos: Vec2::ZERO,
                start_pct: Vec2::new(spec.left_pct, spec.top_pct),
                size: spec.size,
                initialized: false,
                seen_on_screen: false,
                velocity: spec.velocity,
                spin: spec.spin,
                angle: 0.0,
            },
        ))
        .with_children(|star| {
            star.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                StarGlyph,
                Transform::default(),
                Text::new(STAR),
                theme.symbol_font(spec.size),
                TextColor(GOLD),
            ));
        })
        .id()
}

fn spawn_link_line(parent: &mut ChildBuilder, end_a: Entity, end_b: Entity, phase: f32) {
    parent
        .spawn(link_line_bundle(end_a, end_b, phase))
        .with_children(|line| {
            line.spawn((pulse_bundle(phase), BackgroundColor(GOLD)));
        });
}

fn link_line_bundle(end_a: Entity, end_b: Entity, _phase: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(0.0),
            height: Val::Px(LINE_HEIGHT),
            overflow: Overflow::clip(),
            ..default()
        },
        StarLinkLine {
            end_a,
            end_b,
            length: 0.0,
            rest_length: 0.0,
        },
        Transform::default(),
        BackgroundColor(GOLD_DIM.with_alpha(0.4)),
    )
}

fn pulse_bundle(phase: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(PULSE_WIDTH),
            height: Val::Px(LINE_HEIGHT),
            ..default()
        },
        StarLinkPulse { phase },
    )
}

fn is_in_dead_zone(p: Vec2) -> bool {
    p.x >= CENTER_X_MIN && p.x <= CENTER_X_MAX && p.y >= CENTER_Y_MIN && p.y <= CENTER_Y_MAX
}

fn generate_poisson_stars(count: usize, rng: &mut impl Rng) -> Vec<Vec2> {
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let mut placed = None;
        for _ in 0..200 {
            let candidate = Vec2::new(rng.gen_range(4.0..96.0), rng.gen_range(4.0..96.0));
            if is_in_dead_zone(candidate) {
                continue;
            }
            if points
                .iter()
                .all(|p: &Vec2| p.distance(candidate) >= MIN_STAR_SEPARATION_PCT)
            {
                placed = Some(candidate);
                break;
            }
        }
        if let Some(p) = placed {
            points.push(p);
        }
    }

    if points.len() < count {
        points.extend(fallback_jitter_grid(count - points.len(), &points, rng));
    }

    points
}

fn fallback_jitter_grid(need: usize, existing: &[Vec2], rng: &mut impl Rng) -> Vec<Vec2> {
    const SLOTS: &[(f32, f32)] = &[
        (10.0, 12.0),
        (22.0, 8.0),
        (8.0, 35.0),
        (18.0, 55.0),
        (10.0, 78.0),
        (88.0, 15.0),
        (78.0, 10.0),
        (92.0, 40.0),
        (85.0, 65.0),
        (90.0, 85.0),
        (35.0, 10.0),
        (65.0, 88.0),
    ];

    let mut out = Vec::with_capacity(need);
    for &(x, y) in SLOTS {
        if out.len() >= need {
            break;
        }
        let candidate = Vec2::new(
            (x + rng.gen_range(-4.0..4.0)).clamp(4.0, 96.0),
            (y + rng.gen_range(-4.0..4.0)).clamp(4.0, 96.0),
        );
        if is_in_dead_zone(candidate) {
            continue;
        }
        if existing
            .iter()
            .chain(out.iter())
            .all(|p| p.distance(candidate) >= MIN_STAR_SEPARATION_PCT * 0.75)
        {
            out.push(candidate);
        }
    }
    out
}

/// Partition stars into 2–4 spatial clusters (k-means++ seeds).
fn cluster_stars(centers: &[Vec2], rng: &mut impl Rng) -> Vec<u8> {
    let n = centers.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }

    let k = rng
        .gen_range(MIN_CONSTELLATIONS..=MAX_CONSTELLATIONS)
        .min(n as u8) as usize;
    let mut seed_indices = vec![rng.gen_range(0..n)];
    while seed_indices.len() < k {
        let next = (0..n)
            .max_by(|&a, &b| {
                let da = seed_indices
                    .iter()
                    .map(|&s| centers[a].distance(centers[s]))
                    .fold(f32::INFINITY, f32::min);
                let db = seed_indices
                    .iter()
                    .map(|&s| centers[b].distance(centers[s]))
                    .fold(f32::INFINITY, f32::min);
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();
        seed_indices.push(next);
    }

    centers
        .iter()
        .map(|center| {
            seed_indices
                .iter()
                .enumerate()
                .min_by(|(_, &a), (_, &b)| {
                    center
                        .distance(centers[a])
                        .partial_cmp(&center.distance(centers[b]))
                        .unwrap()
                })
                .unwrap()
                .0 as u8
        })
        .collect()
}

fn build_knn_edges(
    centers: &[Vec2],
    entities: &[Entity],
    groups: &[u8],
) -> Vec<(Entity, Entity)> {
    let count = centers.len();
    if count <= 1 {
        return Vec::new();
    }

    let mut edges = Vec::new();
    for i in 0..count {
        let mut neighbors: Vec<(usize, f32)> = (0..count)
            .filter(|&j| j != i && groups[j] == groups[i])
            .map(|j| (j, centers[i].distance(centers[j])))
            .collect();
        neighbors.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        if neighbors.len() >= 1 {
            push_edge(&mut edges, entities[i], entities[neighbors[0].0]);
        }
        if neighbors.len() >= 2 {
            push_edge(&mut edges, entities[i], entities[neighbors[1].0]);
            if neighbors.len() >= 3 {
                let d2 = neighbors[1].1;
                let d3 = neighbors[2].1;
                if d3 <= d2 * THIRD_LINK_RATIO {
                    push_edge(&mut edges, entities[i], entities[neighbors[2].0]);
                }
            }
        }
    }
    edges
}

fn links_for_new_star(
    new_entity: Entity,
    new_center: Vec2,
    group: u8,
    existing: &[(Entity, Vec2, u8)],
) -> Vec<(Entity, Entity)> {
    let same_group: Vec<(Entity, f32)> = existing
        .iter()
        .filter(|(_, _, g)| *g == group)
        .map(|(entity, center, _)| (*entity, center.distance(new_center)))
        .collect();

    if same_group.is_empty() {
        return Vec::new();
    }

    let mut neighbors = same_group;
    neighbors.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let mut edges = Vec::new();
    if neighbors.len() >= 1 {
        edges.push(normalize_entity_pair(new_entity, neighbors[0].0));
    }
    if neighbors.len() >= 2 {
        edges.push(normalize_entity_pair(new_entity, neighbors[1].0));
        if neighbors.len() >= 3 {
            let d2 = neighbors[1].1;
            let d3 = neighbors[2].1;
            if d3 <= d2 * THIRD_LINK_RATIO {
                edges.push(normalize_entity_pair(new_entity, neighbors[2].0));
            }
        }
    }
    edges
}

fn group_for_respawn(existing: &[(Entity, Vec2, u8)], new_center: Vec2, rng: &mut impl Rng) -> u8 {
    if existing.is_empty() {
        return rng.gen_range(0..MAX_CONSTELLATIONS);
    }
    existing
        .iter()
        .min_by(|(_, a, _), (_, b, _)| {
            a.distance(new_center)
                .partial_cmp(&b.distance(new_center))
                .unwrap()
        })
        .map(|(_, _, g)| *g)
        .unwrap()
}

fn push_edge(edges: &mut Vec<(Entity, Entity)>, a: Entity, b: Entity) {
    let pair = normalize_entity_pair(a, b);
    if !edges.contains(&pair) {
        edges.push(pair);
    }
}

fn normalize_entity_pair(a: Entity, b: Entity) -> (Entity, Entity) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn star_center(star: &FloatingStar) -> Vec2 {
    star.pos + Vec2::splat(star.size * 0.5)
}

fn star_fully_offscreen(pos: Vec2, size: f32, width: f32, height: f32) -> bool {
    pos.x + size < 0.0 || pos.x > width || pos.y + size < 0.0 || pos.y > height
}

fn min_center_separation(size_a: f32, size_b: f32) -> f32 {
    (size_a + size_b) * 0.5 + OVERLAP_GAP_PX
}

fn accumulate_overlap_forces(
    centers: &HashMap<Entity, Vec2>,
    sizes: &HashMap<Entity, f32>,
) -> HashMap<Entity, Vec2> {
    let mut impulses = HashMap::new();
    let entities: Vec<Entity> = centers.keys().copied().collect();
    for i in 0..entities.len() {
        for j in (i + 1)..entities.len() {
            let a = entities[i];
            let b = entities[j];
            let Some(&ca) = centers.get(&a) else {
                continue;
            };
            let Some(&cb) = centers.get(&b) else {
                continue;
            };
            let Some(&size_a) = sizes.get(&a) else {
                continue;
            };
            let Some(&size_b) = sizes.get(&b) else {
                continue;
            };
            let min_dist = min_center_separation(size_a, size_b);
            let delta = cb - ca;
            let dist = delta.length();
            if dist >= min_dist {
                continue;
            }
            let dir = if dist < 1.0 {
                Vec2::new(1.0, 0.0)
            } else {
                delta / dist
            };
            let penetration = min_dist - dist;
            let strength = penetration / min_dist * OVERLAP_REPULSION_STRENGTH;
            apply_impulses(&mut impulses, a, -dir * strength);
            apply_impulses(&mut impulses, b, dir * strength);
        }
    }
    impulses
}

/// Hard positional separation when overlap persists after force integration.
fn resolve_star_overlaps(
    entities: &[Entity],
    centers: &mut HashMap<Entity, Vec2>,
    sizes: &HashMap<Entity, f32>,
) {
    for _ in 0..OVERLAP_RESOLVE_ITERATIONS {
        for i in 0..entities.len() {
            for j in (i + 1)..entities.len() {
                let a = entities[i];
                let b = entities[j];
                let Some(&size_a) = sizes.get(&a) else {
                    continue;
                };
                let Some(&size_b) = sizes.get(&b) else {
                    continue;
                };
                let min_dist = min_center_separation(size_a, size_b);
                let ca = *centers.get(&a).unwrap_or(&Vec2::ZERO);
                let cb = *centers.get(&b).unwrap_or(&Vec2::ZERO);
                let delta = cb - ca;
                let dist = delta.length();
                if dist >= min_dist {
                    continue;
                }
                let dir = if dist < 1.0 {
                    Vec2::new(1.0, 0.0)
                } else {
                    delta / dist
                };
                let push = (min_dist - dist) * 0.5 + 1.0;
                centers.insert(a, ca - dir * push);
                centers.insert(b, cb + dir * push);
            }
        }
    }
}
fn min_runtime_sep_px(w: f32, h: f32) -> f32 {
    MIN_STAR_SEPARATION_PCT / 100.0 * w.min(h)
}

fn collect_linked_pairs(links: &Query<(Entity, &StarLinkLine)>) -> HashSet<(Entity, Entity)> {
    let mut set = HashSet::new();
    for (_, link) in links.iter() {
        set.insert(normalize_entity_pair(link.end_a, link.end_b));
    }
    set
}

fn apply_impulses(impulses: &mut HashMap<Entity, Vec2>, entity: Entity, force: Vec2) {
    if force.length_squared() > 0.0 {
        *impulses.entry(entity).or_default() += force;
    }
}

/// Bidirectional Hooke springs along linked segments.
fn accumulate_spring_forces(
    centers: &HashMap<Entity, Vec2>,
    links: &[(Entity, Entity, f32)],
) -> HashMap<Entity, Vec2> {
    let mut impulses = HashMap::new();
    for &(end_a, end_b, rest) in links {
        if rest <= 0.0 {
            continue;
        }
        let Some(&a) = centers.get(&end_a) else {
            continue;
        };
        let Some(&b) = centers.get(&end_b) else {
            continue;
        };
        let delta = b - a;
        let len = delta.length();
        if len < 1.0 {
            continue;
        }
        let stretch = len - rest;
        let dir = delta / len;
        let force = dir * (stretch * SPRING_STRENGTH);
        apply_impulses(&mut impulses, end_a, force);
        apply_impulses(&mut impulses, end_b, -force);
    }
    impulses
}

/// Push non-linked stars apart when closer than the runtime separation threshold.
fn accumulate_separation_forces(
    centers: &HashMap<Entity, Vec2>,
    linked: &HashSet<(Entity, Entity)>,
    min_sep: f32,
) -> HashMap<Entity, Vec2> {
    let mut impulses = HashMap::new();
    let entities: Vec<Entity> = centers.keys().copied().collect();
    for i in 0..entities.len() {
        for j in (i + 1)..entities.len() {
            let a = entities[i];
            let b = entities[j];
            if linked.contains(&normalize_entity_pair(a, b)) {
                continue;
            }
            let Some(&ca) = centers.get(&a) else {
                continue;
            };
            let Some(&cb) = centers.get(&b) else {
                continue;
            };
            let delta = cb - ca;
            let dist = delta.length();
            if dist < 1.0 || dist >= min_sep {
                continue;
            }
            let dir = delta / dist;
            let strength = (min_sep - dist) / min_sep * REPULSION_STRENGTH;
            apply_impulses(&mut impulses, a, -dir * strength);
            apply_impulses(&mut impulses, b, dir * strength);
        }
    }
    impulses
}

/// Gentle pull back toward each star's home position (percent space).
fn accumulate_home_forces(
    centers: &HashMap<Entity, Vec2>,
    homes_pct: &HashMap<Entity, Vec2>,
    w: f32,
    h: f32,
) -> HashMap<Entity, Vec2> {
    let mut impulses = HashMap::new();
    for (&entity, &center) in centers {
        let Some(home_pct) = homes_pct.get(&entity) else {
            continue;
        };
        if home_pct.length_squared() <= 0.0 {
            continue;
        }
        let home_px = Vec2::new(home_pct.x / 100.0 * w, home_pct.y / 100.0 * h);
        let force = (home_px - center) * HOME_ANCHOR_STRENGTH;
        apply_impulses(&mut impulses, entity, force);
    }
    impulses
}

fn merge_impulses(into: &mut HashMap<Entity, Vec2>, from: HashMap<Entity, Vec2>) {
    for (entity, force) in from {
        *into.entry(entity).or_default() += force;
    }
}

fn accumulate_star_forces(
    centers: &HashMap<Entity, Vec2>,
    sizes: &HashMap<Entity, f32>,
    homes_pct: &HashMap<Entity, Vec2>,
    spring_links: &[(Entity, Entity, f32)],
    linked: &HashSet<(Entity, Entity)>,
    w: f32,
    h: f32,
) -> HashMap<Entity, Vec2> {
    let min_sep = min_runtime_sep_px(w, h);
    let mut impulses = accumulate_spring_forces(centers, spring_links);
    merge_impulses(&mut impulses, accumulate_overlap_forces(centers, sizes));
    merge_impulses(&mut impulses, accumulate_separation_forces(centers, linked, min_sep));
    merge_impulses(&mut impulses, accumulate_home_forces(centers, homes_pct, w, h));
    impulses
}

fn quadrant_index(center: Vec2, w: f32, h: f32) -> usize {
    let qx = (center.x >= w * 0.5) as usize;
    let qy = (center.y >= h * 0.5) as usize;
    qx + qy * 2
}

fn quadrant_counts(centers: &[Vec2], w: f32, h: f32) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for center in centers {
        counts[quadrant_index(*center, w, h)] += 1;
    }
    counts
}

fn entry_quadrant(pos: Vec2, size: f32, w: f32, h: f32) -> usize {
    quadrant_index(pos + Vec2::splat(size * 0.5), w, h)
}

fn pos_to_pct(pos: Vec2, size: f32, w: f32, h: f32) -> Vec2 {
    let center = pos + Vec2::splat(size * 0.5);
    Vec2::new(center.x / w * 100.0, center.y / h * 100.0)
}

fn sparse_entry_spawn(
    width: f32,
    height: f32,
    size: f32,
    existing_centers: &[Vec2],
    rng: &mut impl Rng,
) -> (Vec2, Vec2) {
    if existing_centers.is_empty() {
        return random_entry_spawn(width, height, size, rng);
    }

    let quadrant_counts = quadrant_counts(existing_centers, width, height);
    let min_quad_count = quadrant_counts.iter().min().copied().unwrap_or(0);

    let mut best_score = f32::NEG_INFINITY;
    let mut best = random_entry_spawn(width, height, size, rng);

    for _ in 0..SPARSE_ENTRY_CANDIDATES {
        let candidate = random_entry_spawn(width, height, size, rng);
        let center = candidate.0 + Vec2::splat(size * 0.5);
        let min_dist = existing_centers
            .iter()
            .map(|c| c.distance(center))
            .fold(f32::INFINITY, f32::min);
        if min_dist < min_center_separation(size, size) {
            continue;
        }
        let q = entry_quadrant(candidate.0, size, width, height);
        let quad_bonus = if quadrant_counts[q] <= min_quad_count {
            QUADRANT_BONUS
        } else {
            0.0
        };
        let score = min_dist + quad_bonus;
        if score > best_score {
            best_score = score;
            best = candidate;
        }
    }

    best
}

fn random_star_motion(rng: &mut impl Rng) -> (f32, Vec2, f32) {
    let size = rng.gen_range(MIN_STAR_SIZE..=MAX_STAR_SIZE);
    let speed = rng.gen_range(1.0..=20.0);
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let velocity = Vec2::new(angle.cos() * speed, angle.sin() * speed);
    let spin = rng.gen_range(-0.18..=0.18);
    (size, velocity, spin)
}

fn random_entry_spawn(width: f32, height: f32, size: f32, rng: &mut impl Rng) -> (Vec2, Vec2) {
    let edge = rng.gen_range(0..4);
    let margin = size + 8.0;
    match edge {
        0 => {
            let y = rng.gen_range(0.0..(height - size).max(1.0));
            (
                Vec2::new(-margin, y),
                Vec2::new(rng.gen_range(4.0..=18.0), rng.gen_range(-6.0..=6.0)),
            )
        }
        1 => {
            let y = rng.gen_range(0.0..(height - size).max(1.0));
            (
                Vec2::new(width + margin - size, y),
                Vec2::new(-rng.gen_range(4.0..=18.0), rng.gen_range(-6.0..=6.0)),
            )
        }
        2 => {
            let x = rng.gen_range(0.0..(width - size).max(1.0));
            (
                Vec2::new(x, -margin),
                Vec2::new(rng.gen_range(-6.0..=6.0), rng.gen_range(4.0..=18.0)),
            )
        }
        _ => {
            let x = rng.gen_range(0.0..(width - size).max(1.0));
            (
                Vec2::new(x, height + margin - size),
                Vec2::new(rng.gen_range(-6.0..=6.0), -rng.gen_range(4.0..=18.0)),
            )
        }
    }
}

fn spawn_star_at(
    commands: &mut Commands,
    layer: Entity,
    links_root: Entity,
    theme: &UiTheme,
    pos: Vec2,
    size: f32,
    velocity: Vec2,
    spin: f32,
    group: u8,
    existing: &[(Entity, Vec2, u8)],
    rng: &mut impl Rng,
) -> Entity {
    let star = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pos.x),
                top: Val::Px(pos.y),
                width: Val::Px(size),
                height: Val::Px(size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ZIndex(1),
            ConstellationGroup(group),
            FloatingStar {
                pos,
                start_pct: Vec2::ZERO,
                size,
                initialized: true,
                seen_on_screen: false,
                velocity,
                spin,
                angle: rng.gen_range(0.0..std::f32::consts::TAU),
            },
        ))
        .with_children(|star_node| {
            star_node.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                StarGlyph,
                Transform::default(),
                Text::new(STAR),
                theme.symbol_font(size),
                TextColor(GOLD),
            ));
        })
        .id();

    commands.entity(layer).add_child(star);

    let center = pos + Vec2::splat(size * 0.5);
    for (a, b) in links_for_new_star(star, center, group, existing) {
        spawn_link_via_commands(commands, links_root, a, b, rng.gen_range(0.0..200.0));
    }

    star
}

fn spawn_link_via_commands(
    commands: &mut Commands,
    links_root: Entity,
    end_a: Entity,
    end_b: Entity,
    phase: f32,
) {
    let line = commands
        .spawn(link_line_bundle(end_a, end_b, phase))
        .with_children(|line| {
            line.spawn((
                pulse_bundle(phase),
                BackgroundColor(GOLD),
            ));
        })
        .id();
    commands.entity(links_root).add_child(line);
}

fn remove_links_for_star(commands: &mut Commands, links: &Query<(Entity, &StarLinkLine)>, star: Entity) {
    for (line_entity, link) in links.iter() {
        if link.end_a == star || link.end_b == star {
            commands.entity(line_entity).despawn_recursive();
        }
    }
}

pub fn animate_title_constellations(
    time: Res<Time>,
    mut commands: Commands,
    mut state: ResMut<TitleConstellationState>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut stars: Query<(Entity, &mut FloatingStar, &mut Node)>,
    links: Query<(Entity, &StarLinkLine)>,
) {
    let dt = time.delta_secs();
    let Ok(window) = window.get_single() else {
        return;
    };
    let w = window.width();
    let h = window.height();

    let mut centers = HashMap::new();
    let mut sizes = HashMap::new();
    let mut homes_pct = HashMap::new();
    let mut entities = Vec::new();
    for (entity, mut star, _) in &mut stars {
        if !star.initialized {
            star.pos = Vec2::new(star.start_pct.x / 100.0 * w, star.start_pct.y / 100.0 * h);
            star.initialized = true;
        }
        entities.push(entity);
        sizes.insert(entity, star.size);
        centers.insert(entity, star_center(&star));
        homes_pct.insert(entity, star.start_pct);
    }

    let linked = collect_linked_pairs(&links);
    let spring_links: Vec<(Entity, Entity, f32)> = links
        .iter()
        .map(|(_, link)| (link.end_a, link.end_b, link.rest_length))
        .collect();
    let forces = accumulate_star_forces(&centers, &sizes, &homes_pct, &spring_links, &linked, w, h);

    for (entity, mut star, _) in &mut stars {
        if let Some(force) = forces.get(&entity) {
            star.velocity += *force * dt;
        }
        let velocity = star.velocity;
        star.pos += velocity * dt;
        if let Some(center) = centers.get_mut(&entity) {
            *center = star_center(&star);
        }
    }

    resolve_star_overlaps(&entities, &mut centers, &sizes);

    let mut to_despawn = Vec::new();
    for (entity, mut star, mut node) in &mut stars {
        if let Some(center) = centers.get(&entity) {
            star.pos = *center - Vec2::splat(star.size * 0.5);
        }

        let offscreen = star_fully_offscreen(star.pos, star.size, w, h);
        if !offscreen {
            if !star.seen_on_screen {
                star.start_pct = pos_to_pct(star.pos, star.size, w, h);
            }
            star.seen_on_screen = true;
        }
        if star.seen_on_screen && offscreen {
            to_despawn.push(entity);
            continue;
        }

        node.left = Val::Px(star.pos.x);
        node.top = Val::Px(star.pos.y);
        star.angle += star.spin * dt;
    }

    let mut rng = rand::thread_rng();
    for entity in to_despawn {
        remove_links_for_star(&mut commands, &links, entity);
        commands.entity(entity).despawn_recursive();
        state.pending_respawns.push(rng.gen_range(MIN_RESPAWN_DELAY..=MAX_RESPAWN_DELAY));
    }
}

pub fn tick_star_respawns(
    time: Res<Time>,
    mut commands: Commands,
    mut state: ResMut<TitleConstellationState>,
    theme: Res<UiTheme>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    layer: Query<Entity, With<TitleConstellationLayer>>,
    links_root: Query<Entity, With<TitleConstellationLinksRoot>>,
    stars: Query<(Entity, &FloatingStar, &ConstellationGroup)>,
) {
    let dt = time.delta_secs();
    let Ok(layer) = layer.get_single() else {
        return;
    };
    let Ok(links_root) = links_root.get_single() else {
        return;
    };
    let Ok(window) = window.get_single() else {
        return;
    };
    let w = window.width();
    let h = window.height();

    let mut spawn_now = 0usize;
    let mut i = 0;
    while i < state.pending_respawns.len() {
        state.pending_respawns[i] -= dt;
        if state.pending_respawns[i] <= 0.0 {
            state.pending_respawns.swap_remove(i);
            spawn_now += 1;
        } else {
            i += 1;
        }
    }

    if spawn_now == 0 {
        let active = stars.iter().count();
        let queued = state.pending_respawns.len();
        let deficit = state.target_count.saturating_sub(active + queued);
        if deficit > 0 {
            let mut rng = rand::thread_rng();
            for _ in 0..deficit {
                state
                    .pending_respawns
                    .push(rng.gen_range(MIN_RESPAWN_DELAY..=MAX_RESPAWN_DELAY));
            }
        }
        return;
    }

    let mut rng = rand::thread_rng();
    let mut existing_centers: Vec<Vec2> = stars
        .iter()
        .map(|(_, star, _)| star_center(star))
        .collect();
    let mut existing: Vec<(Entity, Vec2, u8)> = stars
        .iter()
        .map(|(entity, star, group)| (entity, star_center(star), group.0))
        .collect();

    for _ in 0..spawn_now {
        let (size, _, spin) = random_star_motion(&mut rng);
        let (pos, velocity) = sparse_entry_spawn(w, h, size, &existing_centers, &mut rng);
        let center = pos + Vec2::splat(size * 0.5);
        let group = group_for_respawn(&existing, center, &mut rng);
        let star = spawn_star_at(
            &mut commands,
            layer,
            links_root,
            &theme,
            pos,
            size,
            velocity,
            spin,
            group,
            &existing,
            &mut rng,
        );
        existing.push((star, center, group));
        existing_centers.push(center);
    }
}

pub fn update_star_link_lines(
    stars: Query<&FloatingStar>,
    mut lines: Query<(&mut StarLinkLine, &mut Node)>,
) {
    for (mut link, mut node) in &mut lines {
        let end_a = link.end_a;
        let end_b = link.end_b;
        let Ok(star_a) = stars.get(end_a) else {
            continue;
        };
        let Ok(star_b) = stars.get(end_b) else {
            continue;
        };

        let from = star_center(star_a);
        let to = star_center(star_b);
        let delta = to - from;
        let length = delta.length();
        if length < 1.0 {
            link.length = 0.0;
            node.width = Val::Px(0.0);
            continue;
        }

        link.length = length;
        if link.rest_length <= 0.0 {
            link.rest_length = length.max(min_center_separation(star_a.size, star_b.size));
        }
        let midpoint = from + delta * 0.5;
        node.left = Val::Px(midpoint.x - length * 0.5);
        node.top = Val::Px(midpoint.y - LINE_HEIGHT * 0.5);
        node.width = Val::Px(length);
        node.height = Val::Px(LINE_HEIGHT);
    }
}

pub fn animate_star_link_pulses(
    time: Res<Time>,
    lines: Query<&StarLinkLine>,
    mut pulses: Query<(&Parent, &StarLinkPulse, &mut Node), With<StarLinkPulse>>,
) {
    let t = time.elapsed_secs();
    for (parent, pulse, mut node) in &mut pulses {
        let parent_entity = parent.get();
        let Ok(line) = lines.get(parent_entity) else {
            continue;
        };
        let length = line.length;
        if length < 1.0 {
            continue;
        }

        let pulse_w = PULSE_WIDTH.min(length);
        let travel = (t * PULSE_SPEED + pulse.phase).rem_euclid(length);
        node.left = Val::Px(travel.min((length - pulse_w).max(0.0)));
        node.width = Val::Px(pulse_w);
        node.height = Val::Px(LINE_HEIGHT);
    }
}

pub fn rotate_title_constellations(
    stars: Query<(&FloatingStar, &Children)>,
    mut glyphs: Query<&mut Transform, With<StarGlyph>>,
    star_lookup: Query<&FloatingStar>,
    mut lines: Query<(&StarLinkLine, &mut Transform), Without<StarGlyph>>,
) {
    for (star, children) in &stars {
        let rotation = Quat::from_rotation_z(star.angle);
        for &child in children.iter() {
            if let Ok(mut transform) = glyphs.get_mut(child) {
                transform.rotation = rotation;
            }
        }
    }

    for (link, mut transform) in &mut lines {
        let Ok(star_a) = star_lookup.get(link.end_a) else {
            continue;
        };
        let Ok(star_b) = star_lookup.get(link.end_b) else {
            continue;
        };
        let delta = star_center(star_b) - star_center(star_a);
        if delta.length_squared() >= 1.0 {
            transform.rotation = Quat::from_rotation_z(delta.y.atan2(delta.x));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_SEP_VIOLATION_RATIO: f32 = 0.6;
    const MAX_QUADRANT_STARS: usize = 5;
    const SIM_VIOLATION_MAX_FRAMES: usize = 30;

    fn test_entity(id: u32) -> Entity {
        Entity::from_raw(id)
    }

    fn min_pairwise_distance(centers: &[Vec2]) -> f32 {
        let mut min = f32::INFINITY;
        for i in 0..centers.len() {
            for j in (i + 1)..centers.len() {
                min = min.min(centers[i].distance(centers[j]));
            }
        }
        min
    }

    fn max_quadrant_count(centers: &[Vec2], w: f32, h: f32) -> usize {
        quadrant_counts(centers, w, h)
            .into_iter()
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn repulsion_pushes_non_linked_stars_apart() {
        let w = 1920.0;
        let h = 1080.0;
        let min_sep = min_runtime_sep_px(w, h);
        let a = test_entity(1);
        let b = test_entity(2);
        let mut centers = HashMap::new();
        centers.insert(a, Vec2::new(100.0, 100.0));
        centers.insert(b, Vec2::new(110.0, 100.0));
        let linked = HashSet::new();

        let forces = accumulate_separation_forces(&centers, &linked, min_sep);
        let fa = forces.get(&a).copied().unwrap_or(Vec2::ZERO);
        let fb = forces.get(&b).copied().unwrap_or(Vec2::ZERO);

        assert!(fa.x < 0.0, "star a should be pushed left");
        assert!(fb.x > 0.0, "star b should be pushed right");
    }

    #[test]
    fn overlap_repulsion_pushes_linked_stars_apart() {
        let a = test_entity(1);
        let b = test_entity(2);
        let size = 40.0;
        let mut centers = HashMap::new();
        let mut sizes = HashMap::new();
        centers.insert(a, Vec2::new(100.0, 100.0));
        centers.insert(b, Vec2::new(115.0, 100.0));
        sizes.insert(a, size);
        sizes.insert(b, size);

        let forces = accumulate_overlap_forces(&centers, &sizes);
        let fa = forces.get(&a).copied().unwrap_or(Vec2::ZERO);
        let fb = forces.get(&b).copied().unwrap_or(Vec2::ZERO);

        assert!(fa.x < 0.0, "overlapping linked stars should still push apart");
        assert!(fb.x > 0.0);
    }

    #[test]
    fn resolve_overlaps_separates_stacked_stars() {
        let a = test_entity(1);
        let b = test_entity(2);
        let size = 40.0;
        let entities = vec![a, b];
        let mut centers = HashMap::new();
        let mut sizes = HashMap::new();
        centers.insert(a, Vec2::new(100.0, 100.0));
        centers.insert(b, Vec2::new(105.0, 100.0));
        sizes.insert(a, size);
        sizes.insert(b, size);

        resolve_star_overlaps(&entities, &mut centers, &sizes);

        let dist = centers[&a].distance(centers[&b]);
        assert!(
            dist >= min_center_separation(size, size),
            "resolved distance {dist} should meet minimum separation"
        );
    }

    #[test]
    fn bidirectional_spring_pushes_when_compressed() {
        let a = test_entity(1);
        let b = test_entity(2);
        let rest = 200.0;
        let mut centers = HashMap::new();
        centers.insert(a, Vec2::new(0.0, 0.0));
        centers.insert(b, Vec2::new(50.0, 0.0));

        let forces = accumulate_spring_forces(&centers, &[(a, b, rest)]);
        let fa = forces.get(&a).copied().unwrap_or(Vec2::ZERO);
        let fb = forces.get(&b).copied().unwrap_or(Vec2::ZERO);

        assert!(fa.x < 0.0, "compressed spring should push a away from b");
        assert!(fb.x > 0.0, "compressed spring should push b away from a");
    }

    #[test]
    fn distribution_predicate_holds_over_simulated_runtime() {
        const W: f32 = 1920.0;
        const H: f32 = 1080.0;
        const DT: f32 = 1.0 / 60.0;
        const FRAMES: usize = 7200;

        let min_sep_threshold = min_runtime_sep_px(W, H) * MIN_SEP_VIOLATION_RATIO;

        let start_pcts = [
            Vec2::new(10.0, 12.0),
            Vec2::new(22.0, 8.0),
            Vec2::new(8.0, 35.0),
            Vec2::new(18.0, 55.0),
            Vec2::new(10.0, 78.0),
            Vec2::new(88.0, 15.0),
            Vec2::new(78.0, 10.0),
            Vec2::new(92.0, 40.0),
            Vec2::new(85.0, 65.0),
            Vec2::new(90.0, 85.0),
            Vec2::new(35.0, 10.0),
            Vec2::new(65.0, 88.0),
        ];

        let entities: Vec<Entity> = (0..12).map(|i| test_entity(i as u32 + 1)).collect();
        let mut centers: HashMap<Entity, Vec2> = entities
            .iter()
            .zip(start_pcts)
            .map(|(&e, pct)| {
                (
                    e,
                    Vec2::new(pct.x / 100.0 * W, pct.y / 100.0 * H),
                )
            })
            .collect();
        let homes_pct: HashMap<Entity, Vec2> = entities
            .iter()
            .zip(start_pcts)
            .map(|(&e, pct)| (e, pct))
            .collect();
        let mut velocities: HashMap<Entity, Vec2> = entities
            .iter()
            .enumerate()
            .map(|(i, &e)| {
                let angle = i as f32 * 0.7;
                (e, Vec2::new(angle.cos() * 8.0, angle.sin() * 8.0))
            })
            .collect();

        let sizes: HashMap<Entity, f32> = entities.iter().map(|&e| (e, 32.0)).collect();
        let entities_list = entities.clone();
        let spring_links = vec![
            (entities[0], entities[1], 180.0),
            (entities[0], entities[2], 200.0),
            (entities[3], entities[4], 190.0),
            (entities[5], entities[6], 170.0),
            (entities[7], entities[8], 210.0),
            (entities[9], entities[10], 185.0),
            (entities[10], entities[11], 195.0),
        ];
        let linked: HashSet<(Entity, Entity)> = spring_links
            .iter()
            .map(|&(a, b, _)| normalize_entity_pair(a, b))
            .collect();

        let mut sep_violation_streak = 0usize;
        let mut max_sep_violation_streak = 0usize;
        let mut quad_violation_streak = 0usize;
        let mut max_quad_violation_streak = 0usize;

        for _ in 0..FRAMES {
            let forces = accumulate_star_forces(
                &centers,
                &sizes,
                &homes_pct,
                &spring_links,
                &linked,
                W,
                H,
            );
            for &entity in &entities {
                if let Some(force) = forces.get(&entity) {
                    *velocities.get_mut(&entity).unwrap() += *force * DT;
                }
                let vel = *velocities.get(&entity).unwrap();
                *centers.get_mut(&entity).unwrap() += vel * DT;
            }
            resolve_star_overlaps(&entities_list, &mut centers, &sizes);

            let center_list: Vec<Vec2> = entities.iter().map(|e| centers[e]).collect();
            let min_dist = min_pairwise_distance(&center_list);
            let min_overlap_threshold = min_center_separation(32.0, 32.0) * MIN_SEP_VIOLATION_RATIO;
            if min_dist < min_overlap_threshold.max(min_sep_threshold) {
                sep_violation_streak += 1;
                max_sep_violation_streak = max_sep_violation_streak.max(sep_violation_streak);
            } else {
                sep_violation_streak = 0;
            }

            let max_quad = max_quadrant_count(&center_list, W, H);
            if max_quad > MAX_QUADRANT_STARS {
                quad_violation_streak += 1;
                max_quad_violation_streak = max_quad_violation_streak.max(quad_violation_streak);
            } else {
                quad_violation_streak = 0;
            }
        }

        assert!(
            max_sep_violation_streak <= SIM_VIOLATION_MAX_FRAMES,
            "min distance below threshold for {max_sep_violation_streak} consecutive frames (max allowed {SIM_VIOLATION_MAX_FRAMES})"
        );
        assert!(
            max_quad_violation_streak <= SIM_VIOLATION_MAX_FRAMES,
            "quadrant over cap for {max_quad_violation_streak} consecutive frames (max allowed {SIM_VIOLATION_MAX_FRAMES})"
        );
    }
}
