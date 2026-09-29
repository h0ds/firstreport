//! `reply`: Kagami pillar 4. See `docs/plan.md`. R0 is offline: contract,
//! templates, `render` and `check`.

mod cli;
mod compose;
mod facts;
mod model;
mod template;

use std::path::Path;

fn main() {
    let args = match cli::parse(std::env::args()) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: cli::Args) -> Result<(), String> {
    let set = template::load(&args.templates)?;
    match args.command {
        cli::Command::Render { fixture, platform } => {
            let fs = model::load_fixture(&fixture)?;
            let layout = set.layout(&platform)?;
            let draft = compose::compose(&fs, &set, layout, &args.urls)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&draft).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        cli::Command::Check { fixtures, update } => {
            let mut problems = set.check();
            let mut n = 0;
            for path in list_fixtures(&fixtures)? {
                n += 1;
                match check_fixture(&path, &set, &args.urls, update) {
                    Ok(msgs) => problems.extend(msgs),
                    Err(e) => problems.push(format!("{}: {e}", path.display())),
                }
            }
            for p in &problems {
                println!("{p}");
            }
            println!(
                "{} templates, {} layouts, {n} fixtures, {} problems",
                set.templates.len(),
                set.layouts.len(),
                problems.len()
            );
            if problems.is_empty() {
                Ok(())
            } else {
                Err("check failed".into())
            }
        }
    }
}

fn list_fixtures(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && !p.to_string_lossy().ends_with(".draft.json")
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Render a fixture on every layout and compare with its golden draft file,
/// `<name>.<platform>.draft.json`. With `update` the golden is rewritten.
pub fn check_fixture(
    path: &Path,
    set: &template::Set,
    urls: &compose::Urls,
    update: bool,
) -> Result<Vec<String>, String> {
    let fs = model::load_fixture(path)?;
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let mut msgs = Vec::new();
    for (platform, layout) in &set.layouts {
        let draft = compose::compose(&fs, set, layout, urls)?;
        model::check_cites(&draft)?;
        let golden = path.with_file_name(format!("{stem}.{platform}.draft.json"));
        let rendered = serde_json::to_string_pretty(&draft).map_err(|e| e.to_string())? + "\n";
        if update {
            std::fs::write(&golden, &rendered).map_err(|e| e.to_string())?;
            continue;
        }
        match std::fs::read_to_string(&golden) {
            Ok(g) if g == rendered => {}
            Ok(_) => msgs.push(format!(
                "{}: draft differs from golden; run `reply check --update` and review the diff",
                golden.display()
            )),
            Err(_) => msgs.push(format!(
                "{}: missing golden; run `reply check --update`",
                golden.display()
            )),
        }
    }
    Ok(msgs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped templates render every shipped fixture to its golden
    /// draft, on every layout, and every draft passes the cite check.
    #[test]
    fn fixtures_match_goldens() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let set = template::load(&root.join("templates")).unwrap();
        assert!(set.check().is_empty(), "{:?}", set.check());
        let urls = compose::Urls::default();
        let fixtures = list_fixtures(&root.join("fixtures/mentions")).unwrap();
        assert!(fixtures.len() >= 4);
        let mut problems = Vec::new();
        for f in fixtures {
            problems.extend(check_fixture(&f, &set, &urls, false).unwrap());
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
