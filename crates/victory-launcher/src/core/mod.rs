//! "Core" half of the launcher: play, settings, presets, flags, profiles, bug report. Owned by the core worker.
pub mod about;
pub mod flags;
pub mod play;
pub mod report;
pub mod settings;

#[derive(Default)]
pub struct CoreState {}
