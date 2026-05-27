//! Hover-panel terrain swatch — even gold frame via 9-slice, centred inner tile.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::{BorderRect, TextureSlicer};
use bevy::ui::widget::{ImageNode, NodeImageMode};

use super::{GEM_FRAME, GOLD};

pub const HOVER_GEM_SIZE: f32 = 34.0;
const HOVER_GEM_BORDER: f32 = 1.0;
const HOVER_GEM_FRAME_TEX: u32 = 3;
const HOVER_SWATCH_OUTER: f32 = 16.0;
const HOVER_SWATCH_SILVER: Color = Color::srgba(1.0, 0.95, 0.75, 0.45);
pub const HOVER_SWATCH_TERRAIN: Color = Color::srgb(0.35, 0.38, 0.45);

#[derive(Component)]
pub struct HoverSwatch;

fn color_to_rgba8(color: Color) -> [u8; 4] {
    let c = color.to_srgba();
    [
        (c.red * 255.0).round() as u8,
        (c.green * 255.0).round() as u8,
        (c.blue * 255.0).round() as u8,
        (c.alpha * 255.0).round() as u8,
    ]
}

fn make_hover_gem_frame_image() -> Image {
    let w = HOVER_GEM_FRAME_TEX;
    let h = HOVER_GEM_SIZE as u32;
    let border = color_to_rgba8(GOLD);
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

pub fn spawn_hover_gem_swatch(
    parent: &mut bevy::hierarchy::ChildBuilder<'_>,
    images: &mut Assets<Image>,
) {
    let frame = images.add(make_hover_gem_frame_image());

    parent
        .spawn((
            Node {
                width: Val::Px(HOVER_GEM_SIZE),
                height: Val::Px(HOVER_GEM_SIZE),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(GEM_FRAME),
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
                        border: BorderRect::square(HOVER_GEM_BORDER),
                        ..default()
                    }),
                    ..default()
                },
            ));
            shell
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(HOVER_GEM_BORDER),
                        right: Val::Px(HOVER_GEM_BORDER),
                        top: Val::Px(HOVER_GEM_BORDER),
                        bottom: Val::Px(HOVER_GEM_BORDER),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(GEM_FRAME),
                ))
                .with_children(|gutter| {
                    gutter
                        .spawn((
                            Node {
                                width: Val::Px(HOVER_SWATCH_OUTER),
                                height: Val::Px(HOVER_SWATCH_OUTER),
                                padding: UiRect::all(Val::Px(1.0)),
                                overflow: Overflow::clip(),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(HOVER_SWATCH_SILVER),
                        ))
                        .with_children(|silver| {
                            silver.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(HOVER_SWATCH_TERRAIN),
                                HoverSwatch,
                            ));
                        });
                });
        });
}
