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
    /// Poll the platform for mentions, resolve and draft each, write it to
    /// the queue. Nothing is posted. `once` polls a single time.
    Listen { platform: String, once: bool },
    /// Print the queue.
    Queue,
    /// Review page, queue actions and the public log; posts approved
    /// drafts when platform credentials are present.
    Serve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub templates: PathBuf,
    pub urls: Urls,
    /// Queue and seen set.
    pub data: PathBuf,
    /// The platform `serve` posts to: `x` or `bluesky`.
    pub platform: String,
    /// The bot's own account on the platform (Bluesky handle).
    pub handle: String,
    /// API base URL; empty means the platform's default.
    pub pds: String,
    pub interval_secs: u64,
    pub bind: String,
    pub limits: crate::api::Limits,
    pub command: Command,
}

pub const USAGE: &str = "\
usage: reply [--templates <dir>] [--data <dir>] [--feed-url <url>] [--ledger-url <url>] [--log-url <url>]
             [--platform x|bluesky] [--handle <bot account>] [--api <url>] [--interval <secs>]
             [--bind <addr>] [--per-account-per-day <n>] [--global-per-hour <n>] <command>

commands:
  render <fixture.json> [--platform <name>]   render a recorded mention; print the draft and its cites
  check [--fixtures <dir>] [--update]         validate templates and layouts, render every fixture,
                                              compare with the golden drafts (rewrite them with --update)
  resolve <mention.json> [--render] [--platform <name>]
                                              ask feed and ledger about a mention; print the FactSet
                                              (a fixture for check) or, with --render, the draft
  listen [--once]                             poll for mentions, draft a reply for each, queue it;
                                              nothing is posted
  queue                                       print the queued drafts
  serve                                       review page and public log on 127.0.0.1:8082; with
                                              credentials for --platform, approving posts

credentials, from the environment:
  x        REPLY_X_ACCESS_TOKEN, and for refresh REPLY_X_REFRESH_TOKEN, REPLY_X_CLIENT_ID,
           REPLY_X_CLIENT_SECRET (confidential clients); the rotated token is kept in data/x_token.json
  bluesky  --handle and REPLY_BSKY_PASSWORD (an app password)
";

impl Args {
    /// `--api` if given, else the platform's own default.
    pub fn pds_or_default(&self, platform: &str) -> String {
        if !self.pds.is_empty() {
            return self.pds.clone();
        }
        match platform {
            "bluesky" => crate::platform::bluesky::DEFAULT_PDS.into(),
            _ => crate::platform::x::DEFAULT_API.into(),
        }
    }
}

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut it = argv.into_iter().skip(1).peekable();
    let mut templates = PathBuf::from("templates");
    let mut urls = Urls::default();
    let mut command = None;
    let mut fixture = None;
    let mut platform = String::from("x");
    let mut fixtures = PathBuf::from("fixtures/mentions");
    let mut update = false;
    let mut do_render = false;
    let mut once = false;
    let mut data = PathBuf::from("data");
    let mut handle = String::new();
    let mut pds = String::new();
    let mut interval_secs = 30;
    let mut bind = String::from("127.0.0.1:8082");
    let mut limits = crate::api::Limits::default();
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
            "--once" => once = true,
            "--data" => data = val("--data")?.into(),
            "--handle" => handle = val("--handle")?,
            "--pds" | "--api" => pds = val("--api")?,
            "--interval" => {
                interval_secs = val("--interval")?
                    .parse()
                    .map_err(|_| format!("--interval needs seconds\n{USAGE}"))?
            }
            "--bind" => bind = val("--bind")?,
            "--per-account-per-day" => {
                limits.per_account_per_day = val("--per-account-per-day")?
                    .parse()
                    .map_err(|_| format!("--per-account-per-day needs a number\n{USAGE}"))?
            }
            "--global-per-hour" => {
                limits.global_per_hour = val("--global-per-hour")?
                    .parse()
                    .map_err(|_| format!("--global-per-hour needs a number\n{USAGE}"))?
            }
            "render" | "check" | "resolve" | "listen" | "queue" | "serve" => command = Some(a),
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
            platform: platform.clone(),
        },
        Some("check") => Command::Check { fixtures, update },
        Some("listen") => Command::Listen {
            platform: platform.clone(),
            once,
        },
        Some("queue") => Command::Queue,
        Some("serve") => Command::Serve,
        Some("resolve") => Command::Resolve {
            mention: fixture.ok_or_else(|| format!("resolve needs a mention file\n{USAGE}"))?,
            platform: platform.clone(),
            render: do_render,
        },
        _ => return Err(USAGE.into()),
    };
    Ok(Args {
        templates,
        urls,
        data,
        platform,
        handle,
        pds,
        interval_secs,
        bind,
        limits,
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
                platform: "x".into(),
                render: true
            }
        );
        let a = p("--platform bluesky --handle kagami.test --interval 5 listen --once").unwrap();
        assert_eq!(
            a.command,
            Command::Listen {
                platform: "bluesky".into(),
                once: true
            }
        );
        assert_eq!(a.pds_or_default("bluesky"), "https://bsky.social");
        assert_eq!((a.handle.as_str(), a.interval_secs), ("kagami.test", 5));
        let a = p("listen").unwrap();
        assert_eq!(a.platform, "x");
        assert_eq!(a.pds_or_default("x"), "https://api.x.com");
        let a = p("--bind 0.0.0.0:9000 --per-account-per-day 5 serve").unwrap();
        assert_eq!(a.command, Command::Serve);
        assert_eq!(
            (a.bind.as_str(), a.limits.per_account_per_day),
            ("0.0.0.0:9000", 5)
        );
        assert!(p("render").is_err());
        assert!(p("bogus").is_err());
    }
}
