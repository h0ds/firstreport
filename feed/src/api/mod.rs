//! Read-only HTTP API (plan §5.5), versioned under `/v1`, plus `/debug`.

pub mod agents;
pub mod debug;
pub mod network;
pub mod openapi;
pub mod routes;

use std::path::PathBuf;
use std::sync::Arc;

use crate::stats::Stats;
use crate::store::Reader;

#[derive(Clone)]
pub struct AppState {
    pub reader: Reader,
    pub stats: Arc<Stats>,
    pub snapshot_dir: PathBuf,
    /// Simhash distance for syndication grouping.
    pub hamming_threshold: u32,
    /// Clustering join threshold, reported in each cluster's confidence.
    pub join_threshold: f32,
    /// Ledger base URL for outlet links; empty for none.
    pub ledger_url: String,
    /// Feed URLs in the loaded feeds.toml. The store also keeps rows for
    /// feeds that were since removed or moved; `/v1/feeds` marks which is which.
    pub configured_feeds: std::sync::Arc<std::collections::HashSet<String>>,
}

pub fn router(state: AppState) -> axum::Router {
    routes::router().with_state(state)
}
