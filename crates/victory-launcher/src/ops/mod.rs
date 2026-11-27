//! "Ops" half of the launcher: monitor, overlay, updates, developer tools. Owned by the ops worker.
pub mod dev;
pub mod monitor;
pub mod updates;

#[derive(Default)]
pub struct OpsState {}
