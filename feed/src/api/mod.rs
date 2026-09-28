//! Read-only HTTP API (plan §5.5), versioned under `/v1`. M1 exposes items,
//! snapshots, outlets and stats; clusters arrive with M3.

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
}

pub fn router(state: AppState) -> axum::Router {
    routes::router().with_state(state)
}
