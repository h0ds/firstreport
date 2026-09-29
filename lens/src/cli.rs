//! Hand-rolled argument parsing, as in `feed` and `ledger`.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read the two exports and write the site.
    Build {
        feed_export: PathBuf,
        ledger_export: PathBuf,
        out: PathBuf,
        window_hours: i64,
        base_url: String,
    },
    /// Serve a built site for local review.
    Serve { out: PathBuf, bind: String },
    /// Every internal link resolves and every page has its twins.
    Check { out: PathBuf },
}

pub const USAGE: &str = "\
usage: lens <command>

commands:
  build --feed-export <dir> --ledger-export <dir> --out <dir> [--window <hours>] [--base-url <url>]
  serve --out <dir> [--bind <addr>]      static file server, default 127.0.0.1:8082
  check --out <dir>                      links resolve, twins present
";

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Command, String> {
    let mut it = argv.into_iter().skip(1);
    let cmd = it.next().ok_or_else(|| USAGE.to_string())?;
    let rest: Vec<String> = it.collect();
    let flag = |name: &str| -> Option<String> {
        rest.iter()
            .position(|x| x == name)
            .and_then(|i| rest.get(i + 1).cloned())
    };
    let need =
        |name: &str| flag(name).ok_or_else(|| format!("{cmd} requires {name} <value>\n\n{USAGE}"));
    match cmd.as_str() {
        "build" => Ok(Command::Build {
            feed_export: need("--feed-export")?.into(),
            ledger_export: need("--ledger-export")?.into(),
            out: need("--out")?.into(),
            window_hours: match flag("--window") {
                Some(v) => v
                    .parse()
                    .ok()
                    .filter(|h: &i64| *h > 0)
                    .ok_or("--window needs a positive number of hours")?,
                None => 24,
            },
            base_url: flag("--base-url").unwrap_or_default(),
        }),
        "serve" => Ok(Command::Serve {
            out: need("--out")?.into(),
            bind: flag("--bind").unwrap_or_else(|| "127.0.0.1:8082".into()),
        }),
        "check" => Ok(Command::Check {
            out: need("--out")?.into(),
        }),
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("lens")
            .chain(s.split_whitespace())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_commands() {
        let c = parse(argv(
            "build --feed-export f --ledger-export l --out o --window 48",
        ))
        .unwrap();
        assert_eq!(
            c,
            Command::Build {
                feed_export: "f".into(),
                ledger_export: "l".into(),
                out: "o".into(),
                window_hours: 48,
                base_url: String::new()
            }
        );
        assert!(parse(argv("build --out o")).is_err());
        assert!(parse(argv(
            "build --feed-export f --ledger-export l --out o --window 0"
        ))
        .is_err());
        assert_eq!(
            parse(argv("check --out site")).unwrap(),
            Command::Check { out: "site".into() }
        );
        assert!(parse(argv("nope")).is_err());
    }
}
