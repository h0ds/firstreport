//! `ledger`: Kagami pillar 2. See `docs/plan.md`.

mod api;
mod cli;
mod export;
mod model;
mod perennial;
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
            let source_id = match source.as_str() {
                "wikidata" => wikidata::SOURCE_ID,
                "wikipedia_rsp" | "perennial" => perennial::SOURCE_ID,
                other => {
                    return Err(format!(
                        "no importer for {other:?}; available: wikidata, wikipedia_rsp"
                    )
                    .into())
                }
            };
            if !ledger.sources.iter().any(|s| s.id == source_id) {
                return Err(format!("sources.toml has no {source_id:?} entry").into());
            }
            if source_id == wikidata::SOURCE_ID {
                import_wikidata(&args.data_dir, &ledger, &outlets, write).await
            } else {
                import_perennial(&args.data_dir, &ledger, &outlets, write).await
            }
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

/// Wikipedia's perennial sources list: one page fetch, then match every
/// outlet against its rows. Uses the outlet's `wikipedia` claims from a
/// prior Wikidata import for the strongest match, so run that first.
async fn import_perennial(
    data_dir: &std::path::Path,
    ledger: &store::Ledger,
    outlets: &[store::FeedOutlet],
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = perennial::client()?;
    let html = perennial::fetch(&client).await?;
    let rows = perennial::parse(&html)?;
    info!(rows = rows.len(), "perennial sources table parsed");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut matched_outlets = 0;
    let mut total = 0;
    for o in outlets {
        let existing = ledger.claims.get(&o.id).cloned().unwrap_or_default();
        let articles: Vec<String> = existing
            .iter()
            .filter(|c| c.field == "wikipedia" && c.source_id == wikidata::SOURCE_ID)
            .map(|c| c.value.clone())
            .collect();
        let m = perennial::matches(&rows, o, &articles);
        if m.is_empty() {
            continue;
        }
        let fresh = perennial::claims(&o.id, &m, now);
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
        matched_outlets += 1;
        total += fresh.len();
        if write {
            let merged = store::merge_source(&existing, perennial::SOURCE_ID, fresh);
            store::write_outlet(data_dir, &o.id, merged)?;
            info!(outlet = %o.id, "written");
        } else {
            println!("# {}", o.id);
            for c in &fresh {
                let scope = c
                    .data
                    .as_ref()
                    .and_then(|d| d.get("scope"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                println!(
                    "  reliability  {:<22} {}  [{}]",
                    c.value,
                    c.ref_url,
                    scope.chars().take(60).collect::<String>()
                );
                if let Some(n) = &c.note {
                    println!("    note: {n}");
                }
            }
        }
    }
    info!(
        matched_outlets,
        claims = total,
        outlets = outlets.len(),
        written = write,
        "perennial import done"
    );
    if !write {
        println!("# re-run with --write to store these claims");
    }
    Ok(())
}
