use bevy::prelude::*;
use bevy::render::{
    render_asset::RenderAssetUsages,
    render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy::window::PrimaryWindow;

use crate::app_state::{AppState, LoadingJob, LoadingProgress};
use crate::{GridToggle, GridToggleLabel};
use crate::ui::{smooth_follow, UiTheme, GOLD};

const TITLE_RAIL_WIDTH: f32 = 38.0;
const TITLE_RAIL_PADDING_LEFT: f32 = 40.0;
const TITLE_SCRIM_SOLID_WIDTH: f32 = 38.0;
const TITLE_SCRIM_FADE_WIDTH: f32 = 36.0;
const TITLE_SCRIM_PEAK_ALPHA: f32 = 0.82;
const TITLE_SCRIM_FADE_TEX_WIDTH: u32 = 512;
const TITLE_HEADLINE_SIZE: f32 = 72.0;
const TITLE_SUBTITLE_SIZE: f32 = 15.0;
const TITLE_ITEM_FONT: f32 = 30.0;
const TITLE_HINT_FONT: f32 = 15.0;
const TITLE_ITEM_HEIGHT: f32 = 54.0;
const TITLE_ITEM_GAP: f32 = 12.0;
const TITLE_HEADER_GAP: f32 = 48.0;
const TITLE_TEXT: Color = Color::srgb(0.98, 0.93, 0.82);
const TITLE_TEXT_DIM: Color = Color::srgba(0.78, 0.62, 0.34, 0.45);
const TITLE_BORDER_GLOW: Color = Color::srgb(1.0, 0.72, 0.28);
const TITLE_BORDER_IDLE: Color = Color::srgba(0.78, 0.62, 0.34, 0.22);
const TITLE_BTN_BG: Color = Color::srgb(0.0, 0.0, 0.0);
const TITLE_BTN_BG_HOVER: Color = Color::srgb(0.06, 0.05, 0.04);
const TITLE_BTN_BG_PRESSED: Color = Color::srgb(0.10, 0.08, 0.06);
const TITLE_MOUSE_Y_RADIUS: f32 = 280.0;
const TITLE_MOUSE_Y_SCALE_BOOST: f32 = 0.11;
const TITLE_UI_SCALE_SMOOTH: f32 = 6.0;

pub fn save_file_exists() -> bool {
    std::path::Path::new(&crate::default_save_path()).exists()
}

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
pub enum TitleScreen {
    #[default]
    Home,
    Settings,
}

#[derive(Component)]
pub struct TitleRoot;

#[derive(Component)]
pub struct TitleHomePanel;

#[derive(Component)]
pub struct TitleSettingsPanel;

#[derive(Component)]
pub struct TitleMenuButton;

#[derive(Component)]
pub struct TitlePlayButton;

#[derive(Component)]
pub struct TitleContinueButton;

#[derive(Component)]
pub struct TitleSettingsButton;

#[derive(Component)]
pub struct TitleQuitButton;

#[derive(Component)]
pub struct TitleBackButton;

#[derive(Component)]
pub struct TitleContinueHint;

#[derive(Component)]
pub struct TitleMenuScrim;

#[derive(Component)]
pub struct TitleMenuHoverTarget;

#[derive(Component)]
pub(crate) struct TitleMenuHoverMotion {
    scale: f32,
}

impl TitleMenuHoverMotion {
    const fn new() -> Self {
        Self { scale: 1.0 }
    }
}

pub fn spawn_title_menu(
    mut commands: Commands,
    theme: Res<UiTheme>,
    mut images: ResMut<Assets<Image>>,
) {
    let has_save = save_file_exists();

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
            TitleRoot,
        ))
        .with_children(|overlay| {
            spawn_title_menu_scrim(overlay, &mut images);

            overlay
                .spawn((title_menu_column(), TitleHomePanel, ZIndex(1)))
                .with_children(|home| {
                    spawn_title_header(home, &theme, "MAIN MENU");
                    spawn_title_menu_button(home, &theme, "NEW GAME", TitlePlayButton, ());
                    spawn_continue_button(home, &theme, has_save);
                    spawn_title_menu_button(home, &theme, "SETTINGS", TitleSettingsButton, ());
                    spawn_title_menu_button(home, &theme, "EXIT GAME", TitleQuitButton, ());
                });

            overlay
                .spawn((
                    Node {
                        display: Display::None,
                        ..title_menu_column()
                    },
                    TitleSettingsPanel,
                    ZIndex(1),
                ))
                .with_children(|settings| {
                    spawn_title_header(settings, &theme, "SETTINGS");
                    spawn_title_menu_button(settings, &theme, "Grid: OFF", GridToggle, GridToggleLabel);
                    spawn_title_menu_button(settings, &theme, "BACK", TitleBackButton, ());
                    settings.spawn((
                        title_hint_bundle(&theme, "Grid applies in-game", TITLE_HINT_FONT),
                        Node {
                            margin: UiRect::top(Val::Px(12.0)),
                            ..default()
                        },
                    ));
                });
        });
}

fn spawn_title_menu_scrim(parent: &mut bevy::hierarchy::ChildBuilder, images: &mut Assets<Image>) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(TITLE_SCRIM_SOLID_WIDTH),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, TITLE_SCRIM_PEAK_ALPHA)),
            TitleMenuScrim,
            ZIndex(0),
        ));

    let fade_image = images.add(make_scrim_fade_image());
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(TITLE_SCRIM_SOLID_WIDTH),
            top: Val::ZERO,
            width: Val::Percent(TITLE_SCRIM_FADE_WIDTH),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(fade_image),
        ZIndex(0),
    ));
}

fn make_scrim_fade_image() -> Image {
    let width = TITLE_SCRIM_FADE_TEX_WIDTH;
    let mut data = Vec::with_capacity((width * 4) as usize);
    for x in 0..width {
        let t = x as f32 / (width - 1) as f32;
        let alpha = scrim_fade_alpha(t);
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        data.extend_from_slice(&[0, 0, 0, a]);
    }
    Image::new(
        Extent3d {
            width,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn scrim_fade_alpha(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    TITLE_SCRIM_PEAK_ALPHA * 0.5 * (1.0 + (std::f32::consts::PI * t).cos())
}

fn title_menu_column() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::ZERO,
        top: Val::ZERO,
        width: Val::Percent(TITLE_RAIL_WIDTH),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Stretch,
        padding: UiRect {
            left: Val::Px(TITLE_RAIL_PADDING_LEFT),
            right: Val::Px(24.0),
            ..default()
        },
        row_gap: Val::Px(TITLE_ITEM_GAP),
        ..default()
    }
}

fn title_menu_button_bundle() -> impl Bundle {
    (
        Button,
        TitleMenuButton,
        TitleMenuHoverTarget,
        TitleMenuHoverMotion::new(),
        Node {
            width: Val::Percent(90.0),
            height: Val::Px(TITLE_ITEM_HEIGHT),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(TITLE_BTN_BG),
        BorderColor(TITLE_BORDER_IDLE),
    )
}

fn title_headline_bundle(theme: &UiTheme, text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        theme.font(TITLE_HEADLINE_SIZE),
        TextColor(TITLE_TEXT),
    )
}

fn title_subtitle_bundle(theme: &UiTheme, text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        theme.font(TITLE_SUBTITLE_SIZE),
        TextColor(GOLD),
    )
}

fn title_item_bundle(theme: &UiTheme, text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        theme.font(TITLE_ITEM_FONT),
        TextColor(TITLE_TEXT),
    )
}

fn title_hint_bundle(theme: &UiTheme, text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        theme.font(size),
        TextColor(TITLE_TEXT_DIM),
    )
}

fn spawn_title_header(parent: &mut bevy::hierarchy::ChildBuilder, theme: &UiTheme, subtitle: &str) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: Val::Px(6.0),
            margin: UiRect::bottom(Val::Px(TITLE_HEADER_GAP)),
            ..default()
        })
        .with_children(|header| {
            header.spawn((
                title_headline_bundle(theme, "MYARI"),
                TitleMenuHoverTarget,
                TitleMenuHoverMotion::new(),
            ));
            header.spawn((
                title_subtitle_bundle(theme, subtitle),
                TitleMenuHoverTarget,
                TitleMenuHoverMotion::new(),
            ));
        });
}

fn spawn_title_menu_button(
    parent: &mut bevy::hierarchy::ChildBuilder,
    theme: &UiTheme,
    text: impl Into<String>,
    marker: impl Bundle,
    label_extra: impl Bundle,
) {
    parent
        .spawn((title_menu_button_bundle(), marker))
        .with_children(|btn| {
            btn.spawn((
                title_item_bundle(theme, text),
                TextLayout::new_with_no_wrap(),
                label_extra,
            ));
        });
}

fn spawn_continue_button(parent: &mut bevy::hierarchy::ChildBuilder, theme: &UiTheme, enabled: bool) {
    if enabled {
        spawn_title_menu_button(parent, theme, "CONTINUE GAME", TitleContinueButton, ());
    } else {
        parent
            .spawn((
                Node {
                    width: Val::Percent(90.0),
                    height: Val::Px(TITLE_ITEM_HEIGHT),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    ..default()
                },
                TitleMenuHoverTarget,
                TitleMenuHoverMotion::new(),
            ))
            .with_children(|row| {
                row.spawn((
                    title_hint_bundle(theme, "CONTINUE GAME", TITLE_ITEM_FONT),
                    TextLayout::new_with_no_wrap(),
                ));
            });
        parent.spawn((
            title_hint_bundle(theme, "No save found", TITLE_HINT_FONT),
            TitleContinueHint,
            TitleMenuHoverTarget,
            TitleMenuHoverMotion::new(),
            Node {
                margin: UiRect::left(Val::Px(8.0)),
                ..default()
            },
        ));
    }
}

fn y_mouse_influence(dist_y: f32) -> f32 {
    let t = (1.0 - dist_y / TITLE_MOUSE_Y_RADIUS).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn animate_title_menu_hover(
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Camera, With<Camera2d>>,
    mut targets: Query<
        (
            &GlobalTransform,
            &ComputedNode,
            &mut TitleMenuHoverMotion,
            &mut Transform,
        ),
        (With<TitleMenuHoverTarget>, With<ViewVisibility>),
    >,
) {
    let dt = time.delta_secs();
    let Ok(window) = window.get_single() else {
        return;
    };
    let Ok(camera) = camera.get_single() else {
        return;
    };

    let mouse_y = window
        .physical_cursor_position()
        .map(|cursor| {
            let viewport_min = camera
                .physical_viewport_rect()
                .map(|rect| rect.min.as_vec2())
                .unwrap_or_default();
            (cursor - viewport_min).y
        });

    for (global, computed, mut motion, mut transform) in &mut targets {
        let target_scale = if computed.size() == Vec2::ZERO {
            1.0
        } else if let Some(mouse_y) = mouse_y {
            let center_y = global.translation().y;
            let influence = y_mouse_influence((mouse_y - center_y).abs());
            1.0 + TITLE_MOUSE_Y_SCALE_BOOST * influence
        } else {
            1.0
        };

        motion.scale = smooth_follow(motion.scale, target_scale, dt, TITLE_UI_SCALE_SMOOTH);
        transform.scale = Vec3::splat(motion.scale);
    }
}

pub fn style_title_menu_buttons(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<TitleMenuButton>),
    >,
) {
    for (interaction, mut bg, mut border) in &mut query {
        bg.0 = match *interaction {
            Interaction::Pressed => TITLE_BTN_BG_PRESSED,
            Interaction::Hovered => TITLE_BTN_BG_HOVER,
            Interaction::None => TITLE_BTN_BG,
        };
        border.0 = match *interaction {
            Interaction::Pressed | Interaction::Hovered => TITLE_BORDER_GLOW,
            Interaction::None => TITLE_BORDER_IDLE,
        };
    }
}

pub fn sync_title_subscreen(
    title: Res<TitleScreen>,
    mut home: Query<&mut Node, (With<TitleHomePanel>, Without<TitleSettingsPanel>)>,
    mut settings: Query<&mut Node, (With<TitleSettingsPanel>, Without<TitleHomePanel>)>,
) {
    if !title.is_changed() {
        return;
    }
    if let Ok(mut node) = home.get_single_mut() {
        node.display = if *title == TitleScreen::Home {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut node) = settings.get_single_mut() {
        node.display = if *title == TitleScreen::Settings {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub fn handle_title_play(
    mut interaction: Query<
        &Interaction,
        (Changed<Interaction>, With<TitlePlayButton>),
    >,
    mut job: ResMut<LoadingJob>,
    mut progress: ResMut<LoadingProgress>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            *job = LoadingJob::NewGame;
            *progress = LoadingProgress::reset_for_job(LoadingJob::NewGame);
            next_state.set(AppState::Loading);
        }
    }
}

pub fn handle_title_continue(
    mut interaction: Query<
        &Interaction,
        (Changed<Interaction>, With<TitleContinueButton>),
    >,
    mut job: ResMut<LoadingJob>,
    mut progress: ResMut<LoadingProgress>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !save_file_exists() {
        return;
    }
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            *job = LoadingJob::Continue;
            *progress = LoadingProgress::reset_for_job(LoadingJob::Continue);
            next_state.set(AppState::Loading);
        }
    }
}

pub fn handle_title_settings(
    mut interaction: Query<
        &Interaction,
        (Changed<Interaction>, With<TitleSettingsButton>),
    >,
    mut title: ResMut<TitleScreen>,
) {
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            *title = TitleScreen::Settings;
        }
    }
}

pub fn handle_title_back(
    mut interaction: Query<
        &Interaction,
        (Changed<Interaction>, With<TitleBackButton>),
    >,
    mut title: ResMut<TitleScreen>,
) {
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            *title = TitleScreen::Home;
        }
    }
}

pub fn handle_title_quit(
    mut interaction: Query<
        &Interaction,
        (Changed<Interaction>, With<TitleQuitButton>),
    >,
    mut exit: EventWriter<AppExit>,
) {
    for interaction in &mut interaction {
        if *interaction == Interaction::Pressed {
            exit.send(AppExit::Success);
        }
    }
}
