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
    Refresh { write: bool },
    /// Pull claims from a source. `write` false prints the proposal.
    Import {
        source: String,
        only: Option<String>,
        write: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub data_dir: PathBuf,
    /// feed's outlets.toml; enables outlet-id validation and drives import.
    pub outlets: Option<PathBuf>,
    pub bind: Option<String>,
    pub command: Command,
}

pub const USAGE: &str = "\
usage: ledger [--data <dir>] [--outlets <feed outlets.toml>] [--bind <addr>] <command>

commands:
  check                              validate sources and claims
  serve                              read-only API (default 127.0.0.1:8081)
  export --out <dir>                 claims.ndjson, sources.ndjson, outlets.ndjson
  refresh [--write]                  re-import scheduled sources that are due (list unless --write)
  import <source> [--only <id>] [--write]
                                     pull claims from a source (wikidata); print unless --write
";

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut data_dir = PathBuf::from("data");
    let mut outlets = None;
    let mut bind = None;
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
        },
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    Ok(Args {
        data_dir,
        outlets,
        bind,
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
                write: true
            }
        );
        assert_eq!(parse(argv("check")).unwrap().command, Command::Check);
        assert_eq!(
            parse(argv("refresh --write")).unwrap().command,
            Command::Refresh { write: true }
        );
        assert!(parse(argv("export")).is_err());
        assert!(parse(argv("import --write")).is_err());
    }
}
