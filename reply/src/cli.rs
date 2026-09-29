//! Hand-rolled argument parsing, as in `feed` and `ledger`.

use std::path::PathBuf;

use crate::compose::Urls;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Render one recorded mention through the templates for one platform.
    Render { fixture: PathBuf, platform: String },
    /// Validate templates and layouts, render every fixture, compare goldens.
    Check { fixtures: PathBuf, update: bool },
    /// Resolve one recorded mention against live feed and ledger; print
    /// the FactSet, or the draft with `render`.
    Resolve {
        mention: PathBuf,
        platform: String,
        render: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub templates: PathBuf,
    pub urls: Urls,
    pub command: Command,
}

pub const USAGE: &str = "\
usage: reply [--templates <dir>] [--feed-url <url>] [--ledger-url <url>] [--log-url <url>] <command>

commands:
  render <fixture.json> [--platform <name>]   render a recorded mention; print the draft and its cites
  check [--fixtures <dir>] [--update]         validate templates and layouts, render every fixture,
                                              compare with the golden drafts (rewrite them with --update)
  resolve <mention.json> [--render] [--platform <name>]
                                              ask feed and ledger about a mention; print the FactSet
                                              (a fixture for check) or, with --render, the draft
";

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut templates = PathBuf::from("templates");
    let mut urls = Urls::default();
    let mut command = None;
    let mut fixture = None;
    let mut platform = String::from("bluesky");
    let mut fixtures = PathBuf::from("fixtures/mentions");
    let mut update = false;
    let mut do_render = false;
    while let Some(a) = it.next() {
        let mut val = |flag: &str| {
            it.next()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--templates" => templates = val("--templates")?.into(),
            "--feed-url" => urls.feed = val("--feed-url")?,
            "--ledger-url" => urls.ledger = val("--ledger-url")?,
            "--log-url" => urls.log = val("--log-url")?,
            "--platform" => platform = val("--platform")?,
            "--fixtures" => fixtures = val("--fixtures")?.into(),
            "--update" => update = true,
            "--render" => do_render = true,
            "render" | "check" | "resolve" => command = Some(a),
            "-h" | "--help" => return Err(USAGE.into()),
            other
                if matches!(command.as_deref(), Some("render" | "resolve"))
                    && fixture.is_none() =>
            {
                fixture = Some(PathBuf::from(other))
            }
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    let command = match command.as_deref() {
        Some("render") => Command::Render {
            fixture: fixture.ok_or_else(|| format!("render needs a fixture\n{USAGE}"))?,
            platform,
        },
        Some("check") => Command::Check { fixtures, update },
        Some("resolve") => Command::Resolve {
            mention: fixture.ok_or_else(|| format!("resolve needs a mention file\n{USAGE}"))?,
            platform,
            render: do_render,
        },
        _ => return Err(USAGE.into()),
    };
    Ok(Args {
        templates,
        urls,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Args, String> {
        parse(std::iter::once("reply".to_string()).chain(s.split(' ').map(String::from)))
    }

    #[test]
    fn parses_render_and_check() {
        let a = p("render fixtures/mentions/x.json --platform mastodon").unwrap();
        assert_eq!(
            a.command,
            Command::Render {
                fixture: "fixtures/mentions/x.json".into(),
                platform: "mastodon".into()
            }
        );
        let a = p("--log-url http://l check --update").unwrap();
        assert_eq!(a.urls.log, "http://l");
        assert_eq!(
            a.command,
            Command::Check {
                fixtures: "fixtures/mentions".into(),
                update: true
            }
        );
        let a = p("resolve m.json --render").unwrap();
        assert_eq!(
            a.command,
            Command::Resolve {
                mention: "m.json".into(),
                platform: "bluesky".into(),
                render: true
            }
        );
        assert!(p("render").is_err());
        assert!(p("bogus").is_err());
    }
}
