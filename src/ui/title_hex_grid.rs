//! Main-menu hex grid — spaced tiles that swell toward the cursor.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::PrimaryWindow;

use crate::app_state::AppState;
use crate::hexgrid::{axial_to_pixel, hex_corners_local, hex_disk};
use crate::ui::GOLD;

pub const MENU_HEX_SIZE: f32 = 22.0;
pub const MENU_GRID_RADIUS: i32 = 18;
const LAYOUT_SPACING: f32 = 1.48;
const MOUSE_INFLUENCE_RADIUS: f32 = 260.0;
const MAX_SCALE_BOOST: f32 = 1.1;
const MOUSE_PULL: f32 = 0.18;

#[derive(Component)]
pub struct TitleHexGrid;

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
    t * t * (3.0 - 2.0 * t)
}

pub fn animate_title_hex_tiles(
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&GlobalTransform, &OrthographicProjection), With<Camera2d>>,
    mut tiles: Query<(&TitleHexTile, &mut Transform), With<TitleHexGrid>>,
) {    let Ok(window) = window.get_single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((cam, proj)) = camera.get_single() else {
        return;
    };

    let ws = Vec2::new(window.width(), window.height());
    let mouse = screen_to_world(cam, ws, cursor, proj.scale);

    for (tile, mut transform) in &mut tiles {
        let delta = mouse - tile.base;
        let dist = delta.length();
        let influence = mouse_influence(dist);
        let scale = 1.0 + MAX_SCALE_BOOST * influence;
        let pull = if dist > 0.5 {
            delta / dist * (dist * MOUSE_PULL * influence)
        } else {
            Vec2::ZERO
        };
        let pos = tile.base + pull;
        transform.translation = Vec3::new(pos.x, pos.y, 0.0);
        transform.scale = Vec3::splat(scale);
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
