mod app_state;
mod buildings;
mod footprint_mesh;
mod game;
mod hexgrid;
mod map;
mod outer_islands;
mod rng;
mod ui;

use app_state::{AppState, InGameHud, LoadingJob, LoadingProgress};
use bevy::hierarchy::Parent;
use bevy::color::Color;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};
use bevy::transform::TransformSystem;
use bevy::ui::{FocusPolicy, UiSystem};
use bevy_pancam::{PanCam, PanCamPlugin};
use rand::Rng;
use std::collections::{HashMap, HashSet};

use buildings::{can_place_lodge, lodge_coords, PlacedLodges};
use game::GameState;
use crate::hexgrid::{axial_to_pixel, hex_corners_at, hex_corners_local, hex_world_bounds, pixel_to_hex, HexCoord};
use map::{Map, HexTile, TerrainType, MAP_RADIUS};
use ui::{
    hover_gem::{spawn_hover_gem_swatch, HoverSwatch},
    loading_screen::{spawn_loading_screen, sync_loading_ui, LoadingRoot},
    menu_button_row_bundle, menu_panel_intro_transform,
    menu_framed_overlay, menu_panel_bundle_with_fade, menu_panel_outro_transform, spawn_framed_panel, spawn_menu_button_label,
    spawn_ornate_divider, spawn_star_watermark, title_menu::{
        self, spawn_title_menu, sync_title_subscreen, TitleRoot, TitleScreen,
    },
    apply_menu_fade_alpha, ease_out_cubic, smooth_follow_vec2_distance_speed, BlocksWorldInput, HudAnchor,
    MenuButton, MenuButtonFill, MenuFadeLayer,
    UiTheme, BTN_HOVER, BTN_IDLE, BTN_PRESSED,
    CREAM, GOLD, HINT, MENU_ENTER_SECS, MENU_EXIT_SECS, MENU_SWITCH_SECS,
    GLOBAL_Z_HUD, GLOBAL_Z_MENU_PANEL, PAUSE_WORLD_DIM_ALPHA,
};

const HEX_SIZE: f32 = 28.0;
const MIN_LOAD_SECS: f32 = 1.55;
const LOAD_INTRO_SECS: f32 = 0.35;
const LOAD_WORLD_SECS: f32 = 0.55;
const LOAD_UI_SECS: f32 = 0.45;
/// Hover ring thickness in world units — scales with the map when zooming.
const HOVER_OUTLINE_STROKE: f32 = 2.2;
/// Map border thickness in world units — scales with the map when zooming.
const MAP_BORDER_STROKE: f32 = 24.0;
/// Slow drift onto the hovered tile; ramps up when the cursor skips ahead.
const HOVER_GLOW_SPEED_NEAR: f32 = 7.5;
const HOVER_GLOW_SPEED_FAR: f32 = 16.0;
/// Full alpha/scale pulse cycle (~2.8s).
const HOVER_GLOW_PULSE_HZ: f32 = 0.36;
const HOVER_GLOW_PULSE_SCALE: f32 = 0.032;
const HOVER_GLOW_ALPHA_BASE: f32 = 0.84;
const HOVER_GLOW_ALPHA_AMP: f32 = 0.12;
/// Extra margin so the map isn't flush against the screen edge.
const MAP_CAMERA_PADDING: f32 = 1.06;
/// Grid fully visible when hex height on screen is at least this many pixels.
const GRID_HEX_PX_FADE_START: f32 = 28.0;
/// Grid hidden when hex height on screen is at or below this (only at extreme zoom-out).
const GRID_HEX_PX_FADE_END: f32 = 7.0;
/// Placed lodge inset fill (opaque; same hue as blueprint).
const LODGE_BUILDING_FILL: Color = Color::srgb(0.45, 0.28, 0.12);
/// Selection highlight fill — blueprint look, fully opaque.
const LODGE_SELECTION_FILL: Color = Color::srgb(0.45, 0.28, 0.12);
const LODGE_BLUEPRINT_FILL: Color = Color::srgba(0.45, 0.28, 0.12, 0.72);
const LODGE_BLUEPRINT_STROKE_VALID: Color = Color::srgba(0.85, 0.72, 0.35, 0.9);
const LODGE_BLUEPRINT_STROKE_INVALID: Color = Color::srgba(0.05, 0.05, 0.05, 0.95);
/// Terrain margin around building fill (constant-distance inset from footprint boundary).
const LODGE_BUILDING_MARGIN: f32 = 7.0;
const LODGE_PLACE_START_SCALE: f32 = 0.2;
const LODGE_PLACE_POP_SECS: f32 = 0.48;

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
        .init_state::<AppState>()
        .init_resource::<SelectedHex>()
        .init_resource::<HoveredHex>()
        .init_resource::<BuildingPlacementMode>()
        .init_resource::<PlacedLodges>()
        .init_resource::<ExaminerSelection>()
        .init_resource::<LodgePlacementRotation>()
        .init_resource::<LodgeBlueprintMeshCache>()
        .init_resource::<LodgePlacementSuppressClick>()
        .init_resource::<FpsCounter>()
        .init_resource::<GridVisible>()
        .init_resource::<MenuScreen>()
        .init_resource::<MenuScreenEpoch>()
        .init_resource::<MenuMotion>()
        .init_resource::<Zoom>()
        .init_resource::<CurrentSeed>()
        .init_resource::<SavePath>()
        .init_resource::<LoadingJob>()
        .init_resource::<LoadingProgress>()
        .init_resource::<TitleScreen>()
        .init_resource::<InGameUiReady>()
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
        .add_systems(Startup, (setup_camera, setup_ui_theme))
        .add_systems(
            Startup,
            (
                init_save_path,
                spawn_title_menu,
                ui::title_hex_grid::spawn_title_hex_grid,
                spawn_loading_screen,
                spawn_pause_menu,
            )
                .after(setup_ui_theme),
        )
        .add_systems(Update, (
            sync_app_screens,
            sync_world_visibility,
            ui::title_hex_grid::sync_title_hex_grid_visibility,
        ))
        .add_systems(
            Update,
            (
                sync_title_subscreen,
                title_menu::handle_title_play,
                title_menu::handle_title_continue,
                title_menu::handle_title_settings,
                title_menu::handle_title_back,
                title_menu::handle_title_quit,
                ui::title_hex_grid::animate_title_hex_tiles,
                title_menu::style_title_menu_buttons,
                handle_toggle_button,
            )
                .run_if(in_state(AppState::MainMenu))
                .before(UiSystem::Layout),
        )
        .add_systems(
            PostUpdate,
            title_menu::animate_title_menu_hover
                .run_if(in_state(AppState::MainMenu))
                .after(UiSystem::Layout),
        )
        .add_systems(
            Update,
            (loading_pipeline, sync_loading_ui).run_if(in_state(AppState::Loading)),
        )
        .add_systems(
            Update,
            sync_zoom_from_camera.run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            OnEnter(AppState::Loading),
            (reset_pause_menu_for_session, reset_lodge_placement_on_session),
        )
        .add_systems(
            OnEnter(AppState::MainMenu),
            reset_lodge_placement_on_session,
        )
        .add_systems(
            OnEnter(AppState::InGame),
            (
                reset_pause_menu_for_session,
                reset_lodge_placement_on_session,
                restore_hud_ornament_colors,
                frame_camera_to_map,
                reset_hover_highlight_on_enter,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                track_hover,
                animate_hover_highlight,
                update_examiner_panel,
                handle_examiner_click,
                animate_selection_highlight,
                handle_selection,
                move_selected_unit,
                handle_menu_escape,
                handle_resume_button,
                handle_open_settings_button,
                on_menu_screen_changed,
                handle_quit_to_main_menu_button,
                style_menu_buttons,
                (handle_toggle_button, sync_grid_for_zoom).chain(),
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            Update,
            (
                fps_update,
                end_turn,
                reroll_world,
                save_game,
                load_game,
                animate_lodge_blueprint,
                handle_hunting_lodge_button,
                sync_hunting_lodge_toolbar,
                handle_lodge_rotate,
                handle_lodge_placement_click,
                cancel_lodge_placement_on_escape,
                tick_lodge_placement_suppress,
                animate_lodge_place_pop,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            PostUpdate,
            (
                update_menu_motion,
                sync_open_pause_menu_fade,
                sync_pause_world_dim,
                debug_log_hud_ornament_fade,
                sync_pancam.after(update_menu_motion),
            )
                .run_if(in_state(AppState::InGame))
                // Scale must be set before layout; after layout causes end-of-close stutter.
                .before(UiSystem::Layout)
                .before(TransformSystem::TransformPropagate),
        )
        .add_systems(
            PostUpdate,
            sync_pancam
                .run_if(not(in_state(AppState::InGame))),
        )
        .run();
}

// ── Resources ───────────────────────────────────────────────────

fn is_under_hud(entity: Entity, hud_roots: &std::collections::HashSet<Entity>, parents: &Query<&Parent>) -> bool {
    let mut current = Some(entity);
    while let Some(e) = current {
        if hud_roots.contains(&e) {
            return true;
        }
        current = parents.get(e).ok().map(|p| p.get());
    }
    false
}

/// #region agent log
fn debug_log_hud_ornament_fade(
    screen: Res<MenuScreen>,
    hud: Query<Entity, With<InGameHud>>,
    parents: Query<&Parent>,
    ornaments: Query<(Entity, &BackgroundColor), (With<MenuFadeLayer>, Without<Text>)>,
) {
    if !screen.is_changed() {
        return;
    }
    let hud_roots: std::collections::HashSet<Entity> = hud.iter().collect();
    let mut hud_ornament_count = 0u32;
    let mut hud_ornament_invisible = 0u32;
    let mut sample_alphas: Vec<f32> = Vec::new();
    for (entity, bg) in &ornaments {
        if !is_under_hud(entity, &hud_roots, &parents) {
            continue;
        }
        hud_ornament_count += 1;
        let a = bg.0.alpha();
        if sample_alphas.len() < 4 {
            sample_alphas.push(a);
        }
        if a < 0.05 {
            hud_ornament_invisible += 1;
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-a40c64.log")
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let screen_name = format!("{:?}", *screen);
        let alphas = sample_alphas
            .iter()
            .map(|a| format!("{a:.3}"))
            .collect::<Vec<_>>()
            .join(",");
        let _ = writeln!(
            f,
            r#"{{"sessionId":"a40c64","runId":"post-fix","hypothesisId":"H3","location":"main.rs:debug_log_hud_ornament_fade","message":"hud ornament alpha on menu screen change","data":{{"screen":"{screen_name}","hud_ornament_count":{hud_ornament_count},"hud_ornament_invisible":{hud_ornament_invisible},"sample_alphas":"[{alphas}]"}},"timestamp":{ts}}}"#
        );
    }
}
/// #endregion

fn restore_hud_ornament_colors(
    mut commands: Commands,
    hud: Query<Entity, With<InGameHud>>,
    parents: Query<&Parent>,
    mut bg_ornaments: Query<
        (Entity, &MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    mut text_ornaments: Query<
        (Entity, &MenuFadeLayer, &mut TextColor),
        (With<Text>, With<MenuFadeLayer>),
    >,
) {
    let hud_roots: std::collections::HashSet<Entity> = hud.iter().collect();
    let mut restored = 0u32;
    for (entity, layer, mut bg) in &mut bg_ornaments {
        if !is_under_hud(entity, &hud_roots, &parents) {
            continue;
        }
        bg.0 = layer.base;
        commands.entity(entity).remove::<MenuFadeLayer>();
        restored += 1;
    }
    for (entity, layer, mut text) in &mut text_ornaments {
        if !is_under_hud(entity, &hud_roots, &parents) {
            continue;
        }
        text.0 = layer.base;
        commands.entity(entity).remove::<MenuFadeLayer>();
        restored += 1;
    }
    // #region agent log
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-a40c64.log")
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(
            f,
            r#"{{"sessionId":"a40c64","runId":"post-fix","hypothesisId":"H3","location":"main.rs:restore_hud_ornament_colors","message":"restored hud ornaments","data":{{"restored":{restored}}},"timestamp":{ts}}}"#
        );
    }
    // #endregion
}

#[derive(Resource, Default)]
struct SelectedHex(Option<HexCoord>);

#[derive(Resource)]
struct GameMap(Map);

#[derive(Resource, Default)]
struct CurrentSeed(u64);

#[derive(Resource, Default)]
struct HoveredHex(Option<HexCoord>);

#[derive(Resource, Default, PartialEq, Eq)]
enum BuildingPlacementMode {
    #[default]
    Idle,
    PlacingHuntingLodge,
}

impl BuildingPlacementMode {
    fn is_placing_lodge(&self) -> bool {
        matches!(self, BuildingPlacementMode::PlacingHuntingLodge)
    }
}

#[derive(Resource, Default)]
struct LodgePlacementRotation(u8);

#[derive(Resource, Default)]
struct LodgePlacementSuppressClick {
    frames: u8,
}

#[derive(Resource, Default)]
struct LodgeBlueprintMeshCache {
    rotation: Option<u8>,
    fill: Option<Handle<Mesh>>,
    outline: Option<Handle<Mesh>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExaminerFocus {
    Tile(HexCoord),
    Building(usize),
}

#[derive(Resource, Default)]
struct ExaminerSelection(Option<ExaminerFocus>);

#[derive(Resource, Default)]
struct SelectionBuildingMeshCache {
    rotation: Option<u8>,
    fill: Option<Handle<Mesh>>,
    outline: Option<Handle<Mesh>>,
}

#[derive(Component)]
struct HuntingLodgeButton;

#[derive(Component)]
struct HuntingLodgeButtonLabel;

#[derive(Component)]
struct LodgeBlueprint;

#[derive(Component)]
struct LodgeBlueprintFill;

#[derive(Component)]
struct LodgeBlueprintOutline;

#[derive(Component)]
struct LodgeTile;

#[derive(Component)]
struct LodgePlacePop {
    elapsed: f32,
}

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
struct GridMaterial(Handle<ColorMaterial>);

#[derive(Resource, Default)]
struct GridVisible(bool);

#[derive(Resource, Default)]
struct MenuScreenEpoch(u32);

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
enum MenuScreen {
    #[default]
    Closed,
    Main,
    Settings,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum MenuMotionPhase {
    #[default]
    Idle,
    Enter,
    Exit,
    /// One frame after exit animation — defers pan re-enable.
    Teardown,
    Switch,
}

#[derive(Resource, Default)]
struct MenuMotion {
    phase: MenuMotionPhase,
    timer: f32,
    duration: f32,
    /// Panel fading out during Exit.
    exit_panel: MenuScreen,
    /// Panel easing in during Enter / Switch.
    intro_panel: MenuScreen,
}

#[derive(Resource)]
struct Zoom(f32);

impl Default for Zoom {
    fn default() -> Self { Zoom(1.0) }
}

#[derive(Component)]
struct PauseWorldDim;

#[derive(Resource)]
struct PauseDimMaterial(Handle<ColorMaterial>);

const PAUSE_DIM_Z: f32 = 3.0;
/// Gold map perimeter — above internal grid, below hover highlight.
const MAP_BORDER_Z: f32 = 4.7;

#[derive(Component)]
struct PauseMenuLayer;

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct PauseHomePanel;

#[derive(Component)]
struct SettingsMenuPanel;

#[derive(Component)]
struct OpenSettingsButton;

#[derive(Component)]
struct ResumeButton;

#[derive(Component)]
struct QuitToMainMenuButton;

#[derive(Resource, Default)]
struct InGameUiReady(bool);

#[derive(Component)]
pub struct GridToggle;

#[derive(Component)]
pub struct GridToggleLabel;

#[derive(Component)]
struct GridMarker;

#[derive(Component)]
struct MapBorder;

struct MapBorderEdge {
    a: Vec2,
    b: Vec2,
    outward: Vec2,
}

#[derive(Component)]
struct TurnText;

#[derive(Component)]
struct SeedText;

#[derive(Component)]
struct WorldEntity;

#[derive(Component)]
struct HoverPanel;

#[derive(Component)]
struct HoverNameText;

#[derive(Component)]
struct HoverCategoryText;

#[derive(Component)]
struct HoverCoordsQ;

#[derive(Component)]
struct HoverCoordsR;

#[derive(Component)]
struct HoverEmptyHint;

#[derive(Component)]
struct ExaminerBuildingBlock;

#[derive(Component)]
struct ExaminerBuildingDivider;

#[derive(Component)]
struct ExaminerCoordsDivider;

#[derive(Component)]
struct ExaminerBuildingNameText;

#[derive(Component)]
struct ExaminerBuildingFoodText;

#[derive(Component)]
struct SelectionTileRing;

#[derive(Component)]
struct SelectionBuildingRoot;

#[derive(Component)]
struct SelectionBuildingFill;

#[derive(Component)]
struct SelectionBuildingOutline;

#[derive(Component)]
struct UnitMarker {
    civ_idx: usize,
    unit_idx: usize,
}

#[derive(Resource, Default)]
struct SavePath(Option<String>);

// ── Startup ─────────────────────────────────────────────────────

fn setup_ui_theme(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(UiTheme::load(&asset_server));
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Right],
            zoom_to_cursor: true,
            min_scale: 0.02,
            max_scale: 60.0,
            ..default()
        },
    ));
}

fn sync_pancam(
    app_state: Res<State<AppState>>,
    screen: Res<MenuScreen>,
    motion: Res<MenuMotion>,
    mut cameras: Query<&mut PanCam>,
) {
    if let Ok(mut pan) = cameras.get_single_mut() {
        let pause_visible = *screen != MenuScreen::Closed
            || motion.phase == MenuMotionPhase::Exit
            || motion.phase == MenuMotionPhase::Teardown;
        let in_game = *app_state.get() == AppState::InGame && !pause_visible;
        pan.enabled = in_game;
        pan.grab_buttons = vec![MouseButton::Right];
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

/// Alpha for the hex grid from camera scale (`projection.scale` ≈ world-units per pixel).
fn grid_alpha_for_scale(scale: f32) -> f32 {
    if scale <= 0.0 || !scale.is_finite() {
        return 0.0;
    }
    let hex_px = (2.0 * HEX_SIZE) / scale;
    if hex_px <= GRID_HEX_PX_FADE_END {
        return 0.0;
    }
    if hex_px >= GRID_HEX_PX_FADE_START {
        return 1.0;
    }
    let t = (hex_px - GRID_HEX_PX_FADE_END) / (GRID_HEX_PX_FADE_START - GRID_HEX_PX_FADE_END);
    t * t * (3.0 - 2.0 * t)
}

fn sync_grid_for_zoom(
    zoom: Res<Zoom>,
    grid_visible: Res<GridVisible>,
    grid_material: Option<Res<GridMaterial>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut grid_query: Query<&mut Visibility, With<GridMarker>>,
) {
    let Some(grid_mat) = grid_material else {
        return;
    };
    let alpha = if grid_visible.0 {
        grid_alpha_for_scale(zoom.0)
    } else {
        0.0
    };
    if let Some(mat) = materials.get_mut(&grid_mat.0) {
        mat.color = Color::srgba(0.0, 0.0, 0.0, alpha);
    }
    if let Ok(mut vis) = grid_query.get_single_mut() {
        *vis = if alpha > 0.001 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn map_center_tile(map: &Map) -> HexCoord {
    let center = HexCoord::origin();
    if map.tile_at(center).is_some() {
        return center;
    }
    map.tiles
        .iter()
        .min_by_key(|t| t.coord.distance(&center))
        .map(|t| t.coord)
        .unwrap_or(center)
}

fn default_examiner_focus(map: &Map, placed: &PlacedLodges) -> ExaminerFocus {
    let coord = map_center_tile(map);
    placed
        .lodge_index_at(coord)
        .map(ExaminerFocus::Building)
        .unwrap_or(ExaminerFocus::Tile(coord))
}

fn reset_lodge_placement_on_session(
    mut mode: ResMut<BuildingPlacementMode>,
    mut rotation: ResMut<LodgePlacementRotation>,
    mut placed: ResMut<PlacedLodges>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
    mut suppress: ResMut<LodgePlacementSuppressClick>,
    mut examiner: ResMut<ExaminerSelection>,
    state: Res<State<AppState>>,
    map: Option<Res<GameMap>>,
) {
    *mode = BuildingPlacementMode::Idle;
    rotation.0 = 0;
    *placed = PlacedLodges::default();
    mesh_cache.rotation = None;
    suppress.frames = 0;
    examiner.0 = match *state.get() {
        AppState::InGame => map
            .as_deref()
            .map(|m| default_examiner_focus(&m.0, &placed)),
        _ => None,
    };
}

fn reset_hover_highlight_on_enter(mut highlight: Query<&mut Visibility, With<Highlight>>) {
    if let Ok(mut visibility) = highlight.get_single_mut() {
        *visibility = Visibility::Hidden;
    }
}

fn frame_camera_to_map(
    game_map: Res<GameMap>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut OrthographicProjection), With<Camera2d>>,
    mut zoom: ResMut<Zoom>,
) {
    let Ok(window) = window.get_single() else {
        return;
    };
    let window_size = Vec2::new(window.width(), window.height());
    if window_size.x <= 0.0 || window_size.y <= 0.0 {
        return;
    }
    let Ok((mut transform, mut projection)) = cameras.get_single_mut() else {
        return;
    };
    apply_camera_frame_to_tiles(
        &game_map.0.tiles,
        window_size,
        &mut transform,
        &mut projection,
        &mut zoom.0,
    );
}

fn apply_camera_frame_to_tiles(
    tiles: &[HexTile],
    window_size: Vec2,
    transform: &mut Transform,
    projection: &mut OrthographicProjection,
    zoom: &mut f32,
) {
    let coords = tiles.iter().map(|t| t.coord);
    let (min_x, min_y, max_x, max_y) = hex_world_bounds(coords, HEX_SIZE);
    if !min_x.is_finite() {
        return;
    }

    transform.translation.x = (min_x + max_x) * 0.5;
    transform.translation.y = (min_y + max_y) * 0.5;

    let map_w = max_x - min_x;
    let map_h = max_y - min_y;
    let scale = (map_w / window_size.x)
        .max(map_h / window_size.y)
        * MAP_CAMERA_PADDING;
    projection.scale = scale.clamp(0.02, 60.0);
    *zoom = projection.scale;
}

fn init_save_path(mut save_path: ResMut<SavePath>) {
    save_path.0 = Some(default_save_path());
}

pub fn default_save_path() -> String {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{base}\\Myari\\save.json")
}

fn sync_app_screens(
    app_state: Res<State<AppState>>,
    mut title: Query<&mut Node, With<TitleRoot>>,
    mut loading: Query<&mut Node, (With<LoadingRoot>, Without<TitleRoot>)>,
    mut pause: Query<
        &mut Node,
        (
            With<PauseMenuLayer>,
            Without<TitleRoot>,
            Without<LoadingRoot>,
        ),
    >,
    mut hud: Query<&mut Visibility, (With<InGameHud>, Without<Highlight>)>,
) {
    let state = app_state.get();
    if app_state.is_changed() {
        if let Ok(mut node) = title.get_single_mut() {
            node.display = if *state == AppState::MainMenu {
                Display::Flex
            } else {
                Display::None
            };
        }
        if let Ok(mut node) = loading.get_single_mut() {
            node.display = if *state == AppState::Loading {
                Display::Flex
            } else {
                Display::None
            };
        }
        for mut node in &mut pause {
            if *state == AppState::InGame {
                // Keep in the layout tree while in-game; open/close uses Visibility to avoid reflow hitches.
                node.display = Display::Flex;
            } else {
                node.display = Display::None;
            }
        }
    }

    // Every frame — HUD may be spawned mid-load before the next state transition.
    let hud_vis = if *state == AppState::InGame {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut hud {
        *vis = hud_vis;
    }
}

fn sync_world_visibility(
    app_state: Res<State<AppState>>,
    mut map_meshes: Query<&mut Visibility, (With<WorldEntity>, Without<GridMarker>)>,
) {
    let show = *app_state.get() == AppState::InGame;
    for mut vis in &mut map_meshes {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn loading_pipeline(
    time: Res<Time>,
    job: Res<LoadingJob>,
    mut progress: ResMut<LoadingProgress>,
    mut next_state: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut seed_res: ResMut<CurrentSeed>,
    theme: Res<UiTheme>,
    mut ui_ready: ResMut<InGameUiReady>,
    game_state: Option<Res<GameState>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut OrthographicProjection), With<Camera2d>>,
    mut zoom: ResMut<Zoom>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<TurnText>>,
        Query<&mut Text, With<SeedText>>,
    )>,
) {
    progress.timer += time.delta_secs();
    progress.phase_timer += time.delta_secs();

    match progress.step {
        0 => {
            let t = load_phase_t(progress.phase_timer, LOAD_INTRO_SECS);
            progress.bar_cap = 0.05 + t * 0.20;
            if progress.phase_timer >= LOAD_INTRO_SECS {
                progress.step = 1;
                progress.phase_timer = 0.0;
            }
        }
        1 => {
            if !progress.work_done {
                if *job == LoadingJob::None {
                    return;
                }

                let (map, gs, seed) = match *job {
                    LoadingJob::NewGame => {
                        let seed = rand::thread_rng().gen::<u64>();
                        let map = Map::generate(MAP_RADIUS, seed);
                        let gs = GameState::new(&map);
                        (map, gs, seed)
                    }
                    LoadingJob::Continue => match load_saved_game() {
                        Ok(data) => {
                            let map = Map::from_tiles(data.tiles);
                            (map, data.game_state, data.seed)
                        }
                        Err(_) => {
                            progress.status = "Failed to load save".to_string();
                            progress.step = 3;
                            progress.phase_timer = 0.0;
                            progress.bar_cap = 1.0;
                            return;
                        }
                    },
                    LoadingJob::None => return,
                };

                seed_res.0 = seed;
                spawn_world_visuals(&mut commands, &mut meshes, &mut materials, &map, &gs);
                if let Ok(window) = window.get_single() {
                    let window_size = Vec2::new(window.width(), window.height());
                    if window_size.x > 0.0 && window_size.y > 0.0 {
                        if let Ok((mut transform, mut projection)) = cameras.get_single_mut() {
                            apply_camera_frame_to_tiles(
                                &map.tiles,
                                window_size,
                                &mut transform,
                                &mut projection,
                                &mut zoom.0,
                            );
                        }
                    }
                }
                commands.insert_resource(GameMap(map));
                commands.insert_resource(gs);
                progress.work_done = true;
            }

            let t = load_phase_t(progress.phase_timer, LOAD_WORLD_SECS);
            progress.bar_cap = 0.25 + t * 0.45;
            if progress.work_done && progress.phase_timer >= LOAD_WORLD_SECS {
                progress.step = 2;
                progress.phase_timer = 0.0;
            }
        }
        2 => {
            if !progress.ui_done {
                if !ui_ready.0 {
                    fps_startup(&mut commands, &theme);
                    spawn_game_hud(&mut commands, &theme, seed_res.0);
                    spawn_hover_panel(&mut commands, &theme, &mut images);
                    spawn_building_toolbar(&mut commands, &theme);
                    let hm = meshes.add(make_hex_ring_mesh(HEX_SIZE, HOVER_OUTLINE_STROKE));
                    commands.spawn((
                        Mesh2d(hm),
                        MeshMaterial2d(materials.add(ColorMaterial::from_color(GOLD))),
                        Transform::from_xyz(0.0, 0.0, 5.0),
                        Visibility::Hidden,
                        Highlight,
                    ));
                    let (bp_fill, bp_outline) = lodge_meshes_for_rotation(&mut meshes, 0);
                    let blueprint_fill_mat =
                        materials.add(ColorMaterial::from_color(LODGE_BLUEPRINT_FILL));
                    let blueprint_stroke_mat =
                        materials.add(ColorMaterial::from_color(LODGE_BLUEPRINT_STROKE_VALID));
                    commands
                        .spawn((Transform::default(), Visibility::Hidden, LodgeBlueprint))
                        .with_children(|root| {
                            root.spawn((
                                Mesh2d(bp_fill),
                                MeshMaterial2d(blueprint_fill_mat),
                                Transform::from_xyz(0.0, 0.0, 4.8),
                                LodgeBlueprintFill,
                            ));
                            root.spawn((
                                Mesh2d(bp_outline),
                                MeshMaterial2d(blueprint_stroke_mat),
                                Transform::from_xyz(0.0, 0.0, 5.0),
                                LodgeBlueprintOutline,
                            ));
                        });
                    commands.insert_resource(LodgeBlueprintMeshCache {
                        rotation: Some(0),
                        fill: None,
                        outline: None,
                    });
                    let sel_mat = materials.add(ColorMaterial::from_color(GOLD));
                    let sel_ring = meshes.add(make_hex_ring_mesh(HEX_SIZE, HOVER_OUTLINE_STROKE));
                    commands.spawn((
                        Mesh2d(sel_ring),
                        MeshMaterial2d(sel_mat.clone()),
                        Transform::from_xyz(0.0, 0.0, 5.2),
                        Visibility::Hidden,
                        SelectionTileRing,
                    ));
                    let (sel_fill, sel_outline) = lodge_selection_meshes_for_rotation(&mut meshes, 0);
                    let sel_fill_mat =
                        materials.add(ColorMaterial::from_color(LODGE_SELECTION_FILL));
                    commands
                        .spawn((
                            Transform::default(),
                            Visibility::Hidden,
                            SelectionBuildingRoot,
                        ))
                        .with_children(|root| {
                            root.spawn((
                                Mesh2d(sel_fill),
                                MeshMaterial2d(sel_fill_mat),
                                Transform::from_xyz(0.0, 0.0, 0.0),
                                SelectionBuildingFill,
                            ));
                            root.spawn((
                                Mesh2d(sel_outline),
                                MeshMaterial2d(sel_mat),
                                Transform::from_xyz(0.0, 0.0, 0.05),
                                SelectionBuildingOutline,
                            ));
                        });
                    commands.insert_resource(SelectionBuildingMeshCache {
                        rotation: Some(0),
                        fill: None,
                        outline: None,
                    });
                    ui_ready.0 = true;
                }

                if let Some(gs) = game_state.as_ref() {
                    if let Ok(mut text) = text_queries.p0().get_single_mut() {
                        text.0 = gs.turn.to_string();
                    }
                }
                if let Ok(mut text) = text_queries.p1().get_single_mut() {
                    text.0 = seed_res.0.to_string();
                }

                progress.ui_done = true;
            }

            let t = load_phase_t(progress.phase_timer, LOAD_UI_SECS);
            progress.bar_cap = 0.70 + t * 0.25;
            if progress.ui_done && progress.phase_timer >= LOAD_UI_SECS {
                progress.step = 3;
                progress.phase_timer = 0.0;
            }
        }
        3 => {
            let tail = (MIN_LOAD_SECS
                - LOAD_INTRO_SECS
                - LOAD_WORLD_SECS
                - LOAD_UI_SECS)
                .max(0.2);
            let t = load_phase_t(progress.phase_timer, tail);
            progress.bar_cap = 0.95 + t * 0.05;
            if progress.timer >= MIN_LOAD_SECS {
                next_state.set(AppState::InGame);
            }
        }
        _ => {}
    }
    if progress.status != "Failed to load save" {
        progress.status = "Preparing…".to_string();
    }
}

fn load_phase_t(elapsed: f32, duration: f32) -> f32 {
    if duration <= 0.0 {
        1.0
    } else {
        (elapsed / duration).clamp(0.0, 1.0)
    }
}

fn load_saved_game() -> Result<SaveData, ()> {
    let path = default_save_path();
    let text = std::fs::read_to_string(&path).map_err(|_| ())?;
    serde_json::from_str(&text).map_err(|_| ())
}

fn spawn_building_toolbar(commands: &mut Commands, theme: &UiTheme) {
    commands
        .spawn((
            HudAnchor::TopCenter { top: 18.0 }.outer_node(),
            Visibility::Hidden,
            GlobalZIndex(GLOBAL_Z_HUD),
            InGameHud,
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    width: Val::Px(220.0),
                    ..default()
                },
                BlocksWorldInput,
                FocusPolicy::Block,
                Interaction::None,
            ))
            .with_children(|wrap| {
                wrap.spawn((
                    menu_button_row_bundle(),
                    MenuButton,
                    HuntingLodgeButton,
                ))
                .with_children(|btn| {
                    spawn_menu_button_label(btn, theme, "Hunting Lodge", HuntingLodgeButtonLabel);
                });
            });
        });
}

fn spawn_game_hud(commands: &mut Commands, theme: &UiTheme, seed: u64) {
    spawn_framed_panel(
        commands,
        theme,
        HudAnchor::TopRight {
            right: 18.0,
            top: 18.0,
        },
        248.0,
        UiRect::new(Val::Px(16.0), Val::Px(14.0), Val::Px(14.0), Val::Px(16.0)),
        8.0,
        |panel, theme| {
            panel.spawn(theme.label("TURN"));
            panel.spawn((theme.value("1", 22.0), TurnText));
            panel.spawn(theme.hint("Space — next turn", 11.0));
            spawn_ornate_divider(panel, theme, false);
            panel.spawn(theme.label("SEED"));
            panel.spawn((theme.value(seed.to_string(), 15.0), SeedText));
            panel.spawn(theme.hint("Press \\ to reroll world", 11.0));
        },
    );
}

fn spawn_hover_panel(commands: &mut Commands, theme: &UiTheme, images: &mut Assets<Image>) {
    spawn_framed_panel(
        commands,
        theme,
        HudAnchor::BottomLeft {
            left: 18.0,
            bottom: 18.0,
        },
        268.0,
        UiRect::new(Val::Px(18.0), Val::Px(16.0), Val::Px(16.0), Val::Px(18.0)),
        12.0,
        |panel, theme| {
            panel.spawn((theme.label("EXAMINER"), HoverPanel));

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(14.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ))
                .with_children(|row| {
                    spawn_hover_gem_swatch(row, images);
                    row.spawn((theme.value("—", 26.0), HoverNameText));
                });

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        ..default()
                    },
                ))
                .with_children(|block| {
                    block.spawn(theme.label("TYPE"));
                    block.spawn((theme.value("—", 19.0), HoverCategoryText));
                });

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        display: Display::None,
                        ..default()
                    },
                    Visibility::Hidden,
                    ExaminerBuildingDivider,
                ))
                .with_children(|wrap| {
                    spawn_ornate_divider(wrap, theme, false);
                });

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        display: Display::None,
                        ..default()
                    },
                    Visibility::Hidden,
                    ExaminerBuildingBlock,
                ))
                .with_children(|block| {
                    block.spawn(theme.label("BUILDING"));
                    block.spawn((theme.value("—", 22.0), ExaminerBuildingNameText));
                    block.spawn(theme.label("FOOD STORED"));
                    block.spawn((theme.value("—", 19.0), ExaminerBuildingFoodText));
                });

            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    ..default()
                }, ExaminerCoordsDivider))
                .with_children(|wrap| {
                    spawn_ornate_divider(wrap, theme, false);
                });

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        ..default()
                    },
                ))
                .with_children(|block| {
                    block.spawn(theme.label("COORDINATES"));
                    block
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                ..default()
                            },
                        ))
                        .with_children(|row| {
                            row.spawn((theme.value("q —", 16.0), HoverCoordsQ));
                            row.spawn(theme.star(13.0));
                            row.spawn((theme.value("r —", 16.0), HoverCoordsR));
                        });
                });

            panel.spawn((
                theme.hint("Click a tile or building", 11.0),
                HoverEmptyHint,
            ));

            panel
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(10.0),
                        bottom: Val::Px(8.0),
                        ..default()
                    },
                ))
                .with_children(|mark| {
                    spawn_star_watermark(mark, theme);
                });
        },
    );
}

fn format_hover_coord_half(axis: &str, value: Option<i32>) -> String {
    let vs = value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());
    format!("{axis} {vs}")
}

fn terrain_swatch_color(t: TerrainType) -> Color {
    let c = terrain_to_color(t).to_srgba();
    Color::srgb(
        (c.red * 1.12 + 0.06).min(1.0),
        (c.green * 1.12 + 0.06).min(1.0),
        (c.blue * 1.12 + 0.06).min(1.0),
    )
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
    mut zoom: ResMut<Zoom>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut OrthographicProjection), With<Camera2d>>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<TurnText>>,
        Query<&mut Text, With<SeedText>>,
    )>,
    world_entities: Query<Entity, With<WorldEntity>>,
    mut placement_mode: ResMut<BuildingPlacementMode>,
    mut placed_lodges: ResMut<PlacedLodges>,
) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    let Ok(data) = load_saved_game() else {
        println!("No save found or failed to parse save");
        return;
    };

    for e in &world_entities {
        commands.entity(e).despawn_recursive();
    }

    *placement_mode = BuildingPlacementMode::Idle;
    *placed_lodges = PlacedLodges::default();

    seed_res.0 = data.seed;
    let map = Map::from_tiles(data.tiles);
    *gs = data.game_state;
    spawn_world_visuals(&mut commands, &mut meshes, &mut materials, &map, &gs);
    game_map.0 = map;

    if let Ok(window) = window.get_single() {
        let window_size = Vec2::new(window.width(), window.height());
        if window_size.x > 0.0 && window_size.y > 0.0 {
            if let Ok((mut transform, mut projection)) = cameras.get_single_mut() {
                apply_camera_frame_to_tiles(
                    &game_map.0.tiles,
                    window_size,
                    &mut transform,
                    &mut projection,
                    &mut zoom.0,
                );
            }
        }
    }

    if let Ok(mut text) = text_queries.p0().get_single_mut() {
        text.0 = gs.turn.to_string();
    }
    if let Ok(mut text) = text_queries.p1().get_single_mut() {
        text.0 = data.seed.to_string();
    }
    println!("Loaded game from: {}", default_save_path());
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
        Visibility::Hidden,
        WorldEntity,
    ));

    let grid_mesh = meshes.add(make_grid_mesh(&map.tiles, HEX_SIZE));
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

    let dim_mat = spawn_pause_world_dim(commands, meshes, materials, &map.tiles);
    commands.insert_resource(PauseDimMaterial(dim_mat));

    let border_edges = build_map_border_path(&map.tiles, HEX_SIZE);
    let border_mesh = meshes.add(make_map_border_stroke_from_edges(
        &border_edges,
        MAP_BORDER_STROKE,
    ));
    commands.spawn((
        Mesh2d(border_mesh),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(GOLD))),
        Transform::from_xyz(0.0, 0.0, MAP_BORDER_Z),
        Visibility::Hidden,
        MapBorder,
        WorldEntity,
    ));

    // Civilization city/unit markers temporarily disabled — game state still
    // tracks them, but we don't draw the red/blue/green dots yet.
    // spawn_civ_markers(commands, meshes, materials, gs);
    let _ = (commands, meshes, materials, gs);
}

fn spawn_pause_world_dim(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    tiles: &[HexTile],
) -> Handle<ColorMaterial> {
    let (min_x, min_y, max_x, max_y) =
        hex_world_bounds(tiles.iter().map(|t| t.coord), HEX_SIZE);
    let pad = HEX_SIZE * 2.0;
    let w = max_x - min_x + pad;
    let h = max_y - min_y + pad;
    let mat = materials.add(ColorMaterial::from_color(Color::srgba(
        0.02,
        0.02,
        0.03,
        0.0,
    )));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(w, h))),
        MeshMaterial2d(mat.clone()),
        Transform::from_xyz((min_x + max_x) * 0.5, (min_y + max_y) * 0.5, PAUSE_DIM_Z),
        Visibility::Hidden,
        PauseWorldDim,
        WorldEntity,
    ));
    mat
}

fn apply_pause_world_dim_alpha(
    alpha: f32,
    materials: &mut Assets<ColorMaterial>,
    handle: &Handle<ColorMaterial>,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if let Some(mat) = materials.get_mut(handle) {
        mat.color = Color::srgba(
            0.02,
            0.02,
            0.03,
            PAUSE_WORLD_DIM_ALPHA * alpha,
        );
    }
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

fn border_vertex_key(v: Vec2) -> (i32, i32) {
    ((v.x * 16.0).round() as i32, (v.y * 16.0).round() as i32)
}

fn dedupe_outwards(outwards: &[Vec2]) -> Vec<Vec2> {
    let mut unique: Vec<Vec2> = Vec::new();
    for &o in outwards {
        let o = o.normalize_or_zero();
        if o.length_squared() < 1e-8 {
            continue;
        }
        if unique.iter().any(|u| u.dot(o) > 0.995) {
            continue;
        }
        unique.push(o);
    }
    unique
}

fn vertex_bevel_offset(outwards: &[Vec2], half: f32) -> (Vec2, Vec2) {
    let unique = dedupe_outwards(outwards);
    if unique.is_empty() {
        return (Vec2::ZERO, Vec2::ZERO);
    }
    if unique.len() == 1 {
        let o = unique[0] * half;
        return (o, -o);
    }
    let sum: Vec2 = unique.iter().copied().sum();
    let n = sum.normalize_or_zero();
    if n.length_squared() < 1e-8 {
        let o = unique[0] * half;
        return (o, -o);
    }
    (n * half, -n * half)
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

fn build_map_border_path(tiles: &[HexTile], hex_size: f32) -> Vec<MapBorderEdge> {
    let coords: HashSet<HexCoord> = tiles.iter().map(|t| t.coord).collect();
    let mut edges = Vec::new();
    for tile in tiles {
        let (cx, cy) = axial_to_pixel(tile.coord.q, tile.coord.r, hex_size);
        let center = Vec2::new(cx, cy);
        let corners = hex_corners_at(tile.coord.q, tile.coord.r, hex_size);
        let neighbors = tile.coord.neighbors();
        for i in 0..6 {
            if coords.contains(&neighbors[i]) {
                continue;
            }
            let a = Vec2::new(corners[i].0, corners[i].1);
            let b = Vec2::new(corners[(i + 1) % 6].0, corners[(i + 1) % 6].1);
            edges.push(MapBorderEdge {
                a,
                b,
                outward: exterior_edge_outward(center, a, b),
            });
        }
    }
    edges
}

fn append_stroke_quad(
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    o_a: Vec2,
    o_b: Vec2,
    i_b: Vec2,
    i_a: Vec2,
) {
    let base = positions.len() as u32;
    positions.extend([
        [o_a.x, o_a.y, 0.0],
        [o_b.x, o_b.y, 0.0],
        [i_b.x, i_b.y, 0.0],
        [i_a.x, i_a.y, 0.0],
    ]);
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn make_map_border_stroke_from_edges(edges: &[MapBorderEdge], stroke: f32) -> Mesh {
    let half = stroke * 0.5;
    let mut vertex_outwards: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
    for edge in edges {
        if edge.outward.length_squared() < 1e-8 {
            continue;
        }
        vertex_outwards
            .entry(border_vertex_key(edge.a))
            .or_default()
            .push(edge.outward);
        vertex_outwards
            .entry(border_vertex_key(edge.b))
            .or_default()
            .push(edge.outward);
    }

    let mut positions = Vec::with_capacity(edges.len() * 4);
    let mut indices = Vec::with_capacity(edges.len() * 6);
    for edge in edges {
        if edge.outward.length_squared() < 1e-8 {
            continue;
        }
        let (o_a, i_a) = vertex_bevel_offset(
            vertex_outwards
                .get(&border_vertex_key(edge.a))
                .map(|v| v.as_slice())
                .unwrap_or(&[]),
            half,
        );
        let (o_b, i_b) = vertex_bevel_offset(
            vertex_outwards
                .get(&border_vertex_key(edge.b))
                .map(|v| v.as_slice())
                .unwrap_or(&[]),
            half,
        );
        append_stroke_quad(
            &mut positions,
            &mut indices,
            edge.a + o_a,
            edge.b + o_b,
            edge.b + i_b,
            edge.a + i_a,
        );
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn build_footprint_border_edges(coords: &[HexCoord], hex_size: f32) -> Vec<MapBorderEdge> {
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
            edges.push(MapBorderEdge {
                a,
                b,
                outward: exterior_edge_outward(center, a, b),
            });
        }
    }
    edges
}

fn make_footprint_fill_mesh(
    coords: &[HexCoord],
    hex_size: f32,
    origin: Vec2,
    color: Color,
    margin: f32,
) -> Mesh {
    let c = color.to_linear().to_f32_array();
    let (positions, colors, indices) =
        footprint_mesh::make_footprint_fill_mesh(coords, hex_size, origin, c, margin);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    if !indices.is_empty() {
        mesh.insert_indices(Indices::U32(indices));
    }
    mesh
}

fn make_footprint_outline_mesh(coords: &[HexCoord], hex_size: f32, origin: Vec2, stroke: f32) -> Mesh {
    let edges = build_footprint_border_edges(coords, hex_size);
    let local_edges: Vec<MapBorderEdge> = edges
        .into_iter()
        .map(|e| MapBorderEdge {
            a: e.a - origin,
            b: e.b - origin,
            outward: e.outward,
        })
        .collect();
    make_map_border_stroke_from_edges(&local_edges, stroke)
}

fn lodge_outline_mesh_for_rotation(meshes: &mut Assets<Mesh>, rotation: u8) -> Handle<Mesh> {
    let coords = lodge_coords(HexCoord::origin(), rotation);
    let origin = footprint_mesh::footprint_centroid(&coords, HEX_SIZE);
    meshes.add(make_footprint_outline_mesh(
        &coords,
        HEX_SIZE,
        origin,
        HOVER_OUTLINE_STROKE,
    ))
}

fn lodge_footprint_fill_mesh_for_rotation(
    meshes: &mut Assets<Mesh>,
    rotation: u8,
    color: Color,
    margin: f32,
) -> Handle<Mesh> {
    let coords = lodge_coords(HexCoord::origin(), rotation);
    let origin = footprint_mesh::footprint_centroid(&coords, HEX_SIZE);
    meshes.add(make_footprint_fill_mesh(
        &coords,
        HEX_SIZE,
        origin,
        color,
        margin,
    ))
}

fn lodge_selection_meshes_for_rotation(
    meshes: &mut Assets<Mesh>,
    rotation: u8,
) -> (Handle<Mesh>, Handle<Mesh>) {
    let fill = lodge_footprint_fill_mesh_for_rotation(
        meshes,
        rotation,
        LODGE_SELECTION_FILL,
        LODGE_BUILDING_MARGIN,
    );
    let outline = lodge_outline_mesh_for_rotation(meshes, rotation);
    (fill, outline)
}

fn lodge_meshes_for_rotation(
    meshes: &mut Assets<Mesh>,
    rotation: u8,
) -> (Handle<Mesh>, Handle<Mesh>) {
    let fill = lodge_footprint_fill_mesh_for_rotation(
        meshes,
        rotation,
        LODGE_BLUEPRINT_FILL,
        LODGE_BUILDING_MARGIN,
    );
    let outline = lodge_outline_mesh_for_rotation(meshes, rotation);
    (fill, outline)
}

/// One continuous hex ring — inner/outer contours share vertices at each corner.
fn make_hex_ring_mesh(corner_radius: f32, stroke: f32) -> Mesh {
    let half = stroke * 0.5;
    let inner = hex_corners_local(corner_radius - half);
    let outer = hex_corners_local(corner_radius + half);
    let mut positions = Vec::with_capacity(12);
    for &(x, y) in &inner {
        positions.push([x, y, 0.0]);
    }
    for &(x, y) in &outer {
        positions.push([x, y, 0.0]);
    }
    let mut indices = Vec::with_capacity(36);
    for i in 0..6u32 {
        let next = (i + 1) % 6;
        let i_in = i;
        let o_in = i + 6;
        let o_out = next + 6;
        let i_out = next;
        indices.extend([i_in, o_in, o_out, i_in, o_out, i_out]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
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
        if aa <= bb {
            (aa, bb)
        } else {
            (bb, aa)
        }
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

fn pointer_over_hunting_lodge_button(
    buttons: Query<&Interaction, With<HuntingLodgeButton>>,
) -> bool {
    buttons
        .iter()
        .any(|i| matches!(*i, Interaction::Hovered | Interaction::Pressed))
}

fn pointer_over_blocked_ui(blockers: Query<&Interaction, With<BlocksWorldInput>>) -> bool {
    blockers
        .iter()
        .any(|i| matches!(*i, Interaction::Hovered | Interaction::Pressed))
}

fn pointer_over_ingame_ui(
    blockers: Query<&Interaction, With<BlocksWorldInput>>,
    lodge_button: Query<&Interaction, With<HuntingLodgeButton>>,
) -> bool {
    pointer_over_blocked_ui(blockers) || pointer_over_hunting_lodge_button(lodge_button)
}

fn hover_suppressed_for_selected_building(
    examiner: &ExaminerSelection,
    placed: &PlacedLodges,
    hex: HexCoord,
) -> bool {
    let Some(ExaminerFocus::Building(idx)) = examiner.0 else {
        return false;
    };
    let Some(lodge) = placed.lodges.get(idx) else {
        return false;
    };
    lodge_coords(lodge.anchor, lodge.rotation).contains(&hex)
}

fn track_hover(
    mode: Res<BuildingPlacementMode>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, With<Camera2d>>,
    mut hovered: ResMut<HoveredHex>,
    zoom: Res<Zoom>,
    blockers: Query<&Interaction, With<BlocksWorldInput>>,
    lodge_button: Query<&Interaction, With<HuntingLodgeButton>>,
    mut cursor_evr: EventReader<CursorMoved>,
) {
    if pointer_over_ingame_ui(blockers, lodge_button) {
        // While placing, keep the last map hex so the blueprint stays visible over the button.
        if mode.is_placing_lodge() {
            return;
        }
        hovered.0 = None;
        return;
    }
    let Some(ev) = cursor_evr.read().last() else { return; };
    let cam = camera.single();
    let ws = Vec2::new(window.single().width(), window.single().height());
    let world = screen_to_world(cam, ws, ev.position, zoom.0);
    hovered.0 = Some(pixel_to_hex(world.x, world.y, HEX_SIZE));
}

fn animate_hover_highlight(
    time: Res<Time>,
    mode: Res<BuildingPlacementMode>,
    hovered: Res<HoveredHex>,
    examiner: Res<ExaminerSelection>,
    placed: Res<PlacedLodges>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut query: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial2d<ColorMaterial>),
        With<Highlight>,
    >,
) {
    let Ok((mut transform, mut visibility, material)) = query.get_single_mut() else {
        return;
    };

    if mode.is_placing_lodge() {
        *visibility = Visibility::Hidden;
        transform.scale = Vec3::ONE;
        return;
    }

    let Some(hex) = hovered.0 else {
        *visibility = Visibility::Hidden;
        transform.scale = Vec3::ONE;
        if let Some(mat) = materials.get_mut(&material.0) {
            mat.color = GOLD;
        }
        return;
    };

    let (tx, ty) = axial_to_pixel(hex.q, hex.r, HEX_SIZE);
    let target = Vec2::new(tx, ty);
    let dt = time.delta_secs();
    let suppressed = hover_suppressed_for_selected_building(&examiner, &placed, hex);

    let current = transform.translation.truncate();
    let pos = smooth_follow_vec2_distance_speed(
        current,
        target,
        dt,
        HOVER_GLOW_SPEED_NEAR,
        HOVER_GLOW_SPEED_FAR,
        HEX_SIZE * 0.35,
        HEX_SIZE * 2.8,
    );
    transform.translation = Vec3::new(pos.x, pos.y, 5.0);

    if suppressed {
        let arrived = pos.distance_squared(target) < (HEX_SIZE * 0.12).powi(2);
        if arrived || *visibility == Visibility::Hidden {
            *visibility = Visibility::Hidden;
            transform.scale = Vec3::ONE;
            return;
        }
        // Still sliding onto a selected-building tile — keep visible until we arrive.
    } else if *visibility == Visibility::Hidden {
        transform.scale = Vec3::ONE;
    }

    let pulse = (time.elapsed_secs() * std::f32::consts::TAU * HOVER_GLOW_PULSE_HZ).sin();
    transform.scale = Vec3::splat(1.0 + pulse * HOVER_GLOW_PULSE_SCALE);
    if let Some(mat) = materials.get_mut(&material.0) {
        let alpha = HOVER_GLOW_ALPHA_BASE + pulse * HOVER_GLOW_ALPHA_AMP;
        mat.color = GOLD.with_alpha(alpha);
    }
    *visibility = Visibility::Visible;
}

// ── Hunting lodge placement ───────────────────────────────────────

fn handle_hunting_lodge_button(
    mut interaction: Query<&Interaction, (Changed<Interaction>, With<HuntingLodgeButton>)>,
    mut mode: ResMut<BuildingPlacementMode>,
    mut rotation: ResMut<LodgePlacementRotation>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
    mut suppress: ResMut<LodgePlacementSuppressClick>,
) {
    for interaction in &mut interaction {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let entering = !mode.is_placing_lodge();
        *mode = match *mode {
            BuildingPlacementMode::Idle => BuildingPlacementMode::PlacingHuntingLodge,
            BuildingPlacementMode::PlacingHuntingLodge => BuildingPlacementMode::Idle,
        };
        if entering && mode.is_placing_lodge() {
            rotation.0 = 0;
            mesh_cache.rotation = None;
            suppress.frames = 5;
        }
    }
}

fn tick_lodge_placement_suppress(mut suppress: ResMut<LodgePlacementSuppressClick>) {
    if suppress.frames > 0 {
        suppress.frames -= 1;
    }
}

fn sync_hunting_lodge_toolbar(
    mode: Res<BuildingPlacementMode>,
    mut label: Query<&mut Text, With<HuntingLodgeButtonLabel>>,
    mut fills: Query<(&mut MenuFadeLayer, &mut BackgroundColor), With<MenuButtonFill>>,
    children: Query<&Children, With<HuntingLodgeButton>>,
) {
    if !mode.is_changed() {
        return;
    }
    let placing = mode.is_placing_lodge();
    for mut text in &mut label {
        text.0 = if placing {
            "Hunting Lodge — R rotate".to_string()
        } else {
            "Hunting Lodge".to_string()
        };
    }
    let btn_color = if placing { BTN_PRESSED } else { BTN_IDLE };
    if let Ok(kids) = children.get_single() {
        for child in kids.iter() {
            if let Ok((mut layer, mut bg)) = fills.get_mut(*child) {
                layer.base = btn_color;
                bg.0 = btn_color;
            }
        }
    }
}

fn cancel_lodge_placement(
    mode: &mut BuildingPlacementMode,
    mesh_cache: &mut LodgeBlueprintMeshCache,
) {
    *mode = BuildingPlacementMode::Idle;
    mesh_cache.rotation = None;
}

fn pause_menu_open(screen: &MenuScreen) -> bool {
    *screen != MenuScreen::Closed
}

fn cancel_lodge_placement_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<MenuScreen>,
    mut mode: ResMut<BuildingPlacementMode>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
) {
    if pause_menu_open(&screen) || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if !mode.is_placing_lodge() {
        return;
    }
    cancel_lodge_placement(&mut mode, &mut mesh_cache);
}

fn handle_lodge_rotate(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<MenuScreen>,
    mode: Res<BuildingPlacementMode>,
    mut rotation: ResMut<LodgePlacementRotation>,
) {
    if pause_menu_open(&screen) || !mode.is_placing_lodge() || !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    rotation.0 = (rotation.0 + 1) % 6;
}

fn animate_lodge_blueprint(
    time: Res<Time>,
    mode: Res<BuildingPlacementMode>,
    hovered: Res<HoveredHex>,
    rotation: Res<LodgePlacementRotation>,
    map: Res<GameMap>,
    placed: Res<PlacedLodges>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut blueprint: Query<
        (&mut Transform, &mut Visibility),
        (With<LodgeBlueprint>, Without<LodgeBlueprintFill>),
    >,
    mut fill: Query<
        (&mut Mesh2d, &MeshMaterial2d<ColorMaterial>),
        (With<LodgeBlueprintFill>, Without<LodgeBlueprintOutline>),
    >,
    mut outline: Query<
        (&mut Mesh2d, &MeshMaterial2d<ColorMaterial>),
        (With<LodgeBlueprintOutline>, Without<LodgeBlueprintFill>),
    >,
) {
    let Ok((mut root_transform, mut root_visibility)) = blueprint.get_single_mut() else {
        return;
    };

    if !mode.is_placing_lodge() {
        *root_visibility = Visibility::Hidden;
        root_transform.scale = Vec3::ONE;
        return;
    }

    let Some(center) = hovered.0 else {
        *root_visibility = Visibility::Hidden;
        root_transform.scale = Vec3::ONE;
        return;
    };

    if mesh_cache.rotation != Some(rotation.0) {
        let (fill_h, outline_h) = lodge_meshes_for_rotation(&mut meshes, rotation.0);
        mesh_cache.rotation = Some(rotation.0);
        mesh_cache.fill = Some(fill_h.clone());
        mesh_cache.outline = Some(outline_h.clone());
        if let Ok((mut mesh2d, _)) = fill.get_single_mut() {
            mesh2d.0 = fill_h;
        }
        if let Ok((mut mesh2d, _)) = outline.get_single_mut() {
            mesh2d.0 = outline_h;
        }
    }

    let coords = lodge_coords(center, rotation.0);
    let valid = can_place_lodge(&map.0, &*placed, center, rotation.0);
    let centroid = footprint_mesh::footprint_centroid(&coords, HEX_SIZE);
    let target = Vec2::new(centroid.x, centroid.y);
    let dt = time.delta_secs();

    if *root_visibility == Visibility::Hidden {
        root_transform.translation = Vec3::new(target.x, target.y, 5.0);
        root_transform.scale = Vec3::ONE;
        *root_visibility = Visibility::Visible;
    } else {
        let current = root_transform.translation.truncate();
        let pos = smooth_follow_vec2_distance_speed(
            current,
            target,
            dt,
            HOVER_GLOW_SPEED_NEAR,
            HOVER_GLOW_SPEED_FAR,
            HEX_SIZE * 0.35,
            HEX_SIZE * 2.8,
        );
        root_transform.translation = Vec3::new(pos.x, pos.y, 5.0);
    }

    let pulse = (time.elapsed_secs() * std::f32::consts::TAU * HOVER_GLOW_PULSE_HZ).sin();
    root_transform.scale = Vec3::splat(1.0 + pulse * HOVER_GLOW_PULSE_SCALE);

    let stroke_color = if valid {
        LODGE_BLUEPRINT_STROKE_VALID
    } else {
        LODGE_BLUEPRINT_STROKE_INVALID
    };
    let stroke_alpha = HOVER_GLOW_ALPHA_BASE + pulse * HOVER_GLOW_ALPHA_AMP;
    if let Ok((_, mat)) = outline.get_single() {
        if let Some(m) = materials.get_mut(&mat.0) {
            m.color = stroke_color.with_alpha(stroke_alpha);
        }
    }
    if let Ok((_, mat)) = fill.get_single() {
        if let Some(m) = materials.get_mut(&mat.0) {
            let mut c = LODGE_BLUEPRINT_FILL;
            c = c.with_alpha(LODGE_BLUEPRINT_FILL.alpha() * (0.92 + pulse * 0.06));
            m.color = c;
        }
    }
    *root_visibility = Visibility::Visible;
}

fn handle_lodge_placement_click(
    mouse: Res<ButtonInput<MouseButton>>,
    screen: Res<MenuScreen>,
    suppress: Res<LodgePlacementSuppressClick>,
    blockers: Query<&Interaction, With<BlocksWorldInput>>,
    lodge_button: Query<&Interaction, With<HuntingLodgeButton>>,
    hovered: Res<HoveredHex>,
    rotation: Res<LodgePlacementRotation>,
    map: Res<GameMap>,
    mut placed: ResMut<PlacedLodges>,
    mut mode: ResMut<BuildingPlacementMode>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !mode.is_placing_lodge() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if pause_menu_open(&screen)
        || suppress.frames > 0
        || pointer_over_ingame_ui(blockers, lodge_button)
    {
        return;
    }
    let Some(center) = hovered.0 else {
        return;
    };
    if !can_place_lodge(&map.0, &*placed, center, rotation.0) {
        return;
    }
    spawn_lodge_building(
        &mut commands,
        &mut meshes,
        &mut materials,
        center,
        rotation.0,
    );
    placed.register_lodge(center, rotation.0);
    cancel_lodge_placement(&mut mode, &mut mesh_cache);
}

fn spawn_lodge_building(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    center: HexCoord,
    rotation: u8,
) {
    let coords = lodge_coords(center, rotation);
    let origin = footprint_mesh::footprint_centroid(&coords, HEX_SIZE);
    let fill_mesh = meshes.add(make_footprint_fill_mesh(
        &coords,
        HEX_SIZE,
        origin,
        LODGE_BUILDING_FILL,
        LODGE_BUILDING_MARGIN,
    ));
    let fill_mat = materials.add(ColorMaterial::from_color(LODGE_BUILDING_FILL));
    commands.spawn((
        Mesh2d(fill_mesh),
        MeshMaterial2d(fill_mat),
        Transform::from_xyz(origin.x, origin.y, 4.6),
        LodgeTile,
        LodgePlacePop { elapsed: 0.0 },
        WorldEntity,
    ));
}

fn lodge_place_pop_transform(t: f32) -> Transform {
    let t = t.clamp(0.0, 1.0);
    let eased = ease_out_cubic(t);
    let scale = LODGE_PLACE_START_SCALE + (1.0 - LODGE_PLACE_START_SCALE) * eased;
    Transform::from_scale(Vec3::splat(scale))
}

fn animate_lodge_place_pop(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut LodgePlacePop), With<LodgeTile>>,
) {
    let dt = time.delta_secs();
    for (mut transform, mut pop) in &mut query {
        pop.elapsed += dt;
        let t = (pop.elapsed / LODGE_PLACE_POP_SECS).min(1.0);
        transform.scale = lodge_place_pop_transform(t).scale;
    }
}

// ── Tile selection (right-click) ────────────────────────────────

fn handle_selection(
    mouse: Res<ButtonInput<MouseButton>>,
    blockers: Query<&Interaction, With<BlocksWorldInput>>,
    lodge_button: Query<&Interaction, With<HuntingLodgeButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, With<Camera2d>>,
    mut selected: ResMut<SelectedHex>,
    zoom: Res<Zoom>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    if pointer_over_ingame_ui(blockers, lodge_button) {
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

fn fps_startup(commands: &mut Commands, theme: &UiTheme) {
    spawn_framed_panel(
        commands,
        theme,
        HudAnchor::TopLeft {
            left: 18.0,
            top: 18.0,
        },
        132.0,
        UiRect::new(Val::Px(14.0), Val::Px(12.0), Val::Px(12.0), Val::Px(14.0)),
        6.0,
        |panel, theme| {
            panel.spawn(theme.label("STATUS"));
            panel.spawn((theme.value("—", 20.0), FpsText));
        },
    );
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
            text.0 = format!("{:.0} FPS", val);
        }
        fps.elapsed = 0.0;
        fps.frames = 0;
    }
}

// ── Pause menu (Esc, in-game only) ──────────────────────────────

fn reset_pause_menu_for_session(
    mut epoch: ResMut<MenuScreenEpoch>,
    mut screen: ResMut<MenuScreen>,
    mut motion: ResMut<MenuMotion>,
    mut menu_layers: Query<
        (&mut Node, &mut Visibility),
        (
            With<PauseMenuLayer>,
            Without<PauseHomePanel>,
            Without<SettingsMenuPanel>,
        ),
    >,
    mut main: Query<
        &mut Node,
        (
            With<PauseHomePanel>,
            Without<MenuRoot>,
            Without<SettingsMenuPanel>,
        ),
    >,
    mut settings: Query<
        &mut Node,
        (
            With<SettingsMenuPanel>,
            Without<MenuRoot>,
            Without<PauseHomePanel>,
        ),
    >,
    mut main_xform: Query<&mut Transform, (With<PauseHomePanel>, Without<SettingsMenuPanel>)>,
    mut settings_xform: Query<&mut Transform, (With<SettingsMenuPanel>, Without<PauseHomePanel>)>,
    mut fade_layers: Query<
        (&MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    mut fade_text: Query<
        (&MenuFadeLayer, &mut TextColor, &mut BackgroundColor),
        With<Text>,
    >,
) {
    epoch.0 += 1;
    *screen = MenuScreen::Closed;
    *motion = MenuMotion::default();
    for (mut node, mut vis) in &mut menu_layers {
        node.display = Display::None;
        *vis = Visibility::Hidden;
    }
    if let Ok(mut node) = main.get_single_mut() {
        node.display = Display::None;
    }
    if let Ok(mut node) = settings.get_single_mut() {
        node.display = Display::None;
    }
    if let Ok(mut xform) = main_xform.get_single_mut() {
        *xform = Transform::default();
    }
    if let Ok(mut xform) = settings_xform.get_single_mut() {
        *xform = Transform::default();
    }
    apply_menu_fade_alpha(0.0, &mut fade_layers, &mut fade_text);
}

fn spawn_pause_menu(mut commands: Commands, theme: Res<UiTheme>) {
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
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(GLOBAL_Z_MENU_PANEL),
            MenuRoot,
            PauseMenuLayer,
            BlocksWorldInput,
            FocusPolicy::Block,
            Interaction::None,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    PauseHomePanel,
                ))
                .with_children(|main| {
                    menu_framed_overlay(main, &theme, true, |frame, theme| {
                        frame
                            .spawn(menu_panel_bundle_with_fade(320.0))
                            .with_children(|panel| {
                            panel.spawn((
                                theme.value("PAUSED", 26.0),
                                MenuFadeLayer { base: CREAM },
                            ));
                            spawn_ornate_divider(panel, theme, true);
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    ResumeButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Resume", ());
                                });
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    OpenSettingsButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Settings", ());
                                });
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    QuitToMainMenuButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Main Menu", ());
                                });
                            panel.spawn((
                                theme.hint("ESC — Close menu", 12.0),
                                MenuFadeLayer { base: HINT },
                            ));
                        });
                    });
                });

            overlay
                .spawn((
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    SettingsMenuPanel,
                ))
                .with_children(|settings| {
                    menu_framed_overlay(settings, &theme, true, |frame, theme| {
                        frame
                            .spawn(menu_panel_bundle_with_fade(320.0))
                            .with_children(|panel| {
                            panel.spawn((
                                theme.value("SETTINGS", 26.0),
                                MenuFadeLayer { base: CREAM },
                            ));
                            spawn_ornate_divider(panel, theme, true);
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    GridToggle,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Grid: OFF", GridToggleLabel);
                                });
                            panel.spawn((
                                theme.hint("ESC — Back", 12.0),
                                MenuFadeLayer { base: HINT },
                            ));
                        });
                    });
                });
        });
}

fn sync_pause_world_dim(
    screen: Res<MenuScreen>,
    motion: Res<MenuMotion>,
    dim_mat: Option<Res<PauseDimMaterial>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut world_dim: Query<&mut Visibility, With<PauseWorldDim>>,
) {
    let show = *screen != MenuScreen::Closed || motion.phase == MenuMotionPhase::Exit;
    for mut vis in &mut world_dim {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !show {
        if let Some(dim_mat) = dim_mat.as_ref() {
            apply_pause_world_dim_alpha(0.0, &mut materials, &dim_mat.0);
        }
    }
}

fn sync_open_pause_menu_fade(
    screen: Res<MenuScreen>,
    motion: Res<MenuMotion>,
    dim_mat: Option<Res<PauseDimMaterial>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut fade_layers: Query<
        (&MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    mut fade_text: Query<
        (&MenuFadeLayer, &mut TextColor, &mut BackgroundColor),
        With<Text>,
    >,
) {
    if motion.phase == MenuMotionPhase::Idle && *screen != MenuScreen::Closed {
        apply_menu_fade_alpha(1.0, &mut fade_layers, &mut fade_text);
        if let Some(dim_mat) = dim_mat.as_ref() {
            apply_pause_world_dim_alpha(1.0, &mut materials, &dim_mat.0);
        }
    }
}

fn on_menu_screen_changed(
    screen: Res<MenuScreen>,
    epoch: Res<MenuScreenEpoch>,
    mut motion: ResMut<MenuMotion>,
    mut placement_mode: ResMut<BuildingPlacementMode>,
    mut mesh_cache: ResMut<LodgeBlueprintMeshCache>,
    mut last_epoch: Local<u32>,
    mut last: Local<MenuScreen>,
    mut overlay: Query<
        (&mut Node, &mut Visibility),
        (
            With<PauseMenuLayer>,
            Without<PauseHomePanel>,
            Without<SettingsMenuPanel>,
        ),
    >,
    mut main: Query<&mut Node, (With<PauseHomePanel>, Without<MenuRoot>, Without<SettingsMenuPanel>)>,
    mut settings: Query<
        &mut Node,
        (With<SettingsMenuPanel>, Without<MenuRoot>, Without<PauseHomePanel>),
    >,
    mut main_xform: Query<
        &mut Transform,
        (With<PauseHomePanel>, Without<MenuRoot>, Without<SettingsMenuPanel>),
    >,
    mut settings_xform: Query<
        &mut Transform,
        (With<SettingsMenuPanel>, Without<MenuRoot>, Without<PauseHomePanel>),
    >,
    mut fade_layers: Query<
        (&MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    mut fade_text: Query<
        (&MenuFadeLayer, &mut TextColor, &mut BackgroundColor),
        With<Text>,
    >,
    dim_mat: Option<Res<PauseDimMaterial>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if *last_epoch != epoch.0 {
        *last_epoch = epoch.0;
        *last = *screen;
        return;
    }
    if *last == *screen {
        return;
    }
    let prev = *last;
    *last = *screen;

    motion.timer = 0.0;
    match (prev, *screen) {
        (_, MenuScreen::Closed) => {
            motion.phase = MenuMotionPhase::Exit;
            motion.duration = MENU_EXIT_SECS;
            motion.exit_panel = prev;
        }
        (MenuScreen::Closed, MenuScreen::Main) => {
            cancel_lodge_placement(&mut placement_mode, &mut mesh_cache);
            motion.phase = MenuMotionPhase::Enter;
            motion.duration = MENU_ENTER_SECS;
            motion.intro_panel = MenuScreen::Main;
        }
        (MenuScreen::Main, MenuScreen::Settings) | (MenuScreen::Settings, MenuScreen::Main) => {
            motion.phase = MenuMotionPhase::Switch;
            motion.duration = MENU_SWITCH_SECS;
            motion.intro_panel = *screen;
        }
        _ => {
            motion.phase = MenuMotionPhase::Idle;
        }
    }

    let show_overlay = *screen != MenuScreen::Closed
        || motion.phase == MenuMotionPhase::Exit;
    if show_overlay {
        for (mut node, mut vis) in &mut overlay {
            node.display = Display::Flex;
            *vis = Visibility::Visible;
        }
    }

    let show_main = *screen == MenuScreen::Main
        || (motion.phase == MenuMotionPhase::Exit && motion.exit_panel == MenuScreen::Main);
    let show_settings = *screen == MenuScreen::Settings
        || (motion.phase == MenuMotionPhase::Exit && motion.exit_panel == MenuScreen::Settings);

    if let Ok(mut node) = main.get_single_mut() {
        node.display = if show_main {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut node) = settings.get_single_mut() {
        node.display = if show_settings {
            Display::Flex
        } else {
            Display::None
        };
    }

    if motion.phase == MenuMotionPhase::Enter {
        apply_menu_fade_alpha(0.0, &mut fade_layers, &mut fade_text);
        if let Some(dim_mat) = dim_mat.as_ref() {
            apply_pause_world_dim_alpha(0.0, &mut materials, &dim_mat.0);
        }
        let panel_xform = if motion.intro_panel == MenuScreen::Main {
            main_xform.get_single_mut()
        } else {
            settings_xform.get_single_mut()
        };
        if let Ok(mut xform) = panel_xform {
            *xform = menu_panel_intro_transform(0.0);
        }
    } else if motion.phase == MenuMotionPhase::Switch {
        let xform = if motion.intro_panel == MenuScreen::Main {
            main_xform.get_single_mut()
        } else {
            settings_xform.get_single_mut()
        };
        if let Ok(mut xform) = xform {
            *xform = menu_panel_intro_transform(0.0);
        }
    } else if motion.phase == MenuMotionPhase::Exit {
        apply_menu_fade_alpha(1.0, &mut fade_layers, &mut fade_text);
        if let Some(dim_mat) = dim_mat.as_ref() {
            apply_pause_world_dim_alpha(1.0, &mut materials, &dim_mat.0);
        }
        let panel_xform = if motion.exit_panel == MenuScreen::Main {
            main_xform.get_single_mut()
        } else {
            settings_xform.get_single_mut()
        };
        if let Ok(mut xform) = panel_xform {
            *xform = menu_panel_outro_transform(0.0);
        }
    }
}

fn update_menu_motion(
    time: Res<Time>,
    mut motion: ResMut<MenuMotion>,
    dim_mat: Option<Res<PauseDimMaterial>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut overlay: Query<
        (&mut Node, &mut Visibility),
        (
            With<PauseMenuLayer>,
            Without<PauseHomePanel>,
            Without<SettingsMenuPanel>,
        ),
    >,
    mut main: Query<&mut Node, (With<PauseHomePanel>, Without<MenuRoot>, Without<SettingsMenuPanel>)>,
    mut settings: Query<
        &mut Node,
        (With<SettingsMenuPanel>, Without<MenuRoot>, Without<PauseHomePanel>),
    >,
    mut main_xform: Query<
        &mut Transform,
        (With<PauseHomePanel>, Without<MenuRoot>, Without<SettingsMenuPanel>),
    >,
    mut settings_xform: Query<
        &mut Transform,
        (With<SettingsMenuPanel>, Without<MenuRoot>, Without<PauseHomePanel>),
    >,
    mut fade_layers: Query<
        (&MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    mut fade_text: Query<
        (&MenuFadeLayer, &mut TextColor, &mut BackgroundColor),
        With<Text>,
    >,
) {
    if motion.phase == MenuMotionPhase::Idle {
        return;
    }

    if motion.phase == MenuMotionPhase::Teardown {
        motion.phase = MenuMotionPhase::Idle;
        motion.timer = 0.0;
        return;
    }

    motion.timer += time.delta_secs();
    let raw_t = (motion.timer / motion.duration).min(1.0);

    match motion.phase {
        MenuMotionPhase::Enter => {
            let fade = ease_out_cubic(raw_t);
            apply_menu_fade_alpha(fade, &mut fade_layers, &mut fade_text);
            if let Some(dim_mat) = dim_mat.as_ref() {
                apply_pause_world_dim_alpha(fade, &mut materials, &dim_mat.0);
            }
            let panel_xform = if motion.intro_panel == MenuScreen::Main {
                main_xform.get_single_mut()
            } else {
                settings_xform.get_single_mut()
            };
            if let Ok(mut xform) = panel_xform {
                *xform = menu_panel_intro_transform(raw_t);
            }
        }
        MenuMotionPhase::Switch => {
            let xform = if motion.intro_panel == MenuScreen::Main {
                main_xform.get_single_mut()
            } else {
                settings_xform.get_single_mut()
            };
            if let Ok(mut xform) = xform {
                *xform = menu_panel_intro_transform(raw_t);
            }
        }
        MenuMotionPhase::Exit => {
            let fade = ease_out_cubic(1.0 - raw_t);
            apply_menu_fade_alpha(fade, &mut fade_layers, &mut fade_text);
            if let Some(dim_mat) = dim_mat.as_ref() {
                apply_pause_world_dim_alpha(fade, &mut materials, &dim_mat.0);
            }
            let panel_xform = if motion.exit_panel == MenuScreen::Main {
                main_xform.get_single_mut()
            } else {
                settings_xform.get_single_mut()
            };
            if let Ok(mut xform) = panel_xform {
                *xform = menu_panel_outro_transform(raw_t);
            }
        }
        MenuMotionPhase::Teardown | MenuMotionPhase::Idle => {}
    }

    if motion.timer < motion.duration {
        return;
    }

    match motion.phase {
        MenuMotionPhase::Enter | MenuMotionPhase::Switch => {
            if let Ok(mut xform) = main_xform.get_single_mut() {
                *xform = Transform::default();
            }
            if let Ok(mut xform) = settings_xform.get_single_mut() {
                *xform = Transform::default();
            }
            apply_menu_fade_alpha(1.0, &mut fade_layers, &mut fade_text);
            if let Some(dim_mat) = dim_mat.as_ref() {
                apply_pause_world_dim_alpha(1.0, &mut materials, &dim_mat.0);
            }
        }
        MenuMotionPhase::Exit => {
            apply_menu_fade_alpha(0.0, &mut fade_layers, &mut fade_text);
            if let Some(dim_mat) = dim_mat.as_ref() {
                apply_pause_world_dim_alpha(0.0, &mut materials, &dim_mat.0);
            }
            if let Ok((mut node, mut vis)) = overlay.get_single_mut() {
                node.display = Display::Flex;
                *vis = Visibility::Hidden;
            }
            if let Ok(mut xform) = main_xform.get_single_mut() {
                *xform = Transform::default();
            }
            if let Ok(mut xform) = settings_xform.get_single_mut() {
                *xform = Transform::default();
            }
            if let Ok(mut node) = main.get_single_mut() {
                node.display = Display::None;
            }
            if let Ok(mut node) = settings.get_single_mut() {
                node.display = Display::None;
            }
            motion.phase = MenuMotionPhase::Teardown;
            motion.timer = 0.0;
        }
        MenuMotionPhase::Idle | MenuMotionPhase::Teardown => {}
    }
    if motion.phase == MenuMotionPhase::Teardown {
        return;
    }
    motion.phase = MenuMotionPhase::Idle;
    motion.timer = 0.0;
}

fn handle_menu_escape(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut screen: ResMut<MenuScreen>,
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

    *screen = match *screen {
        MenuScreen::Closed => MenuScreen::Main,
        MenuScreen::Main => MenuScreen::Closed,
        MenuScreen::Settings => MenuScreen::Main,
    };
}

fn style_menu_buttons(
    mut buttons: Query<(&Interaction, &Children), (Changed<Interaction>, With<MenuButton>)>,
    mut fills: Query<(&mut MenuFadeLayer, &mut BackgroundColor), With<MenuButtonFill>>,
) {
    for (interaction, children) in &mut buttons {
        let color = match *interaction {
            Interaction::Pressed => BTN_PRESSED,
            Interaction::Hovered => BTN_HOVER,
            Interaction::None => BTN_IDLE,
        };
        for child in children.iter() {
            if let Ok((mut layer, mut bg)) = fills.get_mut(*child) {
                layer.base = color;
                bg.0 = color;
            }
        }
    }
}

fn handle_resume_button(
    mut interaction: Query<&Interaction, (Changed<Interaction>, With<ResumeButton>)>,
    mut screen: ResMut<MenuScreen>,
) {
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            *screen = MenuScreen::Closed;
        }
    }
}

fn handle_quit_to_main_menu_button(
    mut interaction: Query<&Interaction, (Changed<Interaction>, With<QuitToMainMenuButton>)>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    mut epoch: ResMut<MenuScreenEpoch>,
    mut screen: ResMut<MenuScreen>,
    mut motion: ResMut<MenuMotion>,
    mut title: ResMut<TitleScreen>,
    mut selected: ResMut<SelectedHex>,
    mut hovered: ResMut<HoveredHex>,
    mut job: ResMut<LoadingJob>,
    mut progress: ResMut<LoadingProgress>,
    world_entities: Query<Entity, With<WorldEntity>>,
    mut cameras: Query<(&mut Transform, &mut OrthographicProjection), With<Camera2d>>,
) {
    for interaction in &mut interaction {
        if *interaction != Interaction::Pressed {
            continue;
        }

        for entity in &world_entities {
            commands.entity(entity).despawn_recursive();
        }
        commands.remove_resource::<GameMap>();
        commands.remove_resource::<GameState>();
        commands.remove_resource::<GridMaterial>();

        selected.0 = None;
        hovered.0 = None;
        epoch.0 += 1;
        *screen = MenuScreen::Closed;
        *motion = MenuMotion::default();
        *title = TitleScreen::Home;
        *job = LoadingJob::None;
        *progress = LoadingProgress::default();

        if let Ok((mut transform, mut projection)) = cameras.get_single_mut() {
            *transform = Transform::default();
            projection.scale = 1.0;
        }

        next_state.set(AppState::MainMenu);
    }
}

fn handle_open_settings_button(
    mut query: Query<&Interaction, (Changed<Interaction>, With<OpenSettingsButton>)>,
    mut screen: ResMut<MenuScreen>,
) {
    for interaction in &mut query {
        if *interaction == Interaction::Pressed {
            *screen = MenuScreen::Settings;
        }
    }
}

fn handle_toggle_button(
    mut interaction_query: Query<
        &Interaction,
        (Changed<Interaction>, With<GridToggle>),
    >,
    mut grid_visible: ResMut<GridVisible>,
    mut grid_query: Query<&mut Visibility, With<GridMarker>>,
    mut text_query: Query<&mut Text, With<GridToggleLabel>>,
) {
    for interaction in &mut interaction_query {
        if *interaction != Interaction::Pressed {
            continue;
        }
        grid_visible.0 = !grid_visible.0;
        if let Ok(mut vis) = grid_query.get_single_mut() {
            *vis = if grid_visible.0 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
        for mut text in &mut text_query {
            text.0 = if grid_visible.0 {
                "Grid: ON".to_string()
            } else {
                "Grid: OFF".to_string()
            };
        }
    }
}

fn examiner_tile_fields(
    map: &Map,
    coord: HexCoord,
) -> (String, String, String, String, Color) {
    match map.tile_at(coord) {
        None => (
            "Out of map".to_string(),
            "—".to_string(),
            format_hover_coord_half("q", Some(coord.q)),
            format_hover_coord_half("r", Some(coord.r)),
            Color::srgb(0.35, 0.38, 0.45),
        ),
        Some(tile) => (
            terrain_label(tile.terrain).to_string(),
            terrain_category(tile.terrain)
                .map(str::to_string)
                .unwrap_or_else(|| "Other".to_string()),
            format_hover_coord_half("q", Some(coord.q)),
            format_hover_coord_half("r", Some(coord.r)),
            terrain_swatch_color(tile.terrain),
        ),
    }
}

fn handle_examiner_click(
    mouse: Res<ButtonInput<MouseButton>>,
    screen: Res<MenuScreen>,
    mode: Res<BuildingPlacementMode>,
    suppress: Res<LodgePlacementSuppressClick>,
    blockers: Query<&Interaction, With<BlocksWorldInput>>,
    lodge_button: Query<&Interaction, With<HuntingLodgeButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, With<Camera2d>>,
    zoom: Res<Zoom>,
    placed: Res<PlacedLodges>,
    mut examiner: ResMut<ExaminerSelection>,
) {
    if mode.is_placing_lodge()
        || pause_menu_open(&screen)
        || !mouse.just_pressed(MouseButton::Left)
    {
        return;
    }
    if suppress.frames > 0 || pointer_over_ingame_ui(blockers, lodge_button) {
        return;
    }
    let Some(mouse_pos) = window.single().cursor_position() else {
        return;
    };
    let cam = camera.single();
    let ws = Vec2::new(window.single().width(), window.single().height());
    let world = screen_to_world(cam, ws, mouse_pos, zoom.0);
    let coord = pixel_to_hex(world.x, world.y, HEX_SIZE);

    examiner.0 = Some(
        placed
            .lodge_index_at(coord)
            .map(ExaminerFocus::Building)
            .unwrap_or(ExaminerFocus::Tile(coord)),
    );
}

fn animate_selection_highlight(
    time: Res<Time>,
    examiner: Res<ExaminerSelection>,
    placed: Res<PlacedLodges>,
    mut mesh_cache: ResMut<SelectionBuildingMeshCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut tile_ring: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial2d<ColorMaterial>),
        With<SelectionTileRing>,
    >,
    mut building_root: Query<
        (&mut Transform, &mut Visibility),
        (With<SelectionBuildingRoot>, Without<SelectionTileRing>),
    >,
    mut building_fill: Query<
        &mut Mesh2d,
        (
            With<SelectionBuildingFill>,
            Without<SelectionTileRing>,
            Without<SelectionBuildingRoot>,
            Without<SelectionBuildingOutline>,
        ),
    >,
    mut building_outline: Query<
        (
            &mut Transform,
            &mut Mesh2d,
            &MeshMaterial2d<ColorMaterial>,
        ),
        (
            With<SelectionBuildingOutline>,
            Without<SelectionTileRing>,
            Without<SelectionBuildingRoot>,
            Without<SelectionBuildingFill>,
        ),
    >,
) {
    let pulse = (time.elapsed_secs() * std::f32::consts::TAU * HOVER_GLOW_PULSE_HZ).sin();
    let scale = 1.0 + pulse * HOVER_GLOW_PULSE_SCALE;
    let alpha = HOVER_GLOW_ALPHA_BASE + pulse * HOVER_GLOW_ALPHA_AMP;

    let Ok((mut tile_transform, mut tile_vis, tile_mat)) = tile_ring.get_single_mut() else {
        return;
    };
    let Ok((mut build_transform, mut build_vis)) = building_root.get_single_mut() else {
        return;
    };

    match examiner.0 {
        None => {
            *tile_vis = Visibility::Hidden;
            *build_vis = Visibility::Hidden;
            tile_transform.scale = Vec3::ONE;
            build_transform.scale = Vec3::ONE;
            if let Ok((mut outline_tf, _, _)) = building_outline.get_single_mut() {
                outline_tf.scale = Vec3::ONE;
            }
        }
        Some(ExaminerFocus::Tile(hex)) => {
            *build_vis = Visibility::Hidden;
            build_transform.scale = Vec3::ONE;
            if let Ok((mut outline_tf, _, _)) = building_outline.get_single_mut() {
                outline_tf.scale = Vec3::ONE;
            }
            let (tx, ty) = axial_to_pixel(hex.q, hex.r, HEX_SIZE);
            tile_transform.translation = Vec3::new(tx, ty, 5.2);
            tile_transform.scale = Vec3::splat(scale);
            *tile_vis = Visibility::Visible;
            if let Some(mat) = materials.get_mut(&tile_mat.0) {
                mat.color = GOLD.with_alpha(alpha);
            }
        }
        Some(ExaminerFocus::Building(idx)) => {
            *tile_vis = Visibility::Hidden;
            tile_transform.scale = Vec3::ONE;
            let Some(lodge) = placed.lodges.get(idx) else {
                *build_vis = Visibility::Hidden;
                return;
            };
            if mesh_cache.rotation != Some(lodge.rotation) {
                let (fill, outline) =
                    lodge_selection_meshes_for_rotation(&mut meshes, lodge.rotation);
                mesh_cache.rotation = Some(lodge.rotation);
                mesh_cache.fill = Some(fill.clone());
                mesh_cache.outline = Some(outline.clone());
                if let Ok(mut mesh2d) = building_fill.get_single_mut() {
                    mesh2d.0 = fill;
                }
                if let Ok((_, mut mesh2d, _)) = building_outline.get_single_mut() {
                    mesh2d.0 = outline;
                }
            }
            let coords = lodge_coords(lodge.anchor, lodge.rotation);
            let centroid = footprint_mesh::footprint_centroid(&coords, HEX_SIZE);
            build_transform.translation = Vec3::new(centroid.x, centroid.y, 5.2);
            build_transform.scale = Vec3::ONE;
            *build_vis = Visibility::Visible;
            if let Ok((mut outline_tf, _, mat)) = building_outline.get_single_mut() {
                outline_tf.scale = Vec3::splat(scale);
                if let Some(m) = materials.get_mut(&mat.0) {
                    m.color = GOLD.with_alpha(alpha);
                }
            }
        }
    }
}

fn update_examiner_panel(
    examiner: Res<ExaminerSelection>,
    placed: Res<PlacedLodges>,
    map: Res<GameMap>,
    mut texts: ParamSet<(
        Query<&mut Text, With<HoverNameText>>,
        Query<&mut Text, With<HoverCategoryText>>,
        Query<&mut Text, With<HoverCoordsQ>>,
        Query<&mut Text, With<HoverCoordsR>>,
        Query<&mut Text, With<ExaminerBuildingNameText>>,
        Query<&mut Text, With<ExaminerBuildingFoodText>>,
    )>,
    mut swatch: Query<&mut BackgroundColor, With<HoverSwatch>>,
    mut hint: Query<
        &mut Visibility,
        (With<HoverEmptyHint>, Without<ExaminerBuildingBlock>),
    >,
    mut building_block: Query<
        (&mut Visibility, &mut Node),
        (
            With<ExaminerBuildingBlock>,
            Without<HoverEmptyHint>,
            Without<ExaminerBuildingDivider>,
        ),
    >,
    mut building_divider: Query<
        (&mut Visibility, &mut Node),
        (
            With<ExaminerBuildingDivider>,
            Without<HoverEmptyHint>,
            Without<ExaminerBuildingBlock>,
        ),
    >,
) {
    let building_selected = matches!(examiner.0, Some(ExaminerFocus::Building(_)));
    if !examiner.is_changed() && !(building_selected && placed.is_changed()) {
        return;
    }

    let Ok(mut swatch) = swatch.get_single_mut() else {
        return;
    };
    let Ok(mut hint) = hint.get_single_mut() else {
        return;
    };
    let Ok((mut block_vis, mut block_node)) = building_block.get_single_mut() else {
        return;
    };
    let Ok((mut divider_vis, mut divider_node)) = building_divider.get_single_mut() else {
        return;
    };

    let (name, category, q_coord, r_coord, color, show_hint, show_building, building_name, food) =
        match examiner.0 {
            None => (
                "—".to_string(),
                "—".to_string(),
                format_hover_coord_half("q", None),
                format_hover_coord_half("r", None),
                Color::srgb(0.35, 0.38, 0.45),
                true,
                false,
                String::new(),
                String::new(),
            ),
            Some(ExaminerFocus::Tile(coord)) => {
                let (n, c, q, r, col) = examiner_tile_fields(&map.0, coord);
                (n, c, q, r, col, false, false, String::new(), String::new())
            }
            Some(ExaminerFocus::Building(idx)) => {
                let Some(lodge) = placed.lodges.get(idx) else {
                    return;
                };
                let (n, c, q, r, col) = examiner_tile_fields(&map.0, lodge.anchor);
                (
                    n,
                    c,
                    q,
                    r,
                    col,
                    false,
                    true,
                    "Hunting Lodge".to_string(),
                    lodge.food_stored.to_string(),
                )
            }
        };

    *hint = if show_hint {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    *block_vis = if show_building {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    block_node.display = if show_building {
        Display::Flex
    } else {
        Display::None
    };
    *divider_vis = if show_building {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    divider_node.display = if show_building {
        Display::Flex
    } else {
        Display::None
    };
    swatch.0 = color;

    if let Ok(mut text) = texts.p0().get_single_mut() {
        text.0 = name;
    }
    if let Ok(mut text) = texts.p1().get_single_mut() {
        text.0 = category;
    }
    if let Ok(mut text) = texts.p2().get_single_mut() {
        text.0 = q_coord;
    }
    if let Ok(mut text) = texts.p3().get_single_mut() {
        text.0 = r_coord;
    }
    if let Ok(mut text) = texts.p4().get_single_mut() {
        text.0 = building_name;
    }
    if let Ok(mut text) = texts.p5().get_single_mut() {
        text.0 = food;
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

fn terrain_category(t: TerrainType) -> Option<&'static str> {
    match t {
        TerrainType::Plains | TerrainType::Greenfield => Some("Base"),
        TerrainType::DeepOcean
        | TerrainType::Ocean
        | TerrainType::Coast
        | TerrainType::Freshwater => Some("Water"),
        TerrainType::SnowPeak | TerrainType::StonySlope => Some("Mountain"),
        TerrainType::Beach => Some("Shore"),
        TerrainType::Oldwood | TerrainType::Darkpine | TerrainType::Deepjungle => Some("Forest"),
        _ => None,
    }
}

fn end_turn(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<MenuScreen>,
    mut gs: ResMut<GameState>,
    mut query: Query<&mut Text, With<TurnText>>,
    map: Res<GameMap>,
) {
    let advance = keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter);
    if !advance || pause_menu_open(&screen) {
        return;
    }
    gs.next_turn(&map.0);
    if let Ok(mut text) = query.get_single_mut() {
        text.0 = gs.turn.to_string();
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
    mut zoom: ResMut<Zoom>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut OrthographicProjection), With<Camera2d>>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<TurnText>>,
        Query<&mut Text, With<SeedText>>,
    )>,
    world_entities: Query<Entity, With<WorldEntity>>,
    mut selected: ResMut<SelectedHex>,
    mut hovered: ResMut<HoveredHex>,
    mut placement_mode: ResMut<BuildingPlacementMode>,
    mut placed_lodges: ResMut<PlacedLodges>,
) {
    if !keys.just_pressed(KeyCode::Backslash) {
        return;
    }

    for e in &world_entities {
        commands.entity(e).despawn_recursive();
    }

    *placement_mode = BuildingPlacementMode::Idle;
    *placed_lodges = PlacedLodges::default();

    let seed = rand::thread_rng().gen::<u64>();
    seed_res.0 = seed;
    println!("World rerolled with seed: {seed}");

    let (map, new_gs) = spawn_world_entities(&mut commands, &mut meshes, &mut materials, seed);
    game_map.0 = map;
    *gs = new_gs;

    if let Ok(window) = window.get_single() {
        let window_size = Vec2::new(window.width(), window.height());
        if window_size.x > 0.0 && window_size.y > 0.0 {
            if let Ok((mut transform, mut projection)) = cameras.get_single_mut() {
                apply_camera_frame_to_tiles(
                    &game_map.0.tiles,
                    window_size,
                    &mut transform,
                    &mut projection,
                    &mut zoom.0,
                );
            }
        }
    }

    selected.0 = None;
    hovered.0 = None;

    if let Ok(mut text) = text_queries.p0().get_single_mut() {
        text.0 = "1".to_string();
    }
    if let Ok(mut text) = text_queries.p1().get_single_mut() {
        text.0 = seed.to_string();
    }
}
