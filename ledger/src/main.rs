//! `ledger`: Kagami pillar 2. See `docs/plan.md`.

mod api;
mod cli;
mod export;
mod model;
mod store;
mod wikidata;

use std::sync::Arc;

use tracing::{error, info, warn};

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
    if let Err(e) = run(args).await {
        error!("{e}");
        std::process::exit(1);
    }
}

async fn run(args: cli::Args) -> Result<(), Box<dyn std::error::Error>> {
    let ledger = store::load(&args.data_dir)?;
    let outlet_ids: Option<Vec<String>> = match &args.outlets {
        Some(p) => Some(
            store::load_feed_outlets(p)?
                .into_iter()
                .map(|o| o.id)
                .collect(),
        ),
        None => None,
    };
    info!(
        contract = model::CONTRACT_VERSION,
        sources = ledger.sources.len(),
        outlets = ledger.claims.len(),
        claims = ledger.claims.values().map(Vec::len).sum::<usize>(),
        "ledger loaded"
    );

    match args.command {
        cli::Command::Check => {
            let findings = store::check(&ledger, outlet_ids.as_deref());
            let errors = findings.iter().filter(|f| f.is_error()).count();
            for f in &findings {
                println!("{f}");
            }
            if outlet_ids.is_none() {
                warn!("no --outlets given; outlet ids were not checked against feed");
            }
            println!("{} findings, {} errors", findings.len(), errors);
            if errors > 0 {
                return Err(format!("{errors} errors").into());
            }
            Ok(())
        }
        cli::Command::Serve => {
            let findings = store::check(&ledger, outlet_ids.as_deref());
            let errors: Vec<_> = findings.iter().filter(|f| f.is_error()).collect();
            if !errors.is_empty() {
                for f in &errors {
                    error!("{f}");
                }
                return Err("refusing to serve a ledger with errors; run `ledger check`".into());
            }
            let bind = args.bind.unwrap_or_else(|| "127.0.0.1:8081".into());
            let app = api::router(Arc::new(ledger));
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            info!(%bind, "ledger api listening");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            Ok(())
        }
        cli::Command::Export { out } => {
            let (c, s, o) = export::write(&ledger, &out)?;
            info!(claims = c, sources = s, outlets = o, out = %out.display(), "export complete");
            Ok(())
        }
        cli::Command::Import {
            source,
            only,
            write,
        } => {
            if source != "wikidata" {
                return Err(format!("no importer for {source:?}; available: wikidata").into());
            }
            let outlets_path = args.outlets.as_ref().ok_or(
                "import needs --outlets <feed outlets.toml> to know which outlets to look up",
            )?;
            let mut outlets = store::load_feed_outlets(outlets_path)?;
            if let Some(id) = &only {
                outlets.retain(|o| &o.id == id);
                if outlets.is_empty() {
                    return Err(format!("outlet {id:?} not in {}", outlets_path.display()).into());
                }
            }
            if !ledger.sources.iter().any(|s| s.id == wikidata::SOURCE_ID) {
                return Err("sources.toml has no \"wikidata\" entry".into());
            }
            import_wikidata(&args.data_dir, &ledger, &outlets, write).await
        }
    }
}

async fn import_wikidata(
    data_dir: &std::path::Path,
    ledger: &store::Ledger,
    outlets: &[store::FeedOutlet],
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = wikidata::client()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut added = 0;
    let mut matched = 0;
    for o in outlets {
        if o.domains.is_empty() {
            warn!(outlet = %o.id, "no domains; skipped");
            continue;
        }
        let body = match wikidata::fetch(&client, o).await {
            Ok(b) => b,
            Err(e) => {
                warn!(outlet = %o.id, "wikidata query failed: {e}");
                continue;
            }
        };
        let fresh = wikidata::parse_response(&o.id, &body, now)?;
        if fresh.is_empty() {
            info!(outlet = %o.id, name = %o.name, domains = ?o.domains, "no wikidata item matches its domains");
            continue;
        }
        matched += 1;
        added += fresh.len();
        let problems: Vec<_> = fresh
            .iter()
            .flat_map(|c| model::validate_claim(c, &ledger.sources, None))
            .filter(|f| f.is_error())
            .collect();
        if !problems.is_empty() {
            for p in &problems {
                error!(outlet = %o.id, "{p}");
            }
            continue;
        }
        if write {
            let existing = ledger.claims.get(&o.id).cloned().unwrap_or_default();
            let merged = store::merge_source(&existing, wikidata::SOURCE_ID, fresh);
            store::write_outlet(data_dir, &o.id, merged)?;
            info!(outlet = %o.id, "written");
        } else {
            println!("# {}", o.id);
            for c in &fresh {
                println!("  {:<12} {}  ({})", c.field, c.value, c.ref_url);
            }
        }
        // Be a polite SPARQL client: one query a second is plenty.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    info!(
        matched,
        claims = added,
        outlets = outlets.len(),
        written = write,
        "wikidata import done"
    );
    if !write {
        println!("# re-run with --write to store these claims");
    }
    Ok(())
}
