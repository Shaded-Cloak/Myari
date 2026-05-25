mod game;
mod hexgrid;
mod map;
mod outer_islands;
mod rng;

use bevy::color::Color;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};
use bevy_pancam::{PanCam, PanCamPlugin};
use rand::Rng;
use std::collections::HashSet;

use game::GameState;
use crate::hexgrid::{axial_to_pixel, hex_corners_at, hex_corners_local, pixel_to_hex, HexCoord};
use map::{Map, HexTile, TerrainType, MAP_RADIUS};

const HEX_SIZE: f32 = 28.0;

fn main() {
    println!("Myari starting up...");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MYARI".into(),
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(PanCamPlugin)
        .init_resource::<SelectedHex>()
        .init_resource::<HoveredHex>()
        .init_resource::<FpsCounter>()
        .init_resource::<GridVisible>()
        .init_resource::<MenuOpen>()
        .init_resource::<Zoom>()
        .init_resource::<CurrentSeed>()
        .init_resource::<SavePath>()
        .add_systems(Startup, (setup_camera, spawn_map_and_game, fps_startup, spawn_menu))
        .add_systems(
            Update,
            (
                sync_pancam,
                sync_zoom_from_camera,
                track_hover,
                highlight_hover,
                update_hover_tile_text,
                handle_selection,
                move_selected_unit,
                toggle_menu,
                handle_toggle_button,
                fps_update,
                end_turn,
                reroll_world,
                save_game,
                load_game,
            ),
        )
        .run();
}

// ── Resources ───────────────────────────────────────────────────

#[derive(Resource, Default)]
struct SelectedHex(Option<HexCoord>);

#[derive(Resource)]
struct GameMap(Map);

#[derive(Resource, Default)]
struct CurrentSeed(u64);

#[derive(Resource, Default)]
struct HoveredHex(Option<HexCoord>);

#[derive(Resource, Default)]
struct FpsCounter {
    elapsed: f32,
    frames: u32,
}

// ── Components ──────────────────────────────────────────────────

#[derive(Component)]
struct FpsText;

#[derive(Component)]
struct Highlight;

#[derive(Resource)]
struct GridMesh(Handle<Mesh>);

#[derive(Resource)]
struct GridMaterial(Handle<ColorMaterial>);

#[derive(Resource, Default)]
struct GridVisible(bool);

#[derive(Resource, Default)]
struct MenuOpen(bool);

#[derive(Resource)]
struct Zoom(f32);

impl Default for Zoom {
    fn default() -> Self { Zoom(1.0) }
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct GridToggle;

#[derive(Component)]
struct GridMarker;

#[derive(Component)]
struct TurnText;

#[derive(Component)]
struct SeedText;

#[derive(Component)]
struct WorldEntity;

#[derive(Component)]
struct HoverTileText;

#[derive(Component)]
struct UnitMarker {
    civ_idx: usize,
    unit_idx: usize,
}

#[derive(Resource, Default)]
struct SavePath(Option<String>);

// ── Startup ─────────────────────────────────────────────────────

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Left, MouseButton::Middle],
            zoom_to_cursor: true,
            min_scale: 0.02,
            max_scale: 60.0,
            ..default()
        },
    ));
}

fn sync_pancam(menu: Res<MenuOpen>, mut cameras: Query<&mut PanCam>) {
    if let Ok(mut pan) = cameras.get_single_mut() {
        pan.enabled = !menu.0;
    }
}

fn sync_zoom_from_camera(
    mut zoom: ResMut<Zoom>,
    cameras: Query<&OrthographicProjection, With<Camera2d>>,
) {
    if let Ok(proj) = cameras.get_single() {
        zoom.0 = proj.scale;
    }
}

fn spawn_map_and_game(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut seed_res: ResMut<CurrentSeed>,
    mut save_path: ResMut<SavePath>,
) {
    println!("spawn_map_and_game called");
    save_path.0 = Some(default_save_path());
    let seed = rand::thread_rng().gen::<u64>();
    seed_res.0 = seed;
    println!("World seed: {seed}");

    let (map, gs) = spawn_world_entities(&mut commands, &mut meshes, &mut materials, seed);
    commands.insert_resource(GameMap(map));
    commands.insert_resource(gs);

    // Turn counter
    commands.spawn((
        Text::new("Turn: 1"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.9, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        },
        TurnText,
    ));

    commands.spawn((
        Text::new(format!("Seed: {seed} (R to reroll)")),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.8, 0.8, 0.9)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(10.0),
            top: Val::Px(34.0),
            ..default()
        },
        SeedText,
    ));

    // Hover highlight entity (single hex outline)
    let hm = meshes.add(make_hex_outline_mesh(HEX_SIZE));
    commands.spawn((
        Mesh2d(hm),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgba(
            0.05, 0.95, 1.0, 1.0,
        )))),
        Transform::from_xyz(0.0, 0.0, 5.0),
        Visibility::Hidden,
        Highlight,
    ));

    commands.spawn((
        Text::new("Hover: --"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.92, 0.92)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            bottom: Val::Px(10.0),
            ..default()
        },
        HoverTileText,
    ));
}

fn default_save_path() -> String {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{base}\\Myari\\save.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SaveData {
    seed: u64,
    tiles: Vec<HexTile>,
    game_state: GameState,
}

fn save_game(
    keys: Res<ButtonInput<KeyCode>>,
    save_path: Res<SavePath>,
    seed: Res<CurrentSeed>,
    gs: Res<GameState>,
    map: Res<GameMap>,
) {
    if !keys.just_pressed(KeyCode::F5) {
        return;
    }
    let Some(path) = save_path.0.as_ref() else {
        return;
    };
    let data = SaveData {
        seed: seed.0,
        tiles: map.0.tiles.clone(),
        game_state: gs.clone(),
    };
    let Ok(text) = serde_json::to_string_pretty(&data) else {
        return;
    };
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(path, text).is_ok() {
        println!("Saved game to: {path}");
    }
}

fn load_game(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut gs: ResMut<GameState>,
    mut game_map: ResMut<GameMap>,
    mut seed_res: ResMut<CurrentSeed>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<TurnText>>,
        Query<&mut Text, With<SeedText>>,
    )>,
    world_entities: Query<Entity, With<WorldEntity>>,
) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    let path = default_save_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("No save found at: {path}");
        return;
    };
    let Ok(data) = serde_json::from_str::<SaveData>(&text) else {
        println!("Failed to parse save at: {path}");
        return;
    };

    for e in &world_entities {
        commands.entity(e).despawn_recursive();
    }

    seed_res.0 = data.seed;
    let map = Map::from_tiles(data.tiles);
    *gs = data.game_state;
    spawn_world_visuals(&mut commands, &mut meshes, &mut materials, &map, &gs);
    game_map.0 = map;

    if let Ok(mut text) = text_queries.p0().get_single_mut() {
        text.0 = format!("Turn: {}", gs.turn);
    }
    if let Ok(mut text) = text_queries.p1().get_single_mut() {
        text.0 = format!("Seed: {} (R to reroll)", data.seed);
    }
    println!("Loaded game from: {path}");
}

fn spawn_world_visuals(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    map: &Map,
    gs: &GameState,
) {
    let map_mesh = meshes.add(make_combined_hex_mesh(&map.tiles, HEX_SIZE));
    commands.spawn((
        Mesh2d(map_mesh),
        MeshMaterial2d(materials.add(ColorMaterial::default())),
        Transform::default(),
        WorldEntity,
    ));

    let grid_mesh = meshes.add(make_grid_mesh(&map.tiles, HEX_SIZE));
    commands.insert_resource(GridMesh(grid_mesh.clone()));
    let grid_mat = materials.add(ColorMaterial::from_color(Color::srgb(0.0, 0.0, 0.0)));
    commands.insert_resource(GridMaterial(grid_mat.clone()));
    commands.spawn((
        Mesh2d(grid_mesh),
        MeshMaterial2d(grid_mat),
        Transform::from_xyz(0.0, 0.0, 4.0),
        Visibility::Hidden,
        GridMarker,
        WorldEntity,
    ));

    // Civilization city/unit markers temporarily disabled — game state still
    // tracks them, but we don't draw the red/blue/green dots yet.
    // spawn_civ_markers(commands, meshes, materials, gs);
    let _ = (commands, meshes, materials, gs);
}

#[allow(dead_code)] // temporarily disabled — see spawn_world_visuals
fn spawn_civ_markers(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    gs: &GameState,
) {
    for (civ_idx, civ) in gs.civs.iter().enumerate() {
        let color = Color::srgb(
            civ.color.0 as f32 / 255.0,
            civ.color.1 as f32 / 255.0,
            civ.color.2 as f32 / 255.0,
        );
        for city in &civ.cities {
            let (cx, cy) = axial_to_pixel(city.coord.q, city.coord.r, HEX_SIZE);
            commands.spawn((
                Mesh2d(meshes.add(Circle::new(10.0))),
                MeshMaterial2d(materials.add(ColorMaterial::from_color(color))),
                Transform::from_xyz(cx, cy, 3.0),
                WorldEntity,
            ));
        }
        for (unit_idx, unit) in civ.units.iter().enumerate() {
            let (ux, uy) = axial_to_pixel(unit.coord.q, unit.coord.r, HEX_SIZE);
            commands.spawn((
                Mesh2d(meshes.add(Circle::new(5.0))),
                MeshMaterial2d(materials.add(ColorMaterial::from_color(color))),
                Transform::from_xyz(ux, uy, 3.0),
                UnitMarker { civ_idx, unit_idx },
                WorldEntity,
            ));
        }
    }
}

fn spawn_world_entities(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    seed: u64,
) -> (Map, GameState) {
    let map = Map::generate(MAP_RADIUS, seed);
    let gs = GameState::new(&map);
    spawn_world_visuals(commands, meshes, materials, &map, &gs);
    (map, gs)
}

// ── Mesh helpers ────────────────────────────────────────────────

fn make_hex_mesh(size: f32) -> Mesh {
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

fn make_hex_outline_mesh(size: f32) -> Mesh {
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

fn make_combined_hex_mesh(tiles: &[HexTile], size: f32) -> Mesh {
    // 7 vertices per tile (center + 6 corners), 18 indices per tile
    // (6 triangles, 3 verts each). Pre-reserving prevents the dozen-or-so
    // realloc/copy cycles a Vec would otherwise do when growing to ~1M verts.
    let n = tiles.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 7);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(n * 7);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 18);
    let mut base: u32 = 0;

    for tile in tiles {
        let (cx, cy) = axial_to_pixel(tile.coord.q, tile.coord.r, size);
        let c = terrain_to_color(tile.terrain).to_linear().to_f32_array();

        positions.push([cx, cy, 0.0]);
        colors.push(c);

        for &(x, y) in &hex_corners_at(tile.coord.q, tile.coord.r, size) {
            positions.push([x, y, 0.0]);
            colors.push(c);
        }

        // 6 triangles (center + 2 adjacent corners)
        for i in 0..6 {
            indices.push(base);
            indices.push(base + 1 + i);
            indices.push(base + 1 + ((i + 1) % 6));
        }

        base += 7;
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn make_grid_mesh(tiles: &[HexTile], size: f32) -> Mesh {
    // ~3 unique edges per tile on average. Pre-size accordingly.
    let n = tiles.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 6);
    let mut seen: HashSet<((i32, i32), (i32, i32))> = HashSet::with_capacity(n * 3);
    let edge_key = |a: HexCoord, b: HexCoord| -> ((i32, i32), (i32, i32)) {
        let aa = (a.q, a.r);
        let bb = (b.q, b.r);
        if aa <= bb { (aa, bb) } else { (bb, aa) }
    };
    for tile in tiles {
        let mut corners: [[f32; 3]; 6] = [[0.0; 3]; 6];
        for (i, &(x, y)) in hex_corners_at(tile.coord.q, tile.coord.r, size).iter().enumerate() {
            corners[i] = [x, y, 0.0];
        }
        let neighbors = tile.coord.neighbors();
        for i in 0..6 {
            let key = edge_key(neighbors[i], neighbors[(i + 1) % 6]);
            if seen.insert(key) {
                positions.push(corners[i]);
                positions.push(corners[(i + 1) % 6]);
            }
        }
    }
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ── Color helpers ───────────────────────────────────────────────

fn terrain_to_color(t: TerrainType) -> Color {
    match t {
        TerrainType::DeepOcean => Color::srgb_u8(0x0d, 0x1b, 0x3e),
        TerrainType::Ocean => Color::srgb_u8(0x1a, 0x30, 0x60),
        TerrainType::Coast => Color::srgb_u8(0x2e, 0x6e, 0xa6),
        TerrainType::Freshwater => Color::srgb_u8(0x4a, 0xa0, 0xd0),
        TerrainType::Beach => Color::srgb_u8(0xc2, 0xa9, 0x6e),
        TerrainType::Hills => Color::srgb_u8(0x7a, 0x6a, 0x4a),
        TerrainType::Mountain => Color::srgb_u8(0x5a, 0x5a, 0x5a),
        TerrainType::SnowPeak => Color::srgb_u8(0xdc, 0xe8, 0xf0),
        TerrainType::StonySlope => Color::srgb_u8(0x7a, 0x68, 0x58),
        TerrainType::AridPeak => Color::srgb_u8(0xb0, 0x78, 0x40),
        TerrainType::GlacialPeak => Color::srgb_u8(0xa8, 0xd8, 0xf0),
        TerrainType::Ashplain => Color::srgb_u8(0xb0, 0x9a, 0x6a),
        TerrainType::Thornveld => Color::srgb_u8(0x8a, 0x78, 0x30),
        TerrainType::Deepjungle => Color::srgb_u8(0x1a, 0x5c, 0x28),
        TerrainType::Steppe => Color::srgb_u8(0xc8, 0xb8, 0x4a),
        TerrainType::Plains => Color::srgb_u8(0x7a, 0xb8, 0x40),
        TerrainType::Greenfield => Color::srgb_u8(0x4e, 0x9e, 0x30),
        TerrainType::Oldwood => Color::srgb_u8(0x2a, 0x6e, 0x38),
        TerrainType::Snowfield => Color::srgb_u8(0xc8, 0xd8, 0xe0),
        TerrainType::Frostmoor => Color::srgb_u8(0x6a, 0x8a, 0x9a),
        TerrainType::Darkpine => Color::srgb_u8(0x1e, 0x44, 0x28),
        TerrainType::AncientRuin => Color::srgb_u8(0x7a, 0x68, 0x48),
        TerrainType::Corrupted => Color::srgb_u8(0x5a, 0x28, 0x78),
        TerrainType::LeyGrove => Color::srgb_u8(0x2a, 0x7a, 0x5a),
        TerrainType::LeyWaste => Color::srgb_u8(0x6a, 0x3a, 0x6a),
        TerrainType::BlightedWaste => Color::srgb_u8(0x3a, 0x1a, 0x4a),
        TerrainType::RuinField => Color::srgb_u8(0x5a, 0x4a, 0x38),
        TerrainType::SacredGround => Color::srgb_u8(0xc8, 0xa8, 0x30),
    }
}

// ── Hover (cheap: single entity, no material mutation) ──────────

fn screen_to_world(cam: &Transform, window_size: Vec2, screen_pos: Vec2, zoom: f32) -> Vec2 {
    let ndc = Vec2::new(
        (screen_pos.x / window_size.x) * 2.0 - 1.0,
        ((window_size.y - screen_pos.y) / window_size.y) * 2.0 - 1.0,
    );
    cam.translation.truncate() + ndc * window_size * 0.5 * zoom
}

fn track_hover(
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, With<Camera2d>>,
    mut hovered: ResMut<HoveredHex>,
    zoom: Res<Zoom>,
    mut cursor_evr: EventReader<CursorMoved>,
) {
    let Some(ev) = cursor_evr.read().last() else { return; };
    let cam = camera.single();
    let ws = Vec2::new(window.single().width(), window.single().height());
    let world = screen_to_world(cam, ws, ev.position, zoom.0);
    hovered.0 = Some(pixel_to_hex(world.x, world.y, HEX_SIZE));
}

fn highlight_hover(
    hovered: Res<HoveredHex>,
    mut query: Query<(&mut Transform, &mut Visibility), With<Highlight>>,
    mut prev: Local<Option<HexCoord>>,
) {
    if hovered.0 == *prev { return; }
    *prev = hovered.0;
    let Ok((mut transform, mut visibility)) = query.get_single_mut() else {
        return;
    };
    if let Some(hex) = hovered.0 {
        let (wx, wy) = axial_to_pixel(hex.q, hex.r, HEX_SIZE);
        transform.translation = Vec3::new(wx, wy, 5.0);
        *visibility = Visibility::Visible;
    } else {
        *visibility = Visibility::Hidden;
    }
}

// ── Tile selection (right-click) ────────────────────────────────

fn handle_selection(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, With<Camera2d>>,
    mut selected: ResMut<SelectedHex>,
    zoom: Res<Zoom>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(mouse_pos) = window.single().cursor_position() else {
        return;
    };
    let cam = camera.single();
    let ws = Vec2::new(window.single().width(), window.single().height());
    let world = screen_to_world(cam, ws, mouse_pos, zoom.0);
    selected.0 = Some(pixel_to_hex(world.x, world.y, HEX_SIZE));
}

fn move_selected_unit(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<SelectedHex>,
    hovered: Res<HoveredHex>,
    map: Res<GameMap>,
    mut gs: ResMut<GameState>,
    mut units: Query<(&mut Transform, &UnitMarker)>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    let Some(from) = selected.0 else {
        return;
    };
    let Some(to) = hovered.0 else {
        return;
    };

    let (civ_idx, unit_idx) = match gs.unit_at(from) {
        Some((ci, ui)) if ci == GameState::PLAYER_CIV => (ci, ui),
        _ => return,
    };

    if !gs.try_move_unit(civ_idx, unit_idx, to, &map.0) {
        return;
    }

    // Update any unit marker that corresponds to the moved unit.
    for (mut t, marker) in &mut units {
        if marker.civ_idx == civ_idx && marker.unit_idx == unit_idx {
            let unit = &gs.civs[civ_idx].units[unit_idx];
            let (ux, uy) = axial_to_pixel(unit.coord.q, unit.coord.r, HEX_SIZE);
            t.translation.x = ux;
            t.translation.y = uy;
        }
    }
}

// ── FPS counter ─────────────────────────────────────────────────

fn fps_startup(mut commands: Commands) {
    commands.spawn((
        Text::new("FPS: --"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        },
        FpsText,
    ));
}

fn fps_update(
    time: Res<Time>,
    mut fps: ResMut<FpsCounter>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    fps.elapsed += time.delta_secs();
    fps.frames += 1;
    if fps.elapsed >= 0.5 {
        let val = fps.frames as f32 / fps.elapsed;
        if let Ok(mut text) = query.get_single_mut() {
            text.0 = format!("FPS: {:.0}", val);
        }
        fps.elapsed = 0.0;
        fps.frames = 0;
    }
}

// ── Settings Menu ───────────────────────────────────────────────

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            MenuRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("SETTINGS"),
                TextFont { font_size: 28.0, ..default() },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
            ));
            // Toggle button
            parent
                .spawn((
                    Button,
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.2, 0.2, 0.3)),
                    GridToggle,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Grid: OFF"),
                        TextFont { font_size: 20.0, ..default() },
                        TextColor(Color::srgb(0.8, 0.8, 0.8)),
                    ));
                });
            parent.spawn((
                Text::new("ESC - Close"),
                TextFont { font_size: 16.0, ..default() },
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
            ));
        });
}

fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut menu_open: ResMut<MenuOpen>,
    mut query: Query<&mut Node, With<MenuRoot>>,
    mut was_pressed: Local<bool>,
    mut last_toggle: Local<f32>,
) {
    // Two-belt defence against Escape double-toggling:
    //   1. Rising-edge detector — must release before toggling again.
    //   2. 200 ms cooldown after each toggle — covers any case where the key
    //      state appears to flicker (e.g. OS key auto-repeat surfacing as a
    //      release+press pair within a single tick).
    let is_pressed = keys.pressed(KeyCode::Escape);
    if !is_pressed {
        *was_pressed = false;
        return;
    }
    if *was_pressed {
        return;
    }
    let now = time.elapsed_secs();
    if now - *last_toggle < 0.2 {
        return;
    }
    *was_pressed = true;
    *last_toggle = now;

    menu_open.0 = !menu_open.0;
    if let Ok(mut node) = query.get_single_mut() {
        node.display = if menu_open.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn handle_toggle_button(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor, &Children),
        (Changed<Interaction>, With<GridToggle>),
    >,
    mut grid_visible: ResMut<GridVisible>,
    mut grid_query: Query<&mut Visibility, With<GridMarker>>,
    mut text_query: Query<&mut Text>,
) {
    for (interaction, mut bg, children) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                grid_visible.0 = !grid_visible.0;
                if let Ok(mut vis) = grid_query.get_single_mut() {
                    *vis = if grid_visible.0 {
                        Visibility::Visible
                    } else {
                        Visibility::Hidden
                    };
                }
                bg.0 = Color::srgb(0.3, 0.3, 0.5);
                for &child in children.iter() {
                    if let Ok(mut text) = text_query.get_mut(child) {
                        text.0 = if grid_visible.0 {
                            "Grid: ON".to_string()
                        } else {
                            "Grid: OFF".to_string()
                        };
                    }
                }
            }
            Interaction::Hovered => {
                bg.0 = Color::srgb(0.3, 0.3, 0.4);
            }
            Interaction::None => {
                bg.0 = Color::srgb(0.2, 0.2, 0.3);
            }
        }
    }
}

fn update_hover_tile_text(
    hovered: Res<HoveredHex>,
    map: Res<GameMap>,
    mut query: Query<&mut Text, With<HoverTileText>>,
    mut prev: Local<Option<HexCoord>>,
) {
    // Only rebuild the formatted string when the hovered tile actually
    // changes — saves per-frame allocations and `tile_at` lookups.
    if *prev == hovered.0 {
        return;
    }
    *prev = hovered.0;
    let Ok(mut text) = query.get_single_mut() else {
        return;
    };

    let Some(coord) = hovered.0 else {
        text.0 = "Hover: --".to_string();
        return;
    };

    if let Some(tile) = map.0.tile_at(coord) {
        text.0 = format!(
            "Hover: {}  |  q={}, r={}",
            terrain_label(tile.terrain),
            coord.q,
            coord.r
        );
    } else {
        text.0 = format!("Hover: Out of map  |  q={}, r={}", coord.q, coord.r);
    }
}

fn terrain_label(t: TerrainType) -> &'static str {
    match t {
        TerrainType::DeepOcean => "Deep Ocean",
        TerrainType::Ocean => "Ocean",
        TerrainType::Coast => "Coast",
        TerrainType::Freshwater => "Freshwater",
        TerrainType::Beach => "Beach",
        TerrainType::Hills => "Hills",
        TerrainType::Mountain => "Mountain",
        TerrainType::SnowPeak => "Snow Peak",
        TerrainType::StonySlope => "Stony Slope",
        TerrainType::AridPeak => "Arid Peak",
        TerrainType::GlacialPeak => "Glacial Peak",
        TerrainType::Ashplain => "Ashplain",
        TerrainType::Thornveld => "Thornveld",
        TerrainType::Deepjungle => "Deepjungle",
        TerrainType::Steppe => "Steppe",
        TerrainType::Plains => "Plains",
        TerrainType::Greenfield => "Greenfield",
        TerrainType::Oldwood => "Oldwood",
        TerrainType::Snowfield => "Snowfield",
        TerrainType::Frostmoor => "Frostmoor",
        TerrainType::Darkpine => "Darkpine",
        TerrainType::AncientRuin => "Ancient Ruin",
        TerrainType::Corrupted => "Corrupted",
        TerrainType::LeyGrove => "Ley Grove",
        TerrainType::LeyWaste => "Ley Waste",
        TerrainType::BlightedWaste => "Blighted Waste",
        TerrainType::RuinField => "Ruin Field",
        TerrainType::SacredGround => "Sacred Ground",
    }
}

fn end_turn(
    keys: Res<ButtonInput<KeyCode>>,
    mut gs: ResMut<GameState>,
    mut query: Query<&mut Text, With<TurnText>>,
    map: Res<GameMap>,
) {
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    gs.next_turn(&map.0);
    if let Ok(mut text) = query.get_single_mut() {
        text.0 = format!("Turn: {}", gs.turn);
    }
}

fn reroll_world(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut gs: ResMut<GameState>,
    mut game_map: ResMut<GameMap>,
    mut seed_res: ResMut<CurrentSeed>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<TurnText>>,
        Query<&mut Text, With<SeedText>>,
    )>,
    world_entities: Query<Entity, With<WorldEntity>>,
    mut selected: ResMut<SelectedHex>,
    mut hovered: ResMut<HoveredHex>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }

    for e in &world_entities {
        commands.entity(e).despawn_recursive();
    }

    let seed = rand::thread_rng().gen::<u64>();
    seed_res.0 = seed;
    println!("World rerolled with seed: {seed}");

    let (map, new_gs) = spawn_world_entities(&mut commands, &mut meshes, &mut materials, seed);
    game_map.0 = map;
    *gs = new_gs;

    selected.0 = None;
    hovered.0 = None;

    if let Ok(mut text) = text_queries.p0().get_single_mut() {
        text.0 = "Turn: 1".to_string();
    }
    if let Ok(mut text) = text_queries.p1().get_single_mut() {
        text.0 = format!("Seed: {seed} (R to reroll)");
    }
}
