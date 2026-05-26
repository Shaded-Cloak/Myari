use bevy::prelude::*;

use crate::app_state::LoadingProgress;
use crate::ui::{
    menu_framed_overlay, menu_panel_bundle, spawn_ornate_divider, spawn_star_watermark, UiTheme,
    GOLD, GOLD_DIM, MENU_BACKDROP, PANEL,
};

#[derive(Component)]
pub struct LoadingRoot;

#[derive(Component)]
pub struct LoadingStatusText;

#[derive(Component)]
pub struct LoadingBarFill;

pub fn spawn_loading_screen(mut commands: Commands, theme: Res<UiTheme>) {
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
            BackgroundColor(MENU_BACKDROP),
            LoadingRoot,
        ))
        .with_children(|overlay| {
            menu_framed_overlay(overlay, &theme, |frame, theme| {
                frame.spawn(menu_panel_bundle(360.0)).with_children(|panel| {
                    panel.spawn(theme.value("MYARI", 28.0));
                    spawn_ornate_divider(panel, theme);
                    panel.spawn((theme.value("Preparing…", 18.0), LoadingStatusText));
                    panel
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(12.0),
                                margin: UiRect::vertical(Val::Px(8.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                padding: UiRect::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(PANEL),
                            BorderColor(GOLD_DIM),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                Node {
                                    width: Val::Percent(10.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(GOLD),
                                LoadingBarFill,
                            ));
                        });
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
                });
            });
        });
}

pub fn sync_loading_ui(
    progress: Res<LoadingProgress>,
    mut status: Query<&mut Text, With<LoadingStatusText>>,
    mut bar: Query<&mut Node, With<LoadingBarFill>>,
) {
    if !progress.is_changed() {
        return;
    }
    if let Ok(mut text) = status.get_single_mut() {
        text.0 = progress.status.clone();
    }
    if let Ok(mut node) = bar.get_single_mut() {
        node.width = Val::Percent((progress.bar * 100.0).clamp(0.0, 100.0));
    }
}
