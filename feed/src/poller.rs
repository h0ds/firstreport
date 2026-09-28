//! The poller task (plan §5.1): pops due feeds off the scheduler, fetches
//! them under the client's limits, archives the snapshot, parses, dedupes,
//! and hands new items to the store. Emits `Item`s to `out` for the
//! embedder (M2); until then the receiver is simply dropped.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use crate::bloom::Bloom;
use crate::config::{Config, Feeds};
use crate::fetch::client::{Client, Fetched};
use crate::fetch::scheduler::{Outcome, Scheduler};
use crate::parse::{feed as feedparse, outlet::Resolver};
use crate::snapshot;
use crate::stats::{now, Stats};
use crate::store::reader::{self, Reader};
use crate::store::writer::{FeedState, SnapshotRow, WriteMsg, Writer};
use crate::types::Item;

/// Window for the dedupe bloom filter rebuilt on boot.
pub const BLOOM_WINDOW_SECS: i64 = 7 * 86_400;

/// Ids handed to the writer but possibly not yet committed. Consulted on a
/// bloom hit before asking the store, so the writer's batching window can
/// never let a duplicate through.
pub const RECENT_IDS: usize = 1 << 16;

pub struct Poller {
    cfg: Arc<Config>,
    client: Client,
    resolver: Arc<Resolver>,
    reader: Reader,
    writer: Writer,
    stats: Arc<Stats>,
    bloom: Bloom,
    recent: HashSet<u64>,
    recent_order: VecDeque<u64>,
    snapshot_dir: PathBuf,
    /// Per feed: hash and archived path of the last body written.
    last_body: HashMap<String, (u64, String)>,
}

struct FetchDone {
    idx: usize,
    fetched_at: i64,
    result: Result<Fetched, String>,
}

pub struct Processed {
    pub outcome: Outcome,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub new_items: Vec<Item>,
}

impl Poller {
    pub fn new(
        cfg: Arc<Config>,
        resolver: Arc<Resolver>,
        reader: Reader,
        writer: Writer,
        stats: Arc<Stats>,
    ) -> reqwest::Result<Self> {
        let client = Client::new(cfg.poll.global_concurrency, cfg.poll.per_host_concurrency)?;
        let snapshot_dir = cfg.snapshots.dir.clone();
        Ok(Self {
            cfg,
            client,
            resolver,
            reader,
            writer,
            stats,
            bloom: Bloom::new(1 << 16),
            recent: HashSet::with_capacity(RECENT_IDS),
            recent_order: VecDeque::with_capacity(RECENT_IDS),
            snapshot_dir,
            last_body: HashMap::new(),
        })
    }

    /// Merge the configured feed list with persisted per-feed state.
    pub fn build_states(
        feeds: &Feeds,
        persisted: Vec<FeedState>,
        default_interval: u64,
        now: i64,
    ) -> Vec<FeedState> {
        let mut out = Vec::with_capacity(feeds.feeds.len());
        for (i, f) in feeds.feeds.iter().enumerate() {
            let prev = persisted.iter().find(|p| p.url == f.url);
            let interval = f.interval_secs.unwrap_or(default_interval);
            out.push(FeedState {
                url: f.url.clone(),
                outlet_id: f.outlet.clone(),
                kind: f.kind.map(|k| format!("{k:?}").to_lowercase()),
                interval_secs: interval,
                etag: prev.and_then(|p| p.etag.clone()),
                last_modified: prev.and_then(|p| p.last_modified.clone()),
                // Stagger first fetches so boot doesn't burst every host at once.
                next_fetch_at: prev
                    .map(|p| p.next_fetch_at.min(now + interval as i64))
                    .unwrap_or(now + i as i64 / 4),
                fail_count: prev.map(|p| p.fail_count).unwrap_or(0),
                last_ok_at: prev.and_then(|p| p.last_ok_at),
                dead: prev.map(|p| p.dead).unwrap_or(false),
            });
        }
        out
    }

    pub fn seed_bloom(&mut self, ids: &[u64]) {
        for id in ids {
            self.bloom.insert(*id);
        }
        info!(recent_ids = ids.len(), "dedupe filter seeded");
    }

    pub async fn run(
        mut self,
        feeds: Vec<FeedState>,
        out: mpsc::Sender<Item>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut sched = Scheduler::new(feeds, self.cfg.poll.max_backoff_secs);
        let mut inflight: JoinSet<FetchDone> = JoinSet::new();
        info!(feeds = sched.len(), "poller started");

        loop {
            // Dispatch everything due.
            let t = now();
            while let Some(idx) = sched.pop_due(t) {
                let f = sched.feed(idx).clone();
                let client = self.client.clone();
                inflight.spawn(async move {
                    let result = client
                        .fetch(&f.url, f.etag.as_deref(), f.last_modified.as_deref())
                        .await
                        .map_err(|e| e.to_string());
                    FetchDone {
                        idx,
                        fetched_at: now(),
                        result,
                    }
                });
            }

            let sleep_for = sched
                .next_due_at()
                .map(|due| Duration::from_secs((due - now()).max(0) as u64))
                .unwrap_or(Duration::from_secs(60));

            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(sleep_for.min(Duration::from_secs(60))) => {}
                Some(done) = inflight.join_next(), if !inflight.is_empty() => {
                    let Ok(done) = done else { continue };
                    let feed = sched.feed(done.idx).clone();
                    let p = self.process(&feed, done.fetched_at, done.result).await;
                    for it in &p.new_items {
                        let _ = out.try_send(it.clone());
                    }
                    let st = sched.complete(done.idx, done.fetched_at, p.outcome, p.etag, p.last_modified).clone();
                    if st.dead {
                        warn!(url = %st.url, "feed marked dead after 7 days of failures");
                    }
                    self.writer.send(WriteMsg::FeedState(st)).await;
                }
            }
        }
        inflight.abort_all();
        info!("poller stopped");
    }

    fn remember(&mut self, id: u64) {
        if self.recent.insert(id) {
            self.recent_order.push_back(id);
            if self.recent_order.len() > RECENT_IDS {
                if let Some(old) = self.recent_order.pop_front() {
                    self.recent.remove(&old);
                }
            }
        }
    }

    /// Everything between a fetch result and the store. Public so the
    /// fixture replay harness (M3) can drive it without a network.
    pub async fn process(
        &mut self,
        feed: &FeedState,
        fetched_at: i64,
        result: Result<Fetched, String>,
    ) -> Processed {
        let st = self.stats.clone();
        st.inc(&st.fetches);
        let fetched = match result {
            Ok(f) => f,
            Err(e) => {
                st.inc(&st.fetch_errors);
                warn!(url = %feed.url, fail_count = feed.fail_count + 1, "fetch failed: {e}");
                return Processed {
                    outcome: Outcome::Err,
                    etag: None,
                    last_modified: None,
                    new_items: vec![],
                };
            }
        };
        let Some(body) = fetched.body else {
            st.inc(&st.fetch_304);
            debug!(url = %feed.url, "304 not modified");
            return Processed {
                outcome: Outcome::Ok,
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                new_items: vec![],
            };
        };

        // Snapshot first, parse second: a parse failure still keeps evidence.
        // A body identical to this feed's previous one gets a row but no
        // second file: the row points at the existing file. Feeds without
        // ETags return identical bodies constantly.
        let snap_id = snapshot::snapshot_id(&feed.url, fetched_at);
        let hash = snapshot::body_hash(&body);
        let path = if !self.cfg.snapshots.enabled {
            String::new()
        } else if let Some((_, p)) = self.last_body.get(&feed.url).filter(|(h, _)| *h == hash) {
            debug!(url = %feed.url, "body unchanged; reusing snapshot {p}");
            p.clone()
        } else {
            match snapshot::write(
                &self.snapshot_dir,
                snap_id,
                fetched_at,
                &body,
                self.cfg.snapshots.zstd_level,
            ) {
                Ok(p) => {
                    let p = p.to_string_lossy().into_owned();
                    self.last_body.insert(feed.url.clone(), (hash, p.clone()));
                    p
                }
                Err(e) => {
                    warn!(url = %feed.url, "snapshot write failed: {e}");
                    String::new()
                }
            }
        };

        let parsed = match feedparse::parse(&body, &feed.url) {
            Ok(p) => p,
            Err(e) => {
                st.inc(&st.parse_errors);
                warn!(url = %feed.url, "parse failed: {e}");
                self.writer
                    .send(WriteMsg::Snapshot(SnapshotRow {
                        id: snap_id,
                        feed_url: feed.url.clone(),
                        fetched_at,
                        status: fetched.status,
                        body_hash: snapshot::body_hash(&body),
                        path,
                        item_count: 0,
                    }))
                    .await;
                return Processed {
                    outcome: Outcome::Err,
                    etag: fetched.etag,
                    last_modified: fetched.last_modified,
                    new_items: vec![],
                };
            }
        };

        st.add(&st.items_seen, parsed.entries.len() as u64);
        let mut new_items = Vec::new();
        let mut seen_in_feed = HashSet::new();
        for e in parsed.entries {
            let id = crate::types::item_id(&e.canonical_url);
            if !seen_in_feed.insert(id) {
                continue;
            }
            if self.bloom.might_contain(id) {
                st.inc(&st.bloom_hits);
                if self.recent.contains(&id) {
                    continue;
                }
                let exists = self
                    .reader
                    .call(move |c| reader::has_item(c, id))
                    .await
                    .unwrap_or(true);
                if exists {
                    continue;
                }
            }
            self.bloom.insert(id);
            self.remember(id);
            let outlet_id = if e.host.is_empty() {
                feed.outlet_id.clone()
            } else {
                self.resolver.resolve(&e.host)
            };
            let lang = crate::lang::detect(
                e.lang_hint.as_deref(),
                &format!("{} {}", e.title, e.summary),
            );
            new_items.push(Item {
                id,
                outlet_id,
                canonical_url: e.canonical_url,
                title: e.title,
                summary: e.summary,
                published_at: e.published_at,
                fetched_at,
                lang,
                text_hash: e.text_hash,
                cluster_id: None,
                cluster_rank: None,
                joined_at: None,
                snapshot_id: snap_id,
            });
        }

        st.add(&st.items_new, new_items.len() as u64);
        debug!(url = %feed.url, status = fetched.status, new = new_items.len(), "fetched");
        self.writer
            .send(WriteMsg::Snapshot(SnapshotRow {
                id: snap_id,
                feed_url: feed.url.clone(),
                fetched_at,
                status: fetched.status,
                body_hash: snapshot::body_hash(&body),
                path,
                item_count: new_items.len() as u32,
            }))
            .await;
        if !new_items.is_empty() {
            self.writer.send(WriteMsg::Items(new_items.clone())).await;
        }
        Processed {
            outcome: Outcome::Ok,
            etag: fetched.etag,
            last_modified: fetched.last_modified,
            new_items,
        }
    }
}
