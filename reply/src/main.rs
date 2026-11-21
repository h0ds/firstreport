//! `reply`: First Report pillar 4. See `docs/plan.md`. R0 is offline: contract,
//! templates, `render` and `check`.

mod api;
mod cli;
mod client;
mod compose;
mod facts;
mod lang;
mod model;
mod observe;
mod platform;
mod post;
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
            // The same credentials serve the listener, the searcher that
            // observes a subject on demand, and the poster for drafts that
            // have earned autonomy. Without a salt, nothing is observed.
            let salt = std::env::var(observe::SALT_ENV).ok();
            match platform.as_str() {
                "bluesky" => {
                    let listener = bluesky_from_args(&args)?;
                    let searcher: Option<Box<dyn platform::Searcher>> = salt
                        .is_some()
                        .then(|| bluesky_from_args(&args))
                        .transpose()?
                        .map(|s| Box::new(s) as Box<dyn platform::Searcher>);
                    let poster: Option<Box<dyn platform::Poster>> = (args.autonomous_after > 0)
                        .then(|| bluesky_from_args(&args))
                        .transpose()?
                        .map(|p| Box::new(p) as Box<dyn platform::Poster>);
                    let ctx = Ctx {
                        poster,
                        searcher,
                        salt,
                    };
                    listen(listener, ctx, &api, &set, &layout, &args, &mut q, once).await
                }
                "x" => {
                    let mk = || platform::x::X::from_env(&args.pds_or_default("x"), &args.data);
                    let listener = mk()?;
                    let searcher: Option<Box<dyn platform::Searcher>> = salt
                        .is_some()
                        .then(mk)
                        .transpose()?
                        .map(|s| Box::new(s) as Box<dyn platform::Searcher>);
                    let poster: Option<Box<dyn platform::Poster>> = (args.autonomous_after > 0)
                        .then(mk)
                        .transpose()?
                        .map(|p| Box::new(p) as Box<dyn platform::Poster>);
                    let ctx = Ctx {
                        poster,
                        searcher,
                        salt,
                    };
                    listen(listener, ctx, &api, &set, &layout, &args, &mut q, once).await
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
        cli::Command::Observe { from } => observe_cmd(&args, from),
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
                "{} templates in {} languages, {} layouts, {n} fixtures, {} problems",
                set.langs.values().map(|t| t.len()).sum::<usize>(),
                set.langs.len(),
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
/// What `listen` may do beyond drafting: observe a subject, post alone.
#[derive(Default)]
struct Ctx {
    poster: Option<Box<dyn platform::Poster>>,
    searcher: Option<Box<dyn platform::Searcher>>,
    salt: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn listen<L: platform::Listener, A: client::Api>(
    mut listener: L,
    mut ctx: Ctx,
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
                    let resolved = match resolve::resolve(api, &args.urls, &m).await {
                        Ok(mut fs) => {
                            observe_for(&mut ctx, args, &mut fs).await;
                            Ok(fs)
                        }
                        Err(e) => Err(e),
                    };
                    match resolved {
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
                            Ok(mut draft) => {
                                let t = now();
                                let has_obs = !fs.observations.is_empty();
                                let log = q.log()?;
                                draft.autonomous = ctx.poster.is_some()
                                    && post::earned(&draft, has_obs, &log, args.autonomous_after);
                                let path = q.push(fs, draft.clone(), t)?;
                                if !draft.autonomous {
                                    eprintln!("{}: queued {}", m.id, path.display());
                                    continue;
                                }
                                let id = queue::queue_id(&m.id);
                                let entry = q.get(&id)?.ok_or("queued entry vanished")?;
                                let p = ctx.poster.as_deref_mut().unwrap();
                                match post::post_and_log(
                                    q,
                                    p,
                                    &id,
                                    &entry,
                                    draft,
                                    args.limits,
                                    "auto",
                                    t,
                                )
                                .await
                                {
                                    Ok(e) => eprintln!(
                                        "{}: posted by itself {:?}",
                                        m.id,
                                        e.reply.map(|r| r.post_ids)
                                    ),
                                    Err(post::Refused::OverLimit(why)) => {
                                        eprintln!(
                                            "{}: earned but over limit, left for a person: {why}",
                                            m.id
                                        )
                                    }
                                    Err(post::Refused::Platform(e))
                                    | Err(post::Refused::Store(e)) => {
                                        eprintln!(
                                            "{}: autonomous post failed, left for a person: {e}",
                                            m.id
                                        )
                                    }
                                }
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

/// Observe the mention's subject on demand (plan §7, R6): search the
/// platform, record sightings, run the signals, attach what concerns this
/// subject. Anything that stops it becomes a gap in words; the stored
/// observations from earlier searches still apply.
async fn observe_for(ctx: &mut Ctx, args: &cli::Args, fs: &mut model::FactSet) {
    let url = fs.subject.urls.first().cloned();
    let text_ = fs.subject.text.clone();
    if url.is_none() && text_.is_none() {
        return;
    }
    let stored = observe::store::observations_for(
        &args.data,
        url.as_deref(),
        text_.as_deref().map(observe::simhash::simhash),
    );
    let (Some(searcher), Some(salt)) = (ctx.searcher.as_deref_mut(), ctx.salt.as_deref()) else {
        fs.observations = stored;
        fs.gaps
            .push(format!("not observed: {} unset", observe::SALT_ENV));
        return;
    };
    let store = match observe::store::Store::open(&args.data) {
        Ok(s) => s,
        Err(e) => {
            fs.observations = stored;
            fs.gaps.push(format!("not observed: {e}"));
            return;
        }
    };
    match observe::observe_subject(
        searcher,
        &store,
        salt,
        url.as_deref(),
        text_.as_deref(),
        args.thresholds,
        now(),
    )
    .await
    {
        Ok(obs) => fs.observations = obs,
        Err(e) => {
            fs.observations = stored;
            fs.gaps.push(format!("search for the subject failed: {e}"));
        }
    }
}

/// `observe --from`: replay recorded sightings through the signals and
/// print what was found (plan §15). Live observation happens inside
/// `listen`, on demand, when a mention names a subject.
fn observe_cmd(args: &cli::Args, from: std::path::PathBuf) -> Result<(), String> {
    let t = args.thresholds;
    let store = observe::store::Store::open(&args.data)?;
    let text = std::fs::read_to_string(&from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut sightings: Vec<observe::Sighting> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let s: observe::Sighting =
            serde_json::from_str(line).map_err(|e| format!("{}: {e}", from.display()))?;
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
    Ok(())
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
        let mut args = cli::parse(
            ["reply", "--handle", "firstreport.test", "listen", "--once"].map(String::from),
        )
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
            Ctx::default(),
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
            .contains("truth-wire.example is not an outlet First Report follows"));
        assert!(mentions.iter().all(|m| q.seen(&m.id)));

        listen(
            Canned(mentions),
            Ctx::default(),
            &api,
            &set,
            &layout,
            &args,
            &mut q,
            true,
        )
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
