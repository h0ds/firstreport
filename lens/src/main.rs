//! `lens`: pillar 3. A static site generated from `feed` and `ledger`
//! exports. It arranges what the other two record and adds nothing:
//! no score, no lean colour, no summary in its own voice. Plan in
//! `docs/plan.md`.

mod cli;
mod derive;
mod input;

fn main() {
    if let Err(e) = run() {
        eprintln!("lens: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match cli::parse(std::env::args())? {
        cli::Command::Build {
            feed_export,
            ledger_export,
            ..
        } => {
            let input = input::read(&feed_export, &ledger_export)?;
            println!(
                "read {} outlets, {} items, {} clusters, {} events, {} sources, {} outlets with claims; export time {}",
                input.outlets.len(),
                input.items.len(),
                input.clusters.len(),
                input.events.len(),
                input.sources.len(),
                input.claims.len(),
                input.export_time
            );
            let facts = derive::outlet_facts(&input, 30 * derive::DAY);
            for (id, f) in &facts {
                println!(
                    "  {id}: {} stories, first to {} of {} shared, {} copies, {} copied",
                    f.clusters.len(),
                    f.first_to,
                    f.shared_stories,
                    f.copies,
                    f.copied
                );
            }
            if let Some(c) = input.clusters.values().max_by_key(|c| c.last_active_at) {
                let f = derive::cluster_facts(c, &input.events, input.export_time);
                println!(
                    "  latest story {}: {} outlets in 24 h, {} copies, {} revisions, spark {:?}",
                    c.id,
                    f.outlets_24h,
                    f.copies_of.len(),
                    f.revisions.len(),
                    f.spark
                );
            }
            Err("build: pages are not written yet (N1)".into())
        }
        cli::Command::Serve { .. } => Err("serve: not written yet (N1)".into()),
        cli::Command::Check { .. } => Err("check: not written yet (N1)".into()),
    }
}
