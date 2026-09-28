//! `feed`: Kagami pillar 1. See `docs/plan.md`.

// M0: the data contract is defined before anything consumes it. Remove once M1 lands.
#![allow(dead_code)]

mod cli;
mod config;
mod types;

use tracing::{error, info};

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();
}

#[tokio::main]
async fn main() {
    init_tracing();
    let args = match cli::parse(std::env::args()) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let loaded = match config::load(&args.config_dir) {
        Ok(l) => l,
        Err(e) => {
            error!("config: {e}");
            std::process::exit(1);
        }
    };

    let mut cfg = loaded.config;
    if let Some(b) = args.bind {
        cfg.api.bind = b;
    }

    info!(
        contract = types::CONTRACT_VERSION,
        feeds = loaded.feeds.feeds.len(),
        outlets = loaded.outlets.outlets.len(),
        bind = %cfg.api.bind,
        "feed starting"
    );

    match args.command {
        cli::Command::Run => {
            // M0: the pipeline tasks (poller, embedder, clusterer, store, api)
            // are not wired yet. Start, report, and exit cleanly on ctrl-c.
            info!("M0 skeleton: no pipeline tasks yet; press ctrl-c to exit");
            let _ = tokio::signal::ctrl_c().await;
            info!("shutting down");
        }
        cli::Command::FetchModel => info!("fetch-model: not implemented (M2)"),
        cli::Command::Export { format, out } => {
            info!(%format, out = %out.display(), "export: not implemented (M6)")
        }
        cli::Command::Prune { older_than } => info!(%older_than, "prune: not implemented (M6)"),
        cli::Command::Replay { fixtures } => {
            info!(fixtures = %fixtures.display(), "replay: not implemented (M3)")
        }
    }
}
