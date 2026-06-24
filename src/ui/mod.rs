//! Shared fantasy HUD styling — Cinzel font, parchment frame, gold labels.

use bevy::hierarchy::ChildBuilder;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::app_state::InGameHud;

pub mod hover_gem;
pub mod loading_screen;
pub mod title_hex_grid;
pub mod title_menu;

pub const FONT_PATH: &str = "fonts/Cinzel-Regular.ttf";
pub const SYMBOL_FONT_PATH: &str = "fonts/NotoSansSymbols2-Regular.ttf";
pub const STAR: &str = "✦";

pub const GOLD: Color = Color::srgb(0.78, 0.62, 0.34);
pub const GOLD_DIM: Color = Color::srgb(0.52, 0.40, 0.22);
pub const CREAM: Color = Color::srgb(0.93, 0.88, 0.76);
pub const PARCHMENT: Color = Color::srgb(0.36, 0.28, 0.20);
pub const PANEL: Color = Color::srgba(0.07, 0.05, 0.04, 0.98);
pub const GEM_FRAME: Color = Color::srgba(0.04, 0.03, 0.02, 0.95);
pub const HINT: Color = Color::srgba(0.75, 0.62, 0.38, 0.55);
pub const BTN_IDLE: Color = Color::srgba(0.10, 0.08, 0.06, 0.95);
pub const BTN_HOVER: Color = Color::srgba(0.16, 0.12, 0.08, 0.98);
pub const BTN_PRESSED: Color = Color::srgba(0.22, 0.16, 0.10, 1.0);

/// Marks HUD panels that should absorb pointer input from reaching the map.
#[derive(Component)]
pub struct BlocksWorldInput;

pub const MENU_ENTER_SECS: f32 = 0.22;
/// Pause-menu close — fade + shrink together, then done (no post-hide shrink).
pub const MENU_EXIT_SECS: f32 = 0.22;
pub const MENU_SWITCH_SECS: f32 = 0.15;

/// In-game HUD — above world dim, below pause panel.
pub const GLOBAL_Z_HUD: i32 = 0;
/// Pause/settings panel.
pub const GLOBAL_Z_MENU_PANEL: i32 = 10;

/// Base alpha for the world-space pause dim (not UI — never covers HUD).
pub const PAUSE_WORLD_DIM_ALPHA: f32 = 0.72;

/// Pause-menu layer with an authored base color; alpha is driven by menu motion.
#[derive(Component, Clone, Copy)]
pub struct MenuFadeLayer {
    pub base: Color,
}

/// Apply a unified fade (0–1) to every [`MenuFadeLayer`] surface (background or text).
pub fn apply_menu_fade_alpha(
    alpha: f32,
    bg_layers: &mut Query<
        (&MenuFadeLayer, &mut BackgroundColor),
        (Without<Text>, With<MenuFadeLayer>),
    >,
    text_layers: &mut Query<
        (&MenuFadeLayer, &mut TextColor, &mut BackgroundColor),
        With<Text>,
    >,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    let bg_count = bg_layers.iter().count();
    let text_count = text_layers.iter().count();
    for (layer, mut bg) in &mut *bg_layers {
        bg.0 = layer.base.with_alpha(layer.base.alpha() * alpha);
    }
    for (layer, mut text, mut bg) in &mut *text_layers {
        text.0 = layer.base.with_alpha(layer.base.alpha() * alpha);
        // Node requires BackgroundColor; keep text nodes visually transparent.
        bg.0 = Color::NONE;
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
            r#"{{"sessionId":"a40c64","runId":"post-fix","hypothesisId":"H1","location":"ui/mod.rs:apply_menu_fade_alpha","message":"fade applied globally","data":{{"alpha":{alpha},"bg_count":{bg_count},"text_count":{text_count}}},"timestamp":{ts}}}"#
        );
    }
    // #endregion
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Slight overshoot past 1.0 — good for placement pops.
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

pub fn smooth_follow(current: f32, target: f32, delta_secs: f32, speed: f32) -> f32 {
    if delta_secs <= 0.0 {
        return current;
    }
    let t = 1.0 - (-speed * delta_secs).exp();
    current + (target - current) * t
}

pub fn smooth_follow_vec2(current: Vec2, target: Vec2, delta_secs: f32, speed: f32) -> Vec2 {
    Vec2::new(
        smooth_follow(current.x, target.x, delta_secs, speed),
        smooth_follow(current.y, target.y, delta_secs, speed),
    )
}

/// Exponential follow with speed ramped up as the target gets farther away.
pub fn smooth_follow_vec2_distance_speed(
    current: Vec2,
    target: Vec2,
    delta_secs: f32,
    speed_near: f32,
    speed_far: f32,
    dist_near: f32,
    dist_far: f32,
) -> Vec2 {
    let dist = current.distance(target);
    if dist <= f32::EPSILON {
        return target;
    }
    let t = ((dist - dist_near) / (dist_far - dist_near)).clamp(0.0, 1.0);
    let blend = t * t * (3.0 - 2.0 * t);
    let speed = speed_near + (speed_far - speed_near) * blend;
    smooth_follow_vec2(current, target, delta_secs, speed)
}

const MENU_PANEL_START_SCALE: f32 = 0.94;
const MENU_PANEL_EXIT_SCALE: f32 = 0.84;

pub fn menu_panel_intro_transform(t: f32) -> Transform {
    let eased = ease_out_cubic(t);
    Transform {
        scale: Vec3::splat(MENU_PANEL_START_SCALE + (1.0 - MENU_PANEL_START_SCALE) * eased),
        ..default()
    }
}

/// Linear shrink — ease-out was slowing to a crawl while the overlay faded.
pub fn menu_panel_outro_transform(t: f32) -> Transform {
    let t = t.clamp(0.0, 1.0);
    Transform {
        scale: Vec3::splat(1.0 + (MENU_PANEL_EXIT_SCALE - 1.0) * t),
        ..default()
    }
}

#[derive(Resource, Clone)]
pub struct UiTheme {
    pub font: Handle<Font>,
    pub symbol_font: Handle<Font>,
}

impl UiTheme {
    pub fn load(asset_server: &AssetServer) -> Self {
        Self {
            font: asset_server.load(FONT_PATH),
            symbol_font: asset_server.load(SYMBOL_FONT_PATH),
        }
    }

    pub fn font(&self, size: f32) -> TextFont {
        TextFont {
            font: self.font.clone(),
            font_size: size,
            ..default()
        }
    }

    pub fn symbol_font(&self, size: f32) -> TextFont {
        TextFont {
            font: self.symbol_font.clone(),
            font_size: size,
            ..default()
        }
    }

    pub fn label(&self, text: impl Into<String>) -> impl Bundle {
        (
            Text::new(text),
            self.font(10.0),
            TextColor(GOLD),
        )
    }

    pub fn value(&self, text: impl Into<String>, size: f32) -> impl Bundle {
        (
            Text::new(text),
            self.font(size),
            TextColor(CREAM),
        )
    }

    pub fn hint(&self, text: impl Into<String>, size: f32) -> impl Bundle {
        (
            Text::new(text),
            self.font(size),
            TextColor(HINT),
        )
    }

    pub fn star(&self, size: f32) -> impl Bundle {
        (
            Text::new(STAR),
            self.symbol_font(size),
            TextColor(GOLD),
        )
    }
}

pub enum HudAnchor {
    TopLeft { left: f32, top: f32 },
    TopCenter { top: f32 },
    TopRight { right: f32, top: f32 },
    BottomLeft { left: f32, bottom: f32 },
}

impl HudAnchor {
    pub fn outer_node(self) -> Node {
        let mut node = Node {
            position_type: PositionType::Absolute,
            padding: UiRect::all(Val::Px(3.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        };
        match self {
            HudAnchor::TopLeft { left, top } => {
                node.left = Val::Px(left);
                node.top = Val::Px(top);
            }
            HudAnchor::TopCenter { top } => {
                node.left = Val::Px(0.0);
                node.right = Val::Px(0.0);
                node.top = Val::Px(top);
                node.width = Val::Percent(100.0);
                node.justify_content = JustifyContent::Center;
                node.align_items = AlignItems::Center;
            }
            HudAnchor::TopRight { right, top } => {
                node.right = Val::Px(right);
                node.top = Val::Px(top);
            }
            HudAnchor::BottomLeft { left, bottom } => {
                node.left = Val::Px(left);
                node.bottom = Val::Px(bottom);
            }
        }
        node
    }
}

/// Parchment outer frame + dark inner panel; `fill` builds panel contents.
pub fn spawn_framed_panel(
    commands: &mut Commands,
    theme: &UiTheme,
    anchor: HudAnchor,
    width: f32,
    padding: UiRect,
    row_gap: f32,
    fill: impl FnOnce(&mut ChildBuilder, &UiTheme),
) {
    commands
        .spawn((
            anchor.outer_node(),
            BackgroundColor(PARCHMENT),
            BorderColor(GOLD),
            Visibility::Hidden,
            GlobalZIndex(GLOBAL_Z_HUD),
            InGameHud,
        ))
        .with_children(|frame| {
            frame
                .spawn((
                    Node {
                        width: Val::Px(width),
                        flex_direction: FlexDirection::Column,
                        position_type: PositionType::Relative,
                        padding,
                        row_gap: Val::Px(row_gap),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor(GOLD_DIM),
                    BlocksWorldInput,
                    FocusPolicy::Block,
                    Interaction::None,
                ))
                .with_children(|panel| fill(panel, theme));
        });
}

fn menu_panel_node(width: f32) -> Node {
    Node {
        width: Val::Px(width),
        flex_direction: FlexDirection::Column,
        padding: UiRect::new(Val::Px(22.0), Val::Px(20.0), Val::Px(20.0), Val::Px(22.0)),
        row_gap: Val::Px(16.0),
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

pub fn menu_panel_bundle(width: f32) -> impl Bundle {
    (
        menu_panel_node(width),
        BackgroundColor(PANEL),
        BorderColor(GOLD_DIM),
    )
}

pub fn menu_panel_bundle_with_fade(width: f32) -> impl Bundle {
    (
        menu_panel_node(width),
        BackgroundColor(PANEL),
        BorderColor(GOLD_DIM),
        MenuFadeLayer { base: PANEL },
    )
}

pub fn menu_framed_overlay(
    parent: &mut ChildBuilder,
    theme: &UiTheme,
    fade: bool,
    fill: impl FnOnce(&mut ChildBuilder, &UiTheme),
) {
    let mut frame = parent.spawn((
        Node {
            padding: UiRect::all(Val::Px(3.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(PARCHMENT),
        BorderColor(GOLD),
    ));
    if fade {
        frame.insert(MenuFadeLayer {
            base: PARCHMENT,
        });
    }
    frame.with_children(|frame| fill(frame, theme));
}

pub fn spawn_ornate_divider(parent: &mut ChildBuilder, theme: &UiTheme, fade: bool) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                margin: UiRect::vertical(Val::Px(2.0)),
                ..default()
            },
        ))
        .with_children(|rule| {
            let mut left = rule.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(GOLD_DIM),
            ));
            if fade {
                left.insert(MenuFadeLayer { base: GOLD_DIM });
            }
            spawn_star_ornament(rule, theme, 13.0, fade);
            let mut right = rule.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(GOLD_DIM),
            ));
            if fade {
                right.insert(MenuFadeLayer { base: GOLD_DIM });
            }
        });
}

pub fn spawn_star_ornament(parent: &mut ChildBuilder, theme: &UiTheme, size: f32, fade: bool) {
    let mut star = parent.spawn(theme.star(size));
    if fade {
        star.insert(MenuFadeLayer { base: GOLD });
    }
}

pub fn spawn_star_watermark(parent: &mut ChildBuilder, theme: &UiTheme) {
    parent.spawn((
        Text::new(STAR),
        theme.symbol_font(22.0),
        TextColor(Color::srgba(0.78, 0.62, 0.34, 0.22)),
    ));
}

#[derive(Component)]
pub struct MenuButton;

#[derive(Component)]
pub struct MenuButtonFill;

pub fn menu_button_row_bundle() -> impl Bundle {
    menu_button_node_bundle(Val::Percent(100.0))
}

fn menu_button_node_bundle(width: Val) -> impl Bundle {
    (
        Button,
        Node {
            width,
            height: Val::Px(46.0),
            padding: UiRect::all(Val::Px(1.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(GOLD_DIM),
        MenuFadeLayer { base: GOLD_DIM },
    )
}

/// Cinzel sits high in its line box — nudge labels down for even vertical padding.
const MENU_BUTTON_LABEL_NUDGE: f32 = 3.0;

/// Label inside a menu button — clipped inner row keeps Cinzel's line box off the border edges.
pub fn spawn_menu_button_label(
    parent: &mut ChildBuilder,
    theme: &UiTheme,
    text: impl Into<String>,
    extra: impl Bundle,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            MenuButtonFill,
            BackgroundColor(BTN_IDLE),
            MenuFadeLayer { base: BTN_IDLE },
        ))
        .with_children(|inner| {
            inner
                .spawn((
                    Node {
                        margin: UiRect::new(
                            Val::Px(0.0),
                            Val::Px(0.0),
                            Val::Px(MENU_BUTTON_LABEL_NUDGE),
                            Val::Px(0.0),
                        ),
                        ..default()
                    },
                ))
                .with_children(|label| {
                    label.spawn((
                        theme.value(text, 17.0),
                        TextLayout::new_with_no_wrap(),
                        MenuFadeLayer { base: CREAM },
                        extra,
                    ));
                });
        });
}
