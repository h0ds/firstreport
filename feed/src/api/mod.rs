//! Read-only HTTP API (plan §5.5), versioned under `/v1`, plus `/debug`.

pub mod agents;
pub mod debug;
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
    /// Ledger base URL for outlet links; empty for none.
    pub ledger_url: String,
}

pub fn router(state: AppState) -> axum::Router {
    routes::router().with_state(state)
}
