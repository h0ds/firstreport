//! `feed`: Kagami pillar 1. See `docs/plan.md`.

mod api;
mod bloom;
mod cli;
mod cluster;
mod config;
mod embed;
mod eval;
mod export;
mod fetch;
mod lang;
mod nightly;
mod parse;
mod poller;
mod prune;
mod replay;
mod snapshot;
mod stats;
mod store;
mod tooling;
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
        cli::Command::Export { format, out } => (|| {
            let format: export::Format = format.parse()?;
            let reader = store::Reader::open(&cfg.store.path)?;
            let s = export::run(
                &reader,
                format,
                &out,
                stats::now(),
                cfg.syndication.hamming_threshold,
            )?;
            info!(items = s.items, clusters = s.clusters, events = s.events, outlets = s.outlets, out = %out.display(), "export complete");
            Ok(())
        })(),
        cli::Command::Prune {
            older_than,
            embeddings,
        } => prune::run(&cfg, older_than.as_deref(), embeddings, stats::now()).map(|p| {
            info!(
                items = p.items,
                revisions = p.revisions,
                clusters = p.clusters,
                events = p.events,
                embeddings_cleared = p.embeddings_cleared,
                snapshot_files = p.snapshot_files,
                "prune complete"
            );
        }),
        cli::Command::Eval { pairs, json } => eval::run(&cfg, &pairs, json),
        cli::Command::Import { opml, write } => tooling::import_opml(
            &args.config_dir,
            &opml,
            &loaded.feeds,
            &loaded.outlets,
            write,
        ),
        cli::Command::Discover {
            url: Some(url),
            write,
            ..
        } => {
            tooling::discover(
                &args.config_dir,
                &url,
                &loaded.feeds,
                &loaded.outlets,
                write,
            )
            .await
        }
        cli::Command::Discover {
            hosts: Some(hosts),
            write,
            limit,
            max_per_host,
            ..
        } => {
            tooling::discover_hosts(
                &args.config_dir,
                &hosts,
                &loaded.feeds,
                &loaded.outlets,
                write,
                limit,
                max_per_host,
            )
            .await
        }
        cli::Command::Discover { .. } => Err("discover needs a page URL or --hosts <file>".into()),
        cli::Command::Replay { fixtures } => {
            let scratch = replay::default_scratch();
            let out = replay::run(cfg, &fixtures, &scratch).await;
            let _ = std::fs::remove_dir_all(&scratch);
            out.and_then(|r| {
                println!("{}", serde_json::to_string_pretty(&r)?);
                Ok(())
            })
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
    let (emb_tx, emb_rx) = tokio::sync::mpsc::channel::<embed::Embedded>(4096);

    // Clusterer: restore the checkpoint or rebuild from embedded items in
    // the window, then consume the embedder's output.
    let ckpt_path = cfg.store.path.with_file_name("clusterer.ckpt");
    let backlog = reader
        .with(|c| store::reader::embedded_items_since(c, boot_now - cfg.cluster.window_secs))?;
    let dim = backlog.first().map(|(_, v)| v.len());
    let (clusterer, rebuild_writes) = cluster::boot(&cfg, &ckpt_path, dim, backlog);
    for w in rebuild_writes {
        writer.send(w).await;
    }
    stats
        .active_clusters
        .store(clusterer.len() as u64, std::sync::atomic::Ordering::Relaxed);
    let cluster_task = tokio::spawn(
        cluster::Task {
            cfg: cfg.clone(),
            clusterer,
            writer: writer.clone(),
            stats: stats.clone(),
            ckpt_path,
        }
        .run(emb_rx, shutdown_rx.clone()),
    );

    // The embedder is optional at runtime: without a model, feed still
    // ingests and the backlog is picked up once `fetch-model` has run.
    let mut item_rx = Some(item_rx);
    let embed_task = if embed::available(&cfg) {
        match embed::load_from_config(&cfg, None) {
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
    let nightly_task = cfg.export.nightly.then(|| {
        tokio::spawn(nightly::run(
            cfg.clone(),
            reader.clone(),
            shutdown_rx.clone(),
        ))
    });

    let app = api::router(api::AppState {
        reader: reader.clone(),
        stats: stats.clone(),
        snapshot_dir: cfg.snapshots.dir.clone(),
        hamming_threshold: cfg.syndication.hamming_threshold,
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
    let _ = cluster_task.await;
    if let Some(t) = nightly_task {
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

    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let model_url = cfg.embed.model_url_for(os, arch);
    let runtime_url = cfg.embed.runtime_url_for(os, arch)?;
    info!(os, arch, %model_url, %runtime_url, "sources for this machine");

    for (path, url) in [
        (&cfg.embed.model_path, &model_url),
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
    let bytes = get(runtime_url).await?;
    let dir = lib.parent().unwrap_or(std::path::Path::new("."));
    let tgz = dir.join(if os == "windows" {
        "onnxruntime.zip"
    } else {
        "onnxruntime.tgz"
    });
    std::fs::write(&tgz, &bytes)?;
    if os == "windows" {
        // The release zip holds `<name>/lib/onnxruntime.dll`. Windows 10+
        // ships bsdtar as System32\tar.exe, which reads zip; name it
        // explicitly so a GNU tar earlier on PATH (Git Bash) is not used.
        let tar = std::env::var("SystemRoot")
            .map(|r| std::path::Path::new(&r).join("System32").join("tar.exe"))
            .unwrap_or_else(|_| "tar".into());
        let status = std::process::Command::new(tar)
            .arg("-xf")
            .arg(&tgz)
            .arg("-C")
            .arg(dir)
            .args(["--strip-components=2", "*/lib/onnxruntime.dll"])
            .status()?;
        let extracted = dir.join("onnxruntime.dll");
        if !status.success() || !extracted.exists() {
            return Err(format!("tar failed extracting {}", tgz.display()).into());
        }
        if extracted != *lib {
            std::fs::rename(&extracted, lib)?;
        }
        let _ = std::fs::remove_file(&tgz);
        info!(path = %lib.display(), "runtime installed");
        return Ok(());
    }
    // The release tarball holds `<name>/lib/libonnxruntime.so.<ver>` on
    // Linux or `<name>/lib/libonnxruntime.<ver>.dylib` on macOS, plus
    // symlinks. Extract just the versioned library and place it at `lib`.
    let pattern = if os == "macos" {
        "*/lib/libonnxruntime.*.dylib"
    } else {
        "*/lib/libonnxruntime.so.*"
    };
    let tgz_s = tgz.to_string_lossy().into_owned();
    let dir_s = dir.to_string_lossy().into_owned();
    let mut args = vec![
        "-xzf",
        tgz_s.as_str(),
        "-C",
        dir_s.as_str(),
        "--strip-components=2",
    ];
    if os != "macos" {
        args.push("--wildcards"); // GNU tar needs it; bsdtar globs by default
    }
    args.push(pattern);
    let status = std::process::Command::new("tar").args(&args).status()?;
    if !status.success() {
        return Err(format!("tar failed extracting {}", tgz.display()).into());
    }
    let versioned = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    n.starts_with("libonnxruntime.")
                        && (n.contains(".so.") || n.ends_with(".dylib"))
                })
                .unwrap_or(false)
                && p != lib
        })
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .ok_or("no versioned libonnxruntime in the tarball")?;
    std::fs::rename(&versioned, lib)?;
    let _ = std::fs::remove_file(&tgz);
    info!(path = %lib.display(), "runtime installed");
    Ok(())
}
