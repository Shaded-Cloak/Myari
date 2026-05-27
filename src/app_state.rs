use bevy::prelude::*;

#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AppState {
    #[default]
    MainMenu,
    Loading,
    InGame,
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum LoadingJob {
    #[default]
    None,
    NewGame,
    Continue,
}

#[derive(Resource, Default)]
pub struct LoadingProgress {
    pub step: u8,
    /// Total time spent in the loading state.
    pub timer: f32,
    /// Time spent in the current pipeline step.
    pub phase_timer: f32,
    /// Maximum fill (0–1) unlocked so far; rises gradually per phase.
    pub bar_cap: f32,
    /// Value drawn on the loading bar; creeps toward `bar_cap` each frame.
    pub bar_display: f32,
    pub status: String,
    pub work_done: bool,
    pub ui_done: bool,
}

impl LoadingProgress {
    pub fn reset_for_job(job: LoadingJob) -> Self {
        let status = match job {
            LoadingJob::NewGame => "Preparing…",
            LoadingJob::Continue => "Preparing…",
            LoadingJob::None => "Preparing…",
        };
        Self {
            step: 0,
            timer: 0.0,
            phase_timer: 0.0,
            bar_cap: 0.05,
            bar_display: 0.0,
            status: status.to_string(),
            work_done: false,
            ui_done: false,
        }
    }
}

/// In-game HUD panels and world-adjacent UI hidden on title / loading screens.
#[derive(Component)]
pub struct InGameHud;
