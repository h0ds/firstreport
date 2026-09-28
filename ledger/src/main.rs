//! `ledger`: Kagami pillar 2. See `docs/plan.md`.

mod api;
mod cli;
mod export;
mod model;
mod store;

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
        cli::Command::Import { source, .. } => {
            Err(format!("no importer for {source:?} yet (L1: wikidata)").into())
        }
    }
}
