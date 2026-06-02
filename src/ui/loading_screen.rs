use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::{BorderRect, TextureSlicer};
use bevy::ui::widget::{ImageNode, NodeImageMode};

use crate::app_state::LoadingProgress;
use crate::ui::{
    menu_framed_overlay, menu_panel_bundle, spawn_ornate_divider, UiTheme, GOLD, GOLD_DIM,
};

/// Opaque backdrop — loading must fully hide the world until complete.
const LOADING_BACKDROP: Color = Color::srgb(0.02, 0.02, 0.03);

/// How fast the visible bar creeps toward `bar_cap` (fraction per second).
const LOADING_BAR_FILL_RATE: f32 = 0.72;

#[derive(Component)]
pub struct LoadingRoot;

#[derive(Component)]
pub struct LoadingStatusText;

#[derive(Component)]
pub struct LoadingBarFill;

const LOADING_BAR_TRACK: f32 = 12.0;
const LOADING_BAR_PAD: f32 = 2.0;
const LOADING_BAR_BORDER: f32 = 1.0;
/// Solid ring between the gold border and the gradient track (no gradient here).
const LOADING_BAR_GUTTER: Color = Color::srgb(0.02, 0.02, 0.03);
const LOADING_BAR_OUTER_HEIGHT: f32 =
    LOADING_BAR_TRACK + 2.0 * LOADING_BAR_PAD + 2.0 * LOADING_BAR_BORDER;
const LOADING_BAR_FRAME_TEX_WIDTH: u32 = 3;
const LOADING_BAR_GRADIENT_TEX_WIDTH: u32 = 256;
/// Nudge the bar slightly above vertical centre in the panel column.
const LOADING_BAR_MARGIN_TOP: f32 = 5.0;
const LOADING_BAR_MARGIN_BOTTOM: f32 = 11.0;

pub fn spawn_loading_screen(
    mut commands: Commands,
    theme: Res<UiTheme>,
    mut images: ResMut<Assets<Image>>,
) {
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
            BackgroundColor(LOADING_BACKDROP),
            LoadingRoot,
        ))
        .with_children(|overlay| {
            menu_framed_overlay(overlay, &theme, false, |frame, theme| {
                frame.spawn(menu_panel_bundle(360.0)).with_children(|panel| {
                    panel.spawn(theme.value("MYARI", 28.0));
                    spawn_ornate_divider(panel, theme, false);
                    panel.spawn((theme.value("Preparing…", 18.0), LoadingStatusText));
                    spawn_loading_bar(panel, &mut images);
                });
            });
        });
}

fn spawn_loading_bar(parent: &mut bevy::hierarchy::ChildBuilder, images: &mut Assets<Image>) {
    let frame = images.add(make_loading_bar_frame_image());
    let gradient = images.add(make_loading_bar_gradient_image());
    let track_inset = LOADING_BAR_BORDER + LOADING_BAR_PAD;

    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(LOADING_BAR_OUTER_HEIGHT),
                margin: UiRect::new(
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(LOADING_BAR_MARGIN_TOP),
                    Val::Px(LOADING_BAR_MARGIN_BOTTOM),
                ),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(LOADING_BAR_GUTTER),
        ))
        .with_children(|shell| {
            shell.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
                ImageNode {
                    image: frame,
                    image_mode: NodeImageMode::Sliced(TextureSlicer {
                        border: BorderRect::square(LOADING_BAR_BORDER),
                        ..default()
                    }),
                    ..default()
                },
            ));
            // Solid gutter inside the border (the blue-highlighted ring in your screenshot).
            shell.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(LOADING_BAR_BORDER),
                    right: Val::Px(LOADING_BAR_BORDER),
                    top: Val::Px(LOADING_BAR_BORDER),
                    bottom: Val::Px(LOADING_BAR_BORDER),
                    ..default()
                },
                BackgroundColor(LOADING_BAR_GUTTER),
            ));
            shell
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(track_inset),
                        right: Val::Px(track_inset),
                        top: Val::Px(track_inset),
                        bottom: Val::Px(track_inset),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        ImageNode {
                            image: gradient,
                            image_mode: NodeImageMode::Stretch,
                            ..default()
                        },
                    ));
                    track.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            width: Val::Percent(0.0),
                            ..default()
                        },
                        BackgroundColor(GOLD),
                        LoadingBarFill,
                    ));
                });
        });
}

/// Border-only 9-slice (transparent centre). Solid fill comes from the shell background.
fn make_loading_bar_frame_image() -> Image {
    let w = LOADING_BAR_FRAME_TEX_WIDTH;
    let h = LOADING_BAR_OUTER_HEIGHT as u32;
    let border = color_to_rgba8(GOLD_DIM);
    let transparent = [0u8, 0, 0, 0];
    let mut data = Vec::with_capacity((w * h * 4) as usize);

    for y in 0..h {
        for x in 0..w {
            let px = if y == 0 || y == h - 1 || x == 0 || x == w - 1 {
                border
            } else {
                transparent
            };
            data.extend_from_slice(&px);
        }
    }

    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Track background: black on the left → gold on the right.
fn make_loading_bar_gradient_image() -> Image {
    let w = LOADING_BAR_GRADIENT_TEX_WIDTH;
    let h = LOADING_BAR_TRACK as u32;
    let left = color_to_rgba8(LOADING_BAR_GUTTER);
    let right = color_to_rgba8(GOLD);
    let mut data = Vec::with_capacity((w * h * 4) as usize);

    for y in 0..h {
        let _ = y;
        for x in 0..w {
            let t = x as f32 / (w - 1).max(1) as f32;
            let px = lerp_rgba8(left, right, t);
            data.extend_from_slice(&px);
        }
    }

    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn lerp_rgba8(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t).round() as u8,
    ]
}

fn color_to_rgba8(color: Color) -> [u8; 4] {
    let c = color.to_srgba();
    [
        (c.red * 255.0).round() as u8,
        (c.green * 255.0).round() as u8,
        (c.blue * 255.0).round() as u8,
        (c.alpha * 255.0).round() as u8,
    ]
}

pub fn sync_loading_ui(
    time: Res<Time>,
    mut progress: ResMut<LoadingProgress>,
    mut status: Query<&mut Text, With<LoadingStatusText>>,
    mut bar: Query<&mut Node, With<LoadingBarFill>>,
) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        progress.bar_display = (progress.bar_display + LOADING_BAR_FILL_RATE * dt)
            .min(progress.bar_cap);
    }

    if progress.is_changed() {
        if let Ok(mut text) = status.get_single_mut() {
            text.0 = progress.status.clone();
        }
    }
    if let Ok(mut node) = bar.get_single_mut() {
        node.width = Val::Percent((progress.bar_display * 100.0).clamp(0.0, 100.0));
    }
}
