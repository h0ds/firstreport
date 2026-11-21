//! `lens`: pillar 3. A static site generated from `feed` and `ledger`
//! exports. It arranges what the other two record and adds nothing:
//! no score, no lean colour, no summary in its own voice. Plan in
//! `docs/plan.md`.

mod build;
mod check;
mod cli;
mod derive;
mod input;
mod llms;
mod pages;
mod render;
mod serve;

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
            window_hours,
            base_url,
            pages,
        } => {
            let mut input = input::read(&feed_export, &ledger_export)?;
            input.pages = pages;
            let (stories, outlets) = build::write_site(&input, &out, window_hours, &base_url)?;
            println!(
                "built {} story pages and {} outlet pages into {}",
                stories,
                outlets,
                out.display()
            );
            Ok(())
        }
        cli::Command::Serve { out, bind } => serve::run(&out, &bind),
        cli::Command::Check { out } => {
            let r = check::run(&out)?;
            for p in &r.problems {
                println!("{p}");
            }
            println!(
                "{} pages, {} internal links, {} problems",
                r.pages,
                r.links,
                r.problems.len()
            );
            if r.problems.is_empty() {
                Ok(())
            } else {
                Err(format!("{} problems", r.problems.len()))
            }
        }
    }
}
