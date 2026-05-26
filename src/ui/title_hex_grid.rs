//! Main-menu hex grid — spaced tiles that swell toward the cursor.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::PrimaryWindow;

use crate::app_state::AppState;
use crate::hexgrid::{axial_to_pixel, hex_corners_local, hex_disk};
use crate::ui::{smooth_follow, GOLD};

pub const MENU_HEX_SIZE: f32 = 22.0;
pub const MENU_GRID_RADIUS: i32 = 18;
const LAYOUT_SPACING: f32 = 1.48;
const MOUSE_INFLUENCE_RADIUS: f32 = 340.0;
const HEX_SCALE_SMOOTH: f32 = 5.5;

#[derive(Component)]
pub struct TitleHexGrid;

#[derive(Component)]
pub(crate) struct TitleHexTileMotion {
    scale: f32,
}

impl TitleHexTileMotion {
    fn new() -> Self {
        Self { scale: 1.0 }
    }
}

#[derive(Component)]
pub(crate) struct TitleHexTile {
    base: Vec2,
}

pub fn spawn_title_hex_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let fill_mesh = meshes.add(make_unit_hex_fill_mesh(MENU_HEX_SIZE));
    let border_mesh = meshes.add(make_unit_hex_border_mesh(MENU_HEX_SIZE));
    let black = Color::srgb(0.02, 0.02, 0.03);
    let fill_material = materials.add(ColorMaterial::from_color(black));
    let border_material = materials.add(ColorMaterial::from_color(GOLD));

    let layout_size = MENU_HEX_SIZE * LAYOUT_SPACING;

    for coord in hex_disk(MENU_GRID_RADIUS) {
        let (cx, cy) = axial_to_pixel(coord.q, coord.r, layout_size);
        let base = Vec2::new(cx, cy);

        commands
            .spawn((
                TitleHexTile { base },
                TitleHexTileMotion::new(),
                TitleHexGrid,
                Transform::from_xyz(base.x, base.y, 0.0),
                Visibility::Visible,
            ))
            .with_children(|hex| {
                hex.spawn((
                    Mesh2d(fill_mesh.clone()),
                    MeshMaterial2d(fill_material.clone()),
                    Transform::default(),
                ));
                hex.spawn((
                    Mesh2d(border_mesh.clone()),
                    MeshMaterial2d(border_material.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                ));
            });
    }
}

fn make_unit_hex_fill_mesh(size: f32) -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]];
    for &(x, y) in &hex_corners_local(size) {
        positions.push([x, y, 0.0]);
    }
    let mut indices = Vec::new();
    for i in 0..6 {
        indices.push(0);
        indices.push(i as u32 + 1);
        indices.push(((i + 1) % 6) as u32 + 1);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn make_unit_hex_border_mesh(size: f32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = hex_corners_local(size)
        .iter()
        .map(|&(x, y)| [x, y, 0.0])
        .collect();
    positions.push(positions[0]);
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::LineStrip, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn screen_to_world(cam: &GlobalTransform, window_size: Vec2, screen_pos: Vec2, zoom: f32) -> Vec2 {
    let ndc = Vec2::new(
        (screen_pos.x / window_size.x) * 2.0 - 1.0,
        ((window_size.y - screen_pos.y) / window_size.y) * 2.0 - 1.0,
    );
    cam.translation().truncate() + ndc * window_size * 0.5 * zoom
}

fn mouse_influence(dist: f32) -> f32 {
    let t = (1.0 - dist / MOUSE_INFLUENCE_RADIUS).clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn max_tiling_scale() -> f32 {
    let layout_size = MENU_HEX_SIZE * LAYOUT_SPACING;
    let circumradius = hex_corners_local(MENU_HEX_SIZE)
        .iter()
        .map(|&(x, y)| (x * x + y * y).sqrt())
        .fold(0.0_f32, f32::max);
    let (nx, ny) = axial_to_pixel(1, 0, layout_size);
    let neighbor_dist = (nx * nx + ny * ny).sqrt();
    neighbor_dist / (circumradius * 3.0_f32.sqrt())
}

fn target_hex_scale(influence: f32) -> f32 {
    let max_scale = max_tiling_scale();
    1.0 + (max_scale - 1.0) * influence
}

pub fn animate_title_hex_tiles(
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&GlobalTransform, &OrthographicProjection), With<Camera2d>>,
    mut tiles: Query<(&TitleHexTile, &mut TitleHexTileMotion, &mut Transform), With<TitleHexGrid>>,
) {
    let dt = time.delta_secs();
    let Ok(window) = window.get_single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        for (tile, mut motion, mut transform) in &mut tiles {
            motion.scale = smooth_follow(motion.scale, 1.0, dt, HEX_SCALE_SMOOTH);
            transform.translation = Vec3::new(tile.base.x, tile.base.y, 0.0);
            transform.scale = Vec3::splat(motion.scale);
        }
        return;
    };
    let Ok((cam, proj)) = camera.get_single() else {
        return;
    };

    let ws = Vec2::new(window.width(), window.height());
    let mouse = screen_to_world(cam, ws, cursor, proj.scale);

    for (tile, mut motion, mut transform) in &mut tiles {
        let dist = (mouse - tile.base).length();
        let influence = mouse_influence(dist);
        let target_scale = target_hex_scale(influence);

        motion.scale = smooth_follow(motion.scale, target_scale, dt, HEX_SCALE_SMOOTH);
        transform.translation = Vec3::new(tile.base.x, tile.base.y, 0.0);
        transform.scale = Vec3::splat(motion.scale);
    }
}

pub fn sync_title_hex_grid_visibility(
    app_state: Res<State<AppState>>,
    mut query: Query<&mut Visibility, With<TitleHexGrid>>,
) {
    if !app_state.is_changed() {
        return;
    }
    let visible = if *app_state.get() == AppState::MainMenu {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut query {
        *vis = visible;
    }
}
