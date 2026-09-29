//! Hand-rolled argument parsing, as in `feed`.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Validate data/ against sources and (optionally) feed's outlets.
    Check,
    /// Read-only API.
    Serve,
    /// NDJSON export.
    Export { out: PathBuf },
    /// Re-import every scheduled source that is due. `write` false lists them.
    Refresh { write: bool, force: bool },
    /// Pull claims from a source. `write` false prints the proposal.
    Import {
        source: String,
        only: Option<String>,
        write: bool,
        /// Write past the change guard (docs/steward.md §4).
        force: bool,
        /// A local file for sources supplied as files (allsides; optional
        /// for robertson2018, which is otherwise downloaded).
        file: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub data_dir: PathBuf,
    /// feed's outlets.toml; enables outlet-id validation and drives import.
    pub outlets: Option<PathBuf>,
    pub bind: Option<String>,
    /// Directory of a `feed export --format ndjson`; drives `import feed_behaviour`.
    pub feed_export: Option<PathBuf>,
    /// feed API base, used as the ref_url of observed claims.
    pub feed_api: String,
    /// Window in days for observed behaviour.
    pub window_days: i64,
    pub command: Command,
}

pub const USAGE: &str = "\
usage: ledger [--data <dir>] [--outlets <feed outlets.toml>] [--bind <addr>]
              [--feed-export <dir>] [--feed-api <url>] [--window <days>] <command>

commands:
  check                              validate sources and claims
  serve                              read-only API (default 127.0.0.1:8081)
  export --out <dir>                 claims.ndjson, sources.ndjson, outlets.ndjson
  refresh [--write] [--force]        re-import scheduled sources that are due (list unless --write)
  import <source> [--only <id>] [--file <path>] [--write] [--force]
                                     pull claims from a source; print unless --write.
                                     A run that would change more than a third of the
                                     source's claims is held in data/review/ unless --force
                                     sources: wikidata, wikipedia_rsp, rdap,
                                     feed_behaviour (needs --feed-export; --window default 90),
                                     allsides (--file: the CSV or JSON AllSides supplies),
                                     robertson2018 (downloaded from Dataverse, or --file)
";

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut data_dir = PathBuf::from("data");
    let mut outlets = None;
    let mut bind = None;
    let mut feed_export = None;
    let mut feed_api = String::from("http://127.0.0.1:8080");
    let mut window_days = crate::behaviour::DEFAULT_WINDOW_DAYS;
    while let Some(a) = it.peek() {
        match a.as_str() {
            "--data" => {
                it.next();
                data_dir = it.next().ok_or("--data needs a value")?.into();
            }
            "--outlets" => {
                it.next();
                outlets = Some(it.next().ok_or("--outlets needs a value")?.into());
            }
            "--bind" => {
                it.next();
                bind = Some(it.next().ok_or("--bind needs a value")?);
            }
            "--feed-export" => {
                it.next();
                feed_export = Some(it.next().ok_or("--feed-export needs a value")?.into());
            }
            "--feed-api" => {
                it.next();
                feed_api = it.next().ok_or("--feed-api needs a value")?;
            }
            "--window" => {
                it.next();
                window_days = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|d: &i64| *d > 0)
                    .ok_or("--window needs a positive number of days")?;
            }
            _ => break,
        }
    }
    let cmd = it.next().ok_or_else(|| USAGE.to_string())?;
    let rest: Vec<String> = it.collect();
    let flag = |name: &str| -> Option<String> {
        rest.iter()
            .position(|x| x == name)
            .and_then(|i| rest.get(i + 1).cloned())
    };
    let command = match cmd.as_str() {
        "check" => Command::Check,
        "serve" => Command::Serve,
        "refresh" => Command::Refresh {
            write: rest.iter().any(|x| x == "--write"),
            force: rest.iter().any(|x| x == "--force"),
        },
        "export" => Command::Export {
            out: flag("--out").ok_or("export requires --out <dir>")?.into(),
        },
        "import" => Command::Import {
            source: rest
                .iter()
                .find(|x| !x.starts_with("--"))
                .cloned()
                .ok_or("import requires a source name, e.g. wikidata")?,
            only: flag("--only"),
            write: rest.iter().any(|x| x == "--write"),
            force: rest.iter().any(|x| x == "--force"),
            file: flag("--file").map(PathBuf::from),
        },
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    Ok(Args {
        data_dir,
        outlets,
        bind,
        feed_export,
        feed_api,
        window_days,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("ledger")
            .chain(s.split_whitespace())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_commands() {
        let a = parse(argv(
            "--data d --outlets o.toml import wikidata --only bbc --write",
        ))
        .unwrap();
        assert_eq!(a.data_dir, PathBuf::from("d"));
        assert_eq!(a.outlets, Some(PathBuf::from("o.toml")));
        assert_eq!(
            a.command,
            Command::Import {
                source: "wikidata".into(),
                only: Some("bbc".into()),
                write: true,
                force: false,
                file: None
            }
        );
        assert_eq!(parse(argv("check")).unwrap().command, Command::Check);
        assert_eq!(
            parse(argv("refresh --write")).unwrap().command,
            Command::Refresh {
                write: true,
                force: false
            }
        );
        assert_eq!(
            parse(argv("import rdap --write --force")).unwrap().command,
            Command::Import {
                source: "rdap".into(),
                only: None,
                write: true,
                force: true,
                file: None
            }
        );
        let a = parse(argv("--feed-export out --window 30 import feed_behaviour")).unwrap();
        assert_eq!(a.feed_export, Some(PathBuf::from("out")));
        assert_eq!(a.window_days, 30);
        assert_eq!(a.feed_api, "http://127.0.0.1:8080");
        assert!(parse(argv("--window 0 check")).is_err());
        assert!(parse(argv("export")).is_err());
        assert!(parse(argv("import --write")).is_err());
    }
}
