//! `kagami`: the one command. The pillars stay separate binaries with
//! separate contracts; this drives them from a checkout so "runs
//! locally" is true for someone who is not us.
//!
//!   kagami run    feed run + ledger serve, until Ctrl-C
//!   kagami site   feed export → ledger export → lens build → lens check
//!   kagami doctor what is and is not in place
//!
//! Sibling binaries are found next to this one (a cargo target dir or
//! an install prefix); each runs with its own crate directory as the
//! working directory so its relative config and data paths hold.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{Child, Command};

const USAGE: &str = "\
usage: kagami [--root <checkout>] <command>

commands:
  run                   start feed (poll, cluster, API on :8080) and ledger (API on :8081); Ctrl-C stops both
  site [--out <dir>]    export feed and ledger, build the lens site (default <root>/site), check it
  doctor                report which binaries, configs, models and data are present
";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    Run,
    Site { out: PathBuf },
    Doctor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    root: PathBuf,
    cmd: Cmd,
}

fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut root = PathBuf::from(".");
    while let Some(a) = it.peek() {
        if a == "--root" {
            it.next();
            root = it.next().ok_or("--root needs a value")?.into();
        } else {
            break;
        }
    }
    let cmd = it.next().ok_or_else(|| USAGE.to_string())?;
    let rest: Vec<String> = it.collect();
    let flag = |name: &str| -> Option<String> {
        rest.iter()
            .position(|x| x == name)
            .and_then(|i| rest.get(i + 1).cloned())
    };
    let cmd = match cmd.as_str() {
        "run" => Cmd::Run,
        "site" => Cmd::Site {
            out: flag("--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("site")),
        },
        "doctor" => Cmd::Doctor,
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    Ok(Args { root, cmd })
}

/// A sibling tool: where its binary is and where it runs.
struct Tool {
    name: &'static str,
    bin: PathBuf,
    cwd: PathBuf,
}

fn tools(root: &Path) -> Result<Vec<Tool>, String> {
    let here = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = here.parent().ok_or("no parent dir for this binary")?;
    let ext = std::env::consts::EXE_SUFFIX;
    Ok(["feed", "ledger", "lens"]
        .iter()
        .map(|n| Tool {
            name: n,
            bin: dir.join(format!("{n}{ext}")),
            cwd: root.join(n),
        })
        .collect())
}

fn tool<'a>(ts: &'a [Tool], name: &str) -> &'a Tool {
    ts.iter().find(|t| t.name == name).expect("known tool")
}

/// The argument lists each step runs with, as data so they can be
/// tested and printed.
fn plan_run() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("feed", vec!["run".into()]),
        (
            "ledger",
            vec![
                "--outlets".into(),
                "../feed/config/outlets.toml".into(),
                "serve".into(),
            ],
        ),
    ]
}

fn plan_site(work: &Path, out: &Path) -> Vec<(&'static str, Vec<String>)> {
    let feed_export = work.join("feed");
    let ledger_export = work.join("ledger");
    let s = |p: &Path| p.to_string_lossy().to_string();
    vec![
        (
            "feed",
            vec![
                "export".into(),
                "--format".into(),
                "ndjson".into(),
                "--out".into(),
                s(&feed_export),
            ],
        ),
        (
            "ledger",
            vec![
                "--outlets".into(),
                "../feed/config/outlets.toml".into(),
                "export".into(),
                "--out".into(),
                s(&ledger_export),
            ],
        ),
        (
            "lens",
            vec![
                "build".into(),
                "--feed-export".into(),
                s(&feed_export),
                "--ledger-export".into(),
                s(&ledger_export),
                "--out".into(),
                s(out),
            ],
        ),
        ("lens", vec!["check".into(), "--out".into(), s(out)]),
    ]
}

fn spawn(t: &Tool, args: &[String]) -> Result<Child, String> {
    if !t.bin.is_file() {
        return Err(format!(
            "{} not found at {}; build the workspace first (cargo build --workspace)",
            t.name,
            t.bin.display()
        ));
    }
    if !t.cwd.is_dir() {
        return Err(format!(
            "{} directory not found at {}; run from the checkout or pass --root",
            t.name,
            t.cwd.display()
        ));
    }
    Command::new(&t.bin)
        .args(args)
        .current_dir(&t.cwd)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("start {}: {e}", t.name))
}

async fn run_step(t: &Tool, args: &[String]) -> Result<(), String> {
    eprintln!("kagami: {} {}", t.name, args.join(" "));
    let status = spawn(t, args)?
        .wait()
        .await
        .map_err(|e| format!("{}: {e}", t.name))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} {} failed with {status}",
            t.name,
            args.join(" ")
        ))
    }
}

async fn run(root: &Path) -> Result<(), String> {
    let ts = tools(root)?;
    let mut children = Vec::new();
    for (name, args) in plan_run() {
        let t = tool(&ts, name);
        eprintln!("kagami: {} {}", t.name, args.join(" "));
        children.push((name, spawn(t, &args)?));
    }
    eprintln!("kagami: feed on http://127.0.0.1:8080/debug, ledger on http://127.0.0.1:8081/lookup; Ctrl-C stops both");
    let result = tokio::select! {
        _ = tokio::signal::ctrl_c() => Ok(()),
        r = wait_any(&mut children) => r,
    };
    for (name, c) in &mut children {
        if c.try_wait().ok().flatten().is_none() {
            let _ = c.start_kill();
            let _ = c.wait().await;
            eprintln!("kagami: stopped {name}");
        }
    }
    result
}

/// Resolves when any child exits; a pillar exiting on its own is an error.
async fn wait_any(children: &mut [(&'static str, Child)]) -> Result<(), String> {
    loop {
        for (name, c) in children.iter_mut() {
            if let Ok(Some(status)) = c.try_wait() {
                return Err(format!("{name} exited with {status}"));
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

async fn site(root: &Path, out: &Path) -> Result<(), String> {
    let ts = tools(root)?;
    let work = root.join("site-build");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let out = if out.is_absolute() {
        out.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(out)
    };
    let work = if work.is_absolute() {
        work
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(work)
    };
    for (name, args) in plan_site(&work, &out) {
        run_step(tool(&ts, name), &args).await?;
    }
    println!("site built at {}", out.display());
    Ok(())
}

/// Bytes and files under a directory, recursively; zero when absent.
fn dir_usage(dir: &Path) -> (u64, usize) {
    let mut bytes = 0;
    let mut files = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                bytes += m.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn doctor(root: &Path) -> Result<(), String> {
    let ts = tools(root)?;
    let mut missing = 0;
    let mut line = |ok: bool, what: &str, hint: &str| {
        println!(
            "{} {what}{}",
            if ok { "ok  " } else { "MISSING" },
            if ok {
                String::new()
            } else {
                format!("  ({hint})")
            }
        );
        if !ok {
            missing += 1;
        }
    };
    for t in &ts {
        line(
            t.bin.is_file(),
            &format!("{} binary at {}", t.name, t.bin.display()),
            "cargo build --workspace",
        );
        line(
            t.cwd.is_dir(),
            &format!("{} directory at {}", t.name, t.cwd.display()),
            "run from the checkout or pass --root",
        );
    }
    let feed = root.join("feed");
    line(
        feed.join("config/feed.toml").is_file(),
        "feed/config/feed.toml",
        "copy from the repository",
    );
    line(
        feed.join("config/feeds.toml").is_file(),
        "feed/config/feeds.toml",
        "seed feeds; feed import --opml or feed discover add more",
    );
    line(
        feed.join("config/outlets.toml").is_file(),
        "feed/config/outlets.toml",
        "the outlet registry, shared with ledger",
    );
    line(
        feed.join("models/vocab.txt").is_file()
            && feed
                .join("models")
                .read_dir()
                .map(|d| {
                    d.flatten()
                        .any(|e| e.path().extension().is_some_and(|x| x == "onnx"))
                })
                .unwrap_or(false),
        "feed/models (embedding model and vocab)",
        "cd feed && cargo run -- fetch-model; without it feed ingests but does not cluster",
    );
    line(
        feed.join("data").is_dir(),
        "feed/data (store and snapshots)",
        "created on the first feed run",
    );
    // Disk (docs/steward.md §5): what the snapshot archive takes now, so
    // a budget in feed.toml can be set before the disk fills.
    let (bytes, files) = dir_usage(&feed.join("data/snapshots"));
    if files > 0 {
        println!(
            "ok   feed/data/snapshots holds {files} files, {:.1} MB (snapshots.max_bytes in feed.toml caps it)",
            bytes as f64 / 1e6
        );
    }
    let ledger = root.join("ledger");
    line(
        ledger.join("data/sources.toml").is_file(),
        "ledger/data/sources.toml",
        "copy from the repository",
    );
    let claims = ledger
        .join("data/claims")
        .read_dir()
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                .count()
        })
        .unwrap_or(0);
    line(claims > 0, &format!("ledger/data/claims ({claims} outlet files)"), "cd ledger && cargo run -- --outlets ../feed/config/outlets.toml import wikidata --write, then the other importers");
    // The steward's view of the feeds (docs/steward.md §2), once there is
    // a store to read: failing feeds and feeds it is holding.
    if feed.join("data/feed.db").is_file() {
        if let Some(t) = ts.iter().find(|t| t.name == "feed") {
            println!("feed feeds:");
            match std::process::Command::new(&t.bin)
                .current_dir(&t.cwd)
                .arg("feeds")
                .output()
            {
                Ok(out) => {
                    for l in String::from_utf8_lossy(&out.stdout).lines() {
                        println!("  {l}");
                    }
                }
                Err(e) => println!("  could not run feed feeds: {e}"),
            }
        }
    }
    if missing == 0 {
        println!("everything in place: kagami run, then kagami site");
        Ok(())
    } else {
        Err(format!("{missing} missing"))
    }
}

#[tokio::main]
async fn main() {
    let args = match parse(std::env::args()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let r = match args.cmd {
        Cmd::Run => run(&args.root).await,
        Cmd::Site { out } => site(&args.root, &out).await,
        Cmd::Doctor => doctor(&args.root),
    };
    if let Err(e) = r {
        eprintln!("kagami: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("kagami")
            .chain(s.split_whitespace())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_and_plans() {
        assert_eq!(parse(argv("run")).unwrap().cmd, Cmd::Run);
        let a = parse(argv("--root /x site")).unwrap();
        assert_eq!(a.root, PathBuf::from("/x"));
        assert_eq!(
            a.cmd,
            Cmd::Site {
                out: PathBuf::from("/x/site")
            }
        );
        assert_eq!(
            parse(argv("site --out s")).unwrap().cmd,
            Cmd::Site { out: "s".into() }
        );
        assert!(parse(argv("nope")).is_err());
        assert!(parse(argv("")).is_err());
        let p = plan_site(Path::new("/w"), Path::new("/o"));
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].0, "feed");
        assert!(p[2].1.contains(&"/w/feed".to_string()) && p[2].1.contains(&"/o".to_string()));
        assert_eq!(p[3].1, vec!["check", "--out", "/o"]);
        assert_eq!(plan_run()[1].1[1], "../feed/config/outlets.toml");
        let ts = tools(Path::new("/r")).unwrap();
        assert_eq!(ts[2].cwd, PathBuf::from("/r/lens"));
        assert!(ts[0]
            .bin
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("feed"));
    }
}
