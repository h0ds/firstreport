//! Hand-rolled argument parsing. Small enough that `clap` buys nothing.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Poll feeds, embed, cluster, serve the API.
    Run,
    /// Download the embedding model into `models/`.
    FetchModel,
    /// Bulk export.
    Export { format: String, out: PathBuf },
    /// Delete items and clusters older than a duration like `90d`.
    Prune { older_than: String },
    /// Run the pipeline offline over recorded fixtures.
    Replay { fixtures: PathBuf },
    /// Turn an OPML subscription list into feeds.toml / outlets.toml entries.
    Import { opml: PathBuf, write: bool },
    /// Find the feeds an outlet's page advertises.
    Discover { url: String, write: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub config_dir: PathBuf,
    pub bind: Option<String>,
    pub command: Command,
}

pub const USAGE: &str = "\
usage: feed [--config <dir>] [--bind <addr>] <command>

commands:
  run                          poll, embed, cluster, serve
  fetch-model                  download the embedding model
  export --format <f> --out <dir>
  prune --older-than <dur>
  replay --fixtures <dir>
  import --opml <file> [--write]   print (or append) feeds/outlets from an OPML list
  discover <url> [--write]         print (or append) feeds advertised by a page
";

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut config_dir = PathBuf::from("config");
    let mut bind = None;

    while let Some(a) = it.peek() {
        match a.as_str() {
            "--config" => {
                it.next();
                config_dir = it.next().ok_or("--config needs a value")?.into();
            }
            "--bind" => {
                it.next();
                bind = Some(it.next().ok_or("--bind needs a value")?);
            }
            _ => break,
        }
    }

    let cmd = it.next().ok_or_else(|| USAGE.to_string())?;
    let rest: Vec<String> = it.collect();
    let flag = |name: &str| -> Result<String, String> {
        rest.iter()
            .position(|x| x == name)
            .and_then(|i| rest.get(i + 1).cloned())
            .ok_or_else(|| format!("{cmd} requires {name} <value>"))
    };

    let command = match cmd.as_str() {
        "run" => Command::Run,
        "fetch-model" => Command::FetchModel,
        "export" => Command::Export {
            format: flag("--format")?,
            out: flag("--out")?.into(),
        },
        "prune" => Command::Prune {
            older_than: flag("--older-than")?,
        },
        "replay" => Command::Replay {
            fixtures: flag("--fixtures")?.into(),
        },
        "import" => Command::Import {
            opml: flag("--opml")?.into(),
            write: rest.iter().any(|x| x == "--write"),
        },
        "discover" => Command::Discover {
            url: rest
                .iter()
                .find(|x| !x.starts_with("--"))
                .cloned()
                .ok_or("discover requires a page URL")?,
            write: rest.iter().any(|x| x == "--write"),
        },
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };

    Ok(Args {
        config_dir,
        bind,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("feed")
            .chain(s.split_whitespace())
            .map(String::from)
            .collect()
    }

    #[test]
    fn run_with_defaults() {
        let a = parse(argv("run")).unwrap();
        assert_eq!(a.command, Command::Run);
        assert_eq!(a.config_dir, PathBuf::from("config"));
        assert_eq!(a.bind, None);
    }

    #[test]
    fn global_flags_then_command() {
        let a = parse(argv(
            "--config /etc/feed --bind 0.0.0.0:80 replay --fixtures fx",
        ))
        .unwrap();
        assert_eq!(a.config_dir, PathBuf::from("/etc/feed"));
        assert_eq!(a.bind.as_deref(), Some("0.0.0.0:80"));
        assert_eq!(
            a.command,
            Command::Replay {
                fixtures: "fx".into()
            }
        );
    }

    #[test]
    fn missing_required_flag_errors() {
        assert!(parse(argv("export --format ndjson")).is_err());
        assert!(parse(argv("discover --write")).is_err());
    }

    #[test]
    fn import_and_discover() {
        assert_eq!(
            parse(argv("import --opml subs.opml --write"))
                .unwrap()
                .command,
            Command::Import {
                opml: "subs.opml".into(),
                write: true
            }
        );
        assert_eq!(
            parse(argv("discover https://example.com")).unwrap().command,
            Command::Discover {
                url: "https://example.com".into(),
                write: false
            }
        );
    }
}
