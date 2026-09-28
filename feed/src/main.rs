//! `feed`: Kagami pillar 1. See `docs/plan.md`.

mod api;
mod bloom;
mod cli;
mod config;
mod embed;
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
        cli::Command::FetchModel => fetch_model(&cfg).await,
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
    let (item_tx, item_rx) = tokio::sync::mpsc::channel::<types::Item>(4096);
    // Embedded items destined for the clusterer (M3). Drained for now.
    let (emb_tx, mut emb_rx) = tokio::sync::mpsc::channel::<embed::Embedded>(4096);
    tokio::spawn(async move { while emb_rx.recv().await.is_some() {} });

    // The embedder is optional at runtime: without a model, feed still
    // ingests and the backlog is picked up once `fetch-model` has run.
    let mut item_rx = Some(item_rx);
    let embed_task = if cfg.embed.model_path.exists() && cfg.embed.runtime_lib.exists() {
        match embed::load_from_config(&cfg) {
            Ok(model) => {
                let rx = item_rx.take().expect("receiver taken once");
                let backlog = reader.with(|c| {
                    store::reader::unembedded_since(c, boot_now - cfg.cluster.window_secs, 100_000)
                })?;
                info!(
                    backlog = backlog.len(),
                    "embedder loaded; queueing unembedded items"
                );
                let tx = item_tx.clone();
                tokio::spawn(async move {
                    for it in backlog {
                        if tx.send(it).await.is_err() {
                            break;
                        }
                    }
                });
                Some(tokio::spawn(embed::run(
                    cfg.clone(),
                    model,
                    rx,
                    emb_tx,
                    writer.clone(),
                    stats.clone(),
                )))
            }
            Err(e) => {
                error!("embedder disabled: {e}");
                None
            }
        }
    } else {
        tracing::warn!(
            model = %cfg.embed.model_path.display(),
            runtime = %cfg.embed.runtime_lib.display(),
            "embedder disabled: model or runtime missing; run `feed fetch-model`"
        );
        None
    };
    if let Some(mut rx) = item_rx {
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
    }
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
    if let Some(t) = embed_task {
        let _ = t.await;
    }
    let _ = api_task.await;
    writer.flush().await;
    drop(writer);
    let _ = writer_thread.join();
    Ok(())
}

/// Download the model, its vocab, and the ONNX Runtime library into the
/// configured paths. Runtime-only: nothing here runs at build time.
async fn fetch_model(cfg: &config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent(fetch::client::USER_AGENT)
        .build()?;
    let get = |url: String| {
        let client = client.clone();
        async move {
            info!(%url, "downloading");
            let resp = client.get(&url).send().await?.error_for_status()?;
            resp.bytes().await
        }
    };
    let ensure = |p: &std::path::Path| {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok::<_, std::io::Error>(())
    };

    for (path, url) in [
        (&cfg.embed.model_path, &cfg.embed.model_url),
        (&cfg.embed.vocab_path, &cfg.embed.vocab_url),
    ] {
        if path.exists() {
            info!(path = %path.display(), "already present");
            continue;
        }
        ensure(path)?;
        let bytes = get(url.clone()).await?;
        std::fs::write(path, &bytes)?;
        info!(path = %path.display(), bytes = bytes.len(), "saved");
    }

    let lib = &cfg.embed.runtime_lib;
    if lib.exists() {
        info!(path = %lib.display(), "already present");
        return Ok(());
    }
    ensure(lib)?;
    let bytes = get(cfg.embed.runtime_url.clone()).await?;
    let dir = lib.parent().unwrap_or(std::path::Path::new("."));
    let tgz = dir.join("onnxruntime.tgz");
    std::fs::write(&tgz, &bytes)?;
    // The release tarball is `<name>/lib/libonnxruntime.so.<ver>` plus a
    // symlink. Extract just the versioned library and place it at `lib`.
    let status = std::process::Command::new("tar")
        .args([
            "-xzf",
            &tgz.to_string_lossy(),
            "-C",
            &dir.to_string_lossy(),
            "--wildcards",
            "--strip-components=2",
            "*/lib/libonnxruntime.so.*",
        ])
        .status()?;
    if !status.success() {
        return Err(format!("tar failed extracting {}", tgz.display()).into());
    }
    let versioned = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("libonnxruntime.so.") && !n.ends_with(".tgz"))
                .unwrap_or(false)
        })
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .ok_or("no libonnxruntime.so.* in the tarball")?;
    std::fs::rename(&versioned, lib)?;
    let _ = std::fs::remove_file(&tgz);
    info!(path = %lib.display(), "runtime installed");
    Ok(())
}
