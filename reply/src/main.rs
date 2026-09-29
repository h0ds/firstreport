//! `reply`: Kagami pillar 4. See `docs/plan.md`. R0 is offline: contract,
//! templates, `render` and `check`.

mod cli;
mod client;
mod compose;
mod facts;
mod model;
mod platform;
mod queue;
mod resolve;
mod template;

use std::path::Path;

#[tokio::main]
async fn main() {
    let args = match cli::parse(std::env::args()) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(args: cli::Args) -> Result<(), String> {
    let set = template::load(&args.templates)?;
    match args.command.clone() {
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
        cli::Command::Resolve {
            mention,
            platform,
            render,
        } => {
            let m: model::Mention = model::load_json(&mention)?;
            let api = client::Http::new()?;
            let fs = resolve::resolve(&api, &args.urls, &m).await?;
            let out = if render {
                let layout = set.layout(&platform)?;
                serde_json::to_value(compose::compose(&fs, &set, layout, &args.urls)?)
            } else {
                serde_json::to_value(&fs)
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&out.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?
            );
            Ok(())
        }
        cli::Command::Listen { platform, once } => {
            if platform != "bluesky" {
                return Err(format!("no listener for platform {platform}"));
            }
            if args.handle.is_empty() {
                return Err("listen needs --handle <the bot's account>".into());
            }
            let password = std::env::var(platform::bluesky::PASSWORD_ENV).map_err(|_| {
                format!(
                    "listen needs {} in the environment",
                    platform::bluesky::PASSWORD_ENV
                )
            })?;
            let listener = platform::bluesky::Bluesky::new(&args.pds, &args.handle, &password)?;
            let layout = set.layout(&platform)?.clone();
            let api = client::Http::new()?;
            let mut q = queue::Queue::open(&args.data)?;
            listen(listener, &api, &set, &layout, &args, &mut q, once).await
        }
        cli::Command::Queue => {
            let q = queue::Queue::open(&args.data)?;
            let entries = q.list()?;
            for e in &entries {
                println!(
                    "{} {} {} @{} {:?}",
                    e.status,
                    facts::date_time(e.queued_at),
                    e.facts.mention.platform,
                    e.facts.mention.author,
                    e.facts.mention.text
                );
                for (i, p) in e.draft.posts.iter().enumerate() {
                    println!("  {}: {p}", i + 1);
                }
                for g in &e.facts.gaps {
                    println!("  gap: {g}");
                }
            }
            println!("{} queued", entries.len());
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

/// The dry-run loop (plan §16, R2): every new mention is resolved,
/// drafted and queued. A mention that cannot be resolved is logged and
/// marked seen so it is not retried forever; a poll that fails is
/// retried next interval.
async fn listen<L: platform::Listener, A: client::Api>(
    mut listener: L,
    api: &A,
    set: &template::Set,
    layout: &template::Layout,
    args: &cli::Args,
    q: &mut queue::Queue,
    once: bool,
) -> Result<(), String> {
    eprintln!(
        "listening on {} as {} every {}s; drafts go to {}; nothing is posted",
        listener.name(),
        args.handle,
        args.interval_secs,
        args.data.join("queue").display()
    );
    loop {
        match listener.poll().await {
            Err(e) => eprintln!("poll failed: {e}"),
            Ok(mentions) => {
                for m in mentions {
                    if q.seen(&m.id) {
                        continue;
                    }
                    match resolve::resolve(api, &args.urls, &m).await {
                        Err(e) => {
                            eprintln!("{}: not drafted: {e}", m.id);
                            if e.contains("404") {
                                q.mark_seen(&m.id)?;
                            }
                        }
                        Ok(fs) => match compose::compose(&fs, set, layout, &args.urls) {
                            Err(e) => {
                                eprintln!("{}: compose failed: {e}", m.id);
                                q.mark_seen(&m.id)?;
                            }
                            Ok(draft) => {
                                let path = q.push(fs, draft, now())?;
                                eprintln!("{}: queued {}", m.id, path.display());
                            }
                        },
                    }
                }
            }
        }
        if once {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(args.interval_secs)).await;
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

    struct Canned(Vec<model::Mention>);
    impl platform::Listener for Canned {
        fn name(&self) -> &str {
            "canned"
        }
        async fn poll(&mut self) -> Result<Vec<model::Mention>, String> {
            Ok(self.0.clone())
        }
    }

    /// One dry-run pass: every mention is drafted into the queue once,
    /// a second pass queues nothing, and nothing needs a network.
    #[tokio::test]
    async fn listen_once_queues_each_mention_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let set = template::load(&root.join("templates")).unwrap();
        let layout = set.layout("bluesky").unwrap().clone();
        let mut args =
            cli::parse(["reply", "--handle", "kagami.test", "listen", "--once"].map(String::from))
                .unwrap();
        args.data = std::env::temp_dir().join(format!("kg-reply-listen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&args.data);

        let notes: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("fixtures/bluesky/notifications.json")).unwrap(),
        )
        .unwrap();
        let posts = platform::bluesky::parse_posts(
            &serde_json::from_str(
                &std::fs::read_to_string(root.join("fixtures/bluesky/posts.json")).unwrap(),
            )
            .unwrap(),
        );
        let mentions: Vec<model::Mention> = platform::bluesky::parse_notifications(&notes, "")
            .into_iter()
            .map(|p| p.finish(&posts))
            .collect();
        assert_eq!(mentions.len(), 3);

        let mut api = client::Recorded::default();
        // Only the unknown-domain lookup is recorded; the others 404 and are
        // marked seen without a draft.
        api.responses.insert(
            "http://127.0.0.1:8080/v1/lookup?url=https%3A%2F%2Ftruth-wire.example%2Fx".into(),
            serde_json::json!({"matched": "none", "host": "truth-wire.example", "checked": ["item", "outlet"]}),
        );
        let mut q = queue::Queue::open(&args.data).unwrap();
        listen(
            Canned(mentions.clone()),
            &api,
            &set,
            &layout,
            &args,
            &mut q,
            true,
        )
        .await
        .unwrap();
        let entries = q.list().unwrap();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].status, "pending");
        assert!(entries[0].draft.posts[0]
            .contains("truth-wire.example is not an outlet Kagami follows"));
        assert!(mentions.iter().all(|m| q.seen(&m.id)));

        listen(Canned(mentions), &api, &set, &layout, &args, &mut q, true)
            .await
            .unwrap();
        assert_eq!(q.list().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&args.data);
    }

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
