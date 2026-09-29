//! `lens`: pillar 3. A static site generated from `feed` and `ledger`
//! exports. It arranges what the other two record and adds nothing:
//! no score, no lean colour, no summary in its own voice. Plan in
//! `docs/plan.md`.

mod build;
mod cli;
mod derive;
mod input;
mod pages;
mod render;

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
            out,
            ..
        } => {
            let input = input::read(&feed_export, &ledger_export)?;
            let n = build::write_site(&input, &out)?;
            println!(
                "built {} story pages from {} clusters into {}",
                n,
                input.clusters.len(),
                out.display()
            );
            Ok(())
        }
        cli::Command::Serve { .. } => Err("serve: not written yet (N1)".into()),
        cli::Command::Check { .. } => Err("check: not written yet (N1)".into()),
    }
}
