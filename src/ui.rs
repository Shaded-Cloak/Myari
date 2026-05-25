//! Shared fantasy HUD styling — Cinzel font, parchment frame, gold labels.

use bevy::hierarchy::ChildBuilder;
use bevy::prelude::*;

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
    TopRight { right: f32, top: f32 },
    BottomLeft { left: f32, bottom: f32 },
}

impl HudAnchor {
    fn outer_node(self) -> Node {
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
                ))
                .with_children(|panel| fill(panel, theme));
        });
}

pub fn spawn_ornate_divider(parent: &mut ChildBuilder, theme: &UiTheme) {
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
            rule.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(GOLD_DIM),
            ));
            spawn_star_ornament(rule, theme, 13.0);
            rule.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(GOLD_DIM),
            ));
        });
}

pub fn spawn_star_ornament(parent: &mut ChildBuilder, theme: &UiTheme, size: f32) {
    parent.spawn(theme.star(size));
}

pub fn spawn_star_watermark(parent: &mut ChildBuilder, theme: &UiTheme) {
    parent.spawn((
        Text::new(STAR),
        theme.symbol_font(22.0),
        TextColor(Color::srgba(0.78, 0.62, 0.34, 0.22)),
    ));
}

pub fn menu_button_bundle() -> impl Bundle {
    (
        Button,
        Node {
            padding: UiRect::new(Val::Px(14.0), Val::Px(18.0), Val::Px(10.0), Val::Px(10.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(BTN_IDLE),
        BorderColor(GOLD_DIM),
    )
}
