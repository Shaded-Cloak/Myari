use bevy::prelude::*;

use crate::app_state::{AppState, LoadingJob, LoadingProgress};
use crate::{GridToggle, GridToggleLabel};
use crate::ui::{
    menu_button_row_bundle, menu_framed_overlay, menu_panel_bundle, spawn_menu_button_label,
    spawn_ornate_divider, MenuButton, UiTheme, GOLD_DIM,
};

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

pub fn spawn_title_menu(mut commands: Commands, theme: Res<UiTheme>) {
    let has_save = save_file_exists();

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            TitleRoot,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((Node::default(), TitleHomePanel, ZIndex(1)))
                .with_children(|home| {
                    menu_framed_overlay(home, &theme, |frame, theme| {
                        frame.spawn(menu_panel_bundle(340.0)).with_children(|panel| {
                            panel.spawn(theme.value("MYARI", 36.0));
                            spawn_ornate_divider(panel, theme);
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    TitlePlayButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Play", ());
                                });
                            spawn_continue_button(panel, theme, has_save);
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    TitleSettingsButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Settings", ());
                                });
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    TitleQuitButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Quit", ());
                                });
                        });
                    });
                });

            overlay
                .spawn((
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    TitleSettingsPanel,
                    ZIndex(1),
                ))
                .with_children(|settings| {
                    menu_framed_overlay(settings, &theme, |frame, theme| {
                        frame.spawn(menu_panel_bundle(320.0)).with_children(|panel| {
                            panel.spawn(theme.value("SETTINGS", 26.0));
                            spawn_ornate_divider(panel, theme);
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    GridToggle,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(
                                        btn,
                                        theme,
                                        "Grid: OFF",
                                        GridToggleLabel,
                                    );
                                });
                            panel
                                .spawn((
                                    menu_button_row_bundle(),
                                    MenuButton,
                                    TitleBackButton,
                                ))
                                .with_children(|btn| {
                                    spawn_menu_button_label(btn, theme, "Back", ());
                                });
                            panel.spawn(theme.hint("Grid applies in-game", 12.0));
                        });
                    });
                });
        });

}

fn spawn_continue_button(parent: &mut bevy::hierarchy::ChildBuilder, theme: &UiTheme, enabled: bool) {
    if enabled {
        parent
            .spawn((
                menu_button_row_bundle(),
                MenuButton,
                TitleContinueButton,
            ))
            .with_children(|btn| {
                spawn_menu_button_label(btn, theme, "Continue", ());
            });
    } else {
        parent
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(46.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.08, 0.06, 0.45)),
                BorderColor(GOLD_DIM),
            ))
            .with_children(|btn| {
                spawn_menu_button_label(btn, theme, "Continue", ());
            });
        parent.spawn((
            theme.hint("No save found", 11.0),
            TitleContinueHint,
        ));
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
