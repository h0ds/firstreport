//! `reply`: Kagami pillar 4. See `docs/plan.md`. R0 is offline: contract,
//! templates, `render` and `check`.

mod api;
mod cli;
mod client;
mod compose;
mod facts;
mod model;
mod observe;
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
            let layout = set.layout(&platform)?.clone();
            let api = client::Http::new()?;
            let mut q = queue::Queue::open(&args.data)?;
            match platform.as_str() {
                "bluesky" => {
                    let listener = bluesky_from_args(&args)?;
                    listen(listener, &api, &set, &layout, &args, &mut q, once).await
                }
                "x" => {
                    let listener = platform::x::X::from_env(&args.pds_or_default("x"), &args.data)?;
                    listen(listener, &api, &set, &layout, &args, &mut q, once).await
                }
                other => Err(format!("no listener for platform {other}")),
            }
        }
        cli::Command::Serve => {
            let boxed: Option<Box<dyn platform::Poster>> = match args.platform.as_str() {
                "bluesky"
                    if !args.handle.is_empty()
                        && std::env::var(platform::bluesky::PASSWORD_ENV).is_ok() =>
                {
                    Some(Box::new(bluesky_from_args(&args)?))
                }
                "x" if std::env::var(platform::x::ACCESS_TOKEN_ENV).is_ok()
                    || args.data.join("x_token.json").exists() =>
                {
                    Some(Box::new(platform::x::X::from_env(
                        &args.pds_or_default("x"),
                        &args.data,
                    )?))
                }
                _ => None,
            };
            let poster_name = boxed.as_ref().map(|p| p.name().to_string());
            let poster = boxed.map(|b| std::sync::Arc::new(tokio::sync::Mutex::new(b)));
            eprintln!(
                "review page on http://{} ; data in {} ; poster: {}",
                args.bind,
                args.data.display(),
                poster_name.as_deref().unwrap_or("none (approving is off)")
            );
            let app = api::router(api::AppState {
                data: args.data.clone(),
                poster,
                limits: args.limits,
            });
            let listener = tokio::net::TcpListener::bind(&args.bind)
                .await
                .map_err(|e| format!("bind {}: {e}", args.bind))?;
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await
                .map_err(|e| e.to_string())
        }
        cli::Command::Observe { from, once } => {
            // A recorded file already holds hashed accounts; only the live
            // firehose needs the salt.
            let salt = match &from {
                Some(_) => String::new(),
                None => std::env::var(observe::SALT_ENV).map_err(|_| {
                    format!(
                        "observe needs {} in the environment: a long random string that never changes",
                        observe::SALT_ENV
                    )
                })?,
            };
            observe_cmd(&args, &salt, from, once).await
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
                    match resolve::resolve(api, &args.urls, &m).await.map(|mut fs| {
                        fs.observations = observe::store::observations_for(
                            &args.data,
                            fs.subject.urls.first().map(String::as_str),
                            fs.subject.text.as_deref().map(observe::simhash::simhash),
                        );
                        fs
                    }) {
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

/// The observer loop (plan §16, R4). Live: read Jetstream into the store,
/// and every `interval` run the signals over the retained window, look
/// up ages for accounts in bursts, and append observations. Replaying a
/// file: load it, run once, print what was found.
async fn observe_cmd(
    args: &cli::Args,
    salt: &str,
    from: Option<std::path::PathBuf>,
    once: bool,
) -> Result<(), String> {
    let t = args.thresholds;
    let store = observe::store::Store::open(&args.data)?;
    if let Some(path) = from {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut sightings: Vec<observe::Sighting> = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let s: observe::Sighting =
                serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display()))?;
            store.append(&s)?;
            sightings.push(s);
        }
        let now = sightings.iter().map(|s| s.at).max().unwrap_or(0);
        let (obs, bursts) = observe::signals::run(&sightings, t, now);
        store.append_observations(&obs)?;
        for o in &obs {
            println!(
                "{} {} {}/{} over {}s",
                o.kind, o.subject, o.count, o.denominator, o.window_secs
            );
        }
        println!(
            "{} sightings, {} bursts, {} observations",
            sightings.len(),
            bursts.len(),
            obs.len()
        );
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("kagami-reply/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    // Hash → DID for accounts seen recently, in memory only, so ages can be
    // fetched for burst members without ever writing a DID to disk.
    let dids: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        Default::default();
    let store = std::sync::Arc::new(std::sync::Mutex::new(store));
    eprintln!(
        "observing {} ; sightings in {} ; signals every {}s",
        args.firehose,
        args.data.join("sightings").display(),
        args.interval_secs.max(60)
    );

    let reader = {
        let (store, dids, url, salt) = (
            store.clone(),
            dids.clone(),
            args.firehose.clone(),
            salt.to_string(),
        );
        tokio::spawn(async move {
            loop {
                let r = observe::jetstream::run(&url, &salt, |s, did| {
                    dids.lock()
                        .unwrap()
                        .insert(s.account.clone(), did.to_string());
                    store.lock().unwrap().append(&s)
                })
                .await;
                eprintln!(
                    "firehose ended: {}; reconnecting in 10s",
                    r.err().unwrap_or_else(|| "closed".into())
                );
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        })
    };

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(args.interval_secs.max(60))).await;
        let now = now();
        let since = now - t.movers_window_days as i64 * 86_400;
        let sightings = store.lock().unwrap().load(since)?;
        let (_, bursts) = observe::signals::run(&sightings, t, now);
        // Ages for burst members not yet known, then run again with them.
        let want: Vec<String> = {
            let known = &store.lock().unwrap().ages;
            let dids = dids.lock().unwrap();
            bursts
                .iter()
                .flat_map(|b| b.accounts_in_order.iter())
                .filter(|a| !known.contains_key(*a))
                .filter_map(|a| dids.get(a).cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        if !want.is_empty() {
            let ages =
                observe::jetstream::ages(&client, observe::jetstream::PUBLIC_API, &want, now).await;
            let dids = dids.lock().unwrap();
            let mut st = store.lock().unwrap();
            for (hash, did) in dids.iter() {
                if let Some(age) = ages.get(did) {
                    st.remember_age(hash, *age);
                }
            }
            st.save_ages()?;
        }
        let sightings = store.lock().unwrap().load(since)?;
        let (obs, bursts) = observe::signals::run(&sightings, t, now);
        let st = store.lock().unwrap();
        st.append_observations(&obs)?;
        let pruned = st.prune(now, t.retain_days)?;
        eprintln!(
            "{} sightings, {} bursts, {} observations, {} day files pruned",
            sightings.len(),
            bursts.len(),
            obs.len(),
            pruned
        );
        if once {
            reader.abort();
            return Ok(());
        }
    }
}

fn bluesky_from_args(args: &cli::Args) -> Result<platform::bluesky::Bluesky, String> {
    if args.handle.is_empty() {
        return Err("bluesky needs --handle <the bot's account>".into());
    }
    let password = std::env::var(platform::bluesky::PASSWORD_ENV).map_err(|_| {
        format!(
            "bluesky needs {} in the environment",
            platform::bluesky::PASSWORD_ENV
        )
    })?;
    platform::bluesky::Bluesky::new(&args.pds_or_default("bluesky"), &args.handle, &password)
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
