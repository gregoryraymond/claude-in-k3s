// Library root for integration/feature tests to access internal modules.

pub mod app;
pub mod config;
pub mod deps;
pub mod docker;
pub mod error;
pub mod health;
pub mod helm;
pub mod kubectl;
pub mod lockfile;
#[allow(dead_code)]
pub mod orchestrator;
pub mod platform;
pub mod progress;
pub mod projects;
pub mod recovery;
pub mod state;
#[cfg(test)]
pub mod test_utils;
pub mod terraform;
pub mod tray;
