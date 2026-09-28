//! `feed`: Kagami pillar 1. See `docs/plan.md`.

// Contract types and helpers land ahead of their consumers (M2+). Remove once clustering is wired.
#![allow(dead_code)]

mod api;
mod bloom;
mod cli;
mod config;
mod fetch;
mod lang;
mod parse;
mod poller;
mod snapshot;
mod stats;
mod store;
mod types;

#[cfg(test)]
mod pipeline_test;

use std::sync::Arc;

use tracing::{error, info};

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
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

    let result = match args.command {
        cli::Command::Run => run(cfg, loaded.feeds, loaded.outlets).await,
        cli::Command::FetchModel => {
            info!("fetch-model: not implemented (M2)");
            Ok(())
        }
        cli::Command::Export { format, out } => {
            info!(%format, out = %out.display(), "export: not implemented (M6)");
            Ok(())
        }
        cli::Command::Prune { older_than } => {
            info!(%older_than, "prune: not implemented (M6)");
            Ok(())
        }
        cli::Command::Replay { fixtures } => {
            info!(fixtures = %fixtures.display(), "replay: not implemented (M3)");
            Ok(())
        }
    };
    if let Err(e) = result {
        error!("{e}");
        std::process::exit(1);
    }
}

async fn run(
    cfg: config::Config,
    feeds: config::Feeds,
    outlets: config::Outlets,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Arc::new(cfg);
    let stats = Arc::new(stats::Stats::default());
    stats
        .started_at
        .store(stats::now() as u64, std::sync::atomic::Ordering::Relaxed);

    // Store: writer thread plus a reader connection. Outlets are synced
    // synchronously before anything else touches the database.
    let (writer, writer_thread) = store::spawn_writer(&cfg.store.path)?;
    let reader = store::Reader::open(&cfg.store.path)?;
    {
        let mut conn = store::open(&cfg.store.path)?;
        store::writer::sync_outlets(&mut conn, &outlets)?;
    }
    let boot_now = stats::now();
    let persisted = reader.with(store::reader::feed_states)?;
    let recent_ids =
        reader.with(|c| store::reader::item_ids_since(c, boot_now - poller::BLOOM_WINDOW_SECS))?;
    let states =
        poller::Poller::build_states(&feeds, persisted, cfg.poll.default_interval_secs, boot_now);
    for s in &states {
        writer.send(store::WriteMsg::FeedState(s.clone())).await;
    }

    let resolver = Arc::new(parse::outlet::Resolver::from_config(&outlets));
    let mut poller = poller::Poller::new(
        cfg.clone(),
        resolver,
        reader.clone(),
        writer.clone(),
        stats.clone(),
    )?;
    poller.seed_bloom(&recent_ids);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    // Items destined for the embedder (M2). Dropped on the floor for now.
    let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<types::Item>(1024);
    tokio::spawn(async move { while item_rx.recv().await.is_some() {} });
    let poller_task = tokio::spawn(poller.run(states, item_tx, shutdown_rx.clone()));

    let app = api::router(api::AppState {
        reader: reader.clone(),
        stats: stats.clone(),
        snapshot_dir: cfg.snapshots.dir.clone(),
    });
    let listener = tokio::net::TcpListener::bind(&cfg.api.bind).await?;
    info!(bind = %cfg.api.bind, "api listening");
    let mut api_rx = shutdown_rx.clone();
    let api_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = api_rx.changed().await;
            })
            .await
    });

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    let _ = shutdown_tx.send(true);
    let _ = poller_task.await;
    let _ = api_task.await;
    writer.flush().await;
    drop(writer);
    let _ = writer_thread.join();
    Ok(())
}
