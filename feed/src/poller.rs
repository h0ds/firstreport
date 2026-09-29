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
use crate::fetch::scheduler::{Outcome, Report, Scheduler, SchedulerCfg};
use crate::parse::revision::{self, Change, Seen};
use crate::parse::{feed as feedparse, outlet::Resolver, simhash};
use crate::snapshot;
use crate::stats::{now, Stats};
use crate::steward::{FetchObs, Health};
use crate::store::reader::{self, Reader};
use crate::store::writer::{FeedState, SnapshotRow, WriteMsg, Writer};
use crate::types::Item;
use crate::types::{ClusterEvent, EventKind};

fn st_inc(stats: &Stats, c: &std::sync::atomic::AtomicU64) {
    stats.inc(c);
}

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
    /// Per feed: the entries it listed last time, for revision diffing.
    /// Loaded lazily from the latest archived snapshot after a restart.
    prev_seen: HashMap<String, Vec<Seen>>,
}

struct FetchFailure {
    status: Option<u16>,
    retry_after_secs: Option<u64>,
    message: String,
}

struct FetchDone {
    idx: usize,
    fetched_at: i64,
    result: Result<Fetched, FetchFailure>,
}

#[derive(Debug, Default)]
pub struct Processed {
    pub outcome: Outcome,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub resolved_url: Option<String>,
    /// Set for failures that happen after the fetch (parse errors).
    pub error: Option<String>,
    pub new_items: Vec<Item>,
    /// Feed health after this fetch (docs/steward.md §2); None when the
    /// fetch returned no body, so there was nothing to judge.
    pub health: Option<Health>,
}

/// Publish dates further ahead of the fetch than this are treated as
/// broken and replaced with the fetch time.
pub const MAX_FUTURE_SECS: i64 = 3600;

/// Simhash distance at or under which two texts count as the same for
/// the steward's duplicate signal (the syndication threshold).
pub const DUP_HAMMING: u32 = 3;

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
            prev_seen: HashMap::new(),
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
            // A changed configured interval resets the adapted one.
            let adapted = prev
                .filter(|p| p.base_interval_secs == interval)
                .map(|p| p.interval_secs)
                .unwrap_or(interval);
            out.push(FeedState {
                url: f.url.clone(),
                outlet_id: f.outlet.clone(),
                kind: f.kind.map(|k| format!("{k:?}").to_lowercase()),
                base_interval_secs: interval,
                interval_secs: adapted,
                etag: prev.and_then(|p| p.etag.clone()),
                last_modified: prev.and_then(|p| p.last_modified.clone()),
                // Stagger first fetches so boot doesn't burst every host at once.
                next_fetch_at: prev
                    .map(|p| p.next_fetch_at.min(now + interval as i64))
                    .unwrap_or(now + i as i64 / 4),
                fail_count: prev.map(|p| p.fail_count).unwrap_or(0),
                last_ok_at: prev.and_then(|p| p.last_ok_at),
                dead: prev.map(|p| p.dead).unwrap_or(false),
                resolved_url: prev.and_then(|p| p.resolved_url.clone()),
                last_fetch_at: prev.and_then(|p| p.last_fetch_at),
                last_status: prev.and_then(|p| p.last_status),
                last_error: prev.and_then(|p| p.last_error.clone()),
                last_items: prev.map(|p| p.last_items).unwrap_or(0),
                health: prev.and_then(|p| p.health.clone()),
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
        let mut sched = Scheduler::new(feeds, SchedulerCfg::from(&self.cfg.poll));
        let mut inflight: JoinSet<FetchDone> = JoinSet::new();
        info!(feeds = sched.len(), "poller started");

        loop {
            // Dispatch everything due.
            let t = now();
            while let Some(idx) = sched.pop_due(t) {
                let f = sched.feed(idx).clone();
                let client = self.client.clone();
                inflight.spawn(async move {
                    // A permanently moved feed is fetched from its new home
                    // while keeping its configured URL as identity.
                    let target = f.resolved_url.clone().unwrap_or_else(|| f.url.clone());
                    let result = client
                        .fetch(&target, f.etag.as_deref(), f.last_modified.as_deref())
                        .await
                        .map_err(|e| FetchFailure {
                            status: e.status(),
                            retry_after_secs: e.retry_after(),
                            message: e.to_string(),
                        });
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
                    let (retry_after, status, error) = match &done.result {
                        Ok(f) => (None, Some(f.status), None),
                        Err(e) => (e.retry_after_secs, e.status, Some(e.message.clone())),
                    };
                    let p = self.process(&feed, done.fetched_at, done.result.map_err(|e| e.message)).await;
                    for it in &p.new_items {
                        let _ = out.try_send(it.clone());
                    }
                    let report = Report {
                        outcome: Some(p.outcome),
                        status,
                        error: error.or(p.error),
                        etag: p.etag,
                        last_modified: p.last_modified,
                        retry_after_secs: retry_after,
                        resolved_url: p.resolved_url,
                        new_items: p.new_items.len() as u32,
                        health: p.health,
                    };
                    let had_move = feed.resolved_url.is_none() && report.resolved_url.is_some();
                    let st = sched.complete(done.idx, done.fetched_at, report).clone();
                    if had_move {
                        info!(url = %st.url, moved_to = ?st.resolved_url, "feed permanently moved; update feeds.toml");
                    }
                    if st.dead {
                        warn!(url = %st.url, "feed marked dead after 7 days of failures");
                    }
                    self.writer.send(WriteMsg::FeedState(Box::new(st))).await;
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

    /// The feed's previous listing: in memory after the first fetch this
    /// process, otherwise parsed from the latest archived snapshot.
    async fn previous_listing(&mut self, feed_url: &str) -> Vec<Seen> {
        if let Some(prev) = self.prev_seen.get(feed_url) {
            return prev.clone();
        }
        let url = feed_url.to_string();
        let Ok(Some(meta)) = self
            .reader
            .call(move |c| reader::latest_snapshot(c, &url))
            .await
        else {
            return vec![];
        };
        if meta.path.is_empty() {
            return vec![];
        }
        let body = match snapshot::read(&self.snapshot_dir, std::path::Path::new(&meta.path)) {
            Ok(b) => b,
            Err(e) => {
                debug!(url = feed_url, "previous snapshot unreadable: {e}");
                return vec![];
            }
        };
        match feedparse::parse(&body, feed_url) {
            Ok(p) => p
                .entries
                .iter()
                .map(|e| Seen {
                    id: crate::types::item_id(&e.canonical_url),
                    title: e.title.clone(),
                    summary: e.summary.clone(),
                    published_at: e.published_at,
                    text_hash: e.text_hash,
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    async fn track_revisions(&mut self, feed: &FeedState, snap_id: u64, now: i64, cur: Vec<Seen>) {
        let prev = self.previous_listing(&feed.url).await;
        let changes = revision::diff(&prev, &cur);
        for ch in changes {
            match ch {
                Change::Revised { id, title, summary } => {
                    // Only items we hold can be revised; anything else is
                    // just new to us and lands through the normal path.
                    let known = self
                        .reader
                        .call(move |c| reader::has_item(c, id))
                        .await
                        .unwrap_or(false);
                    if !known {
                        continue;
                    }
                    // Record the original text once, keyed by the previous
                    // snapshot, so history starts from what was first seen.
                    let first_change = self
                        .reader
                        .call(move |c| reader::item_revisions(c, id).map(|v| v.is_empty()))
                        .await
                        .unwrap_or(false);
                    if first_change {
                        if let Some(p) = prev.iter().find(|p| p.id == id) {
                            let prev_snap = self
                                .reader
                                .call({
                                    let url = feed.url.clone();
                                    move |c| reader::latest_snapshot(c, &url)
                                })
                                .await
                                .ok()
                                .flatten()
                                .map(|m| m.id)
                                .unwrap_or(0);
                            self.writer
                                .send(WriteMsg::Revision {
                                    item_id: id,
                                    snapshot_id: prev_snap,
                                    seen_at: now - 1,
                                    title: p.title.clone(),
                                    summary: p.summary.clone(),
                                    apply: false,
                                })
                                .await;
                        }
                    }
                    st_inc(&self.stats, &self.stats.revisions);
                    self.writer
                        .send(WriteMsg::Revision {
                            item_id: id,
                            snapshot_id: snap_id,
                            seen_at: now,
                            title,
                            summary,
                            apply: true,
                        })
                        .await;
                    self.writer
                        .send(WriteMsg::Event(ClusterEvent {
                            ts: now,
                            kind: EventKind::Revised,
                            cluster_id: None,
                            item_id: Some(id),
                            merged_into: None,
                            snapshot_id: Some(snap_id),
                        }))
                        .await;
                }
                Change::Withdrawn { id } => {
                    let known = self
                        .reader
                        .call(move |c| reader::has_item(c, id))
                        .await
                        .unwrap_or(false);
                    if !known {
                        continue;
                    }
                    st_inc(&self.stats, &self.stats.withdrawals);
                    self.writer
                        .send(WriteMsg::Event(ClusterEvent {
                            ts: now,
                            kind: EventKind::Withdrawn,
                            cluster_id: None,
                            item_id: Some(id),
                            merged_into: None,
                            snapshot_id: Some(snap_id),
                        }))
                        .await;
                }
            }
        }
        self.prev_seen.insert(feed.url.clone(), cur);
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
                    error: Some(e),
                    ..Default::default()
                };
            }
        };
        let Some(body) = fetched.body else {
            st.inc(&st.fetch_304);
            debug!(url = %feed.url, "304 not modified");
            return Processed {
                outcome: Outcome::Unchanged,
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                resolved_url: fetched.resolved_url,
                ..Default::default()
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
                let mut health = feed.health.clone().unwrap_or_default();
                let d = health.observe(
                    FetchObs {
                        at: fetched_at,
                        parsed: false,
                        new_items: 0,
                        dup_items: 0,
                        skew_secs: None,
                        future_items: 0,
                    },
                    fetched_at,
                );
                if let (true, Some(r)) = (d.quarantined, &d.reason) {
                    warn!(url = %feed.url, reason = %r, "feed quarantined");
                }
                return Processed {
                    outcome: Outcome::Err,
                    etag: fetched.etag,
                    last_modified: fetched.last_modified,
                    resolved_url: fetched.resolved_url,
                    error: Some(format!("parse: {e}")),
                    health: Some(health),
                    ..Default::default()
                };
            }
        };

        st.add(&st.items_seen, parsed.entries.len() as u64);

        // Revision tracking (plan §5.3a): diff this listing against the
        // feed's previous one before dedupe drops the items we already
        // have, since those are exactly the ones that can have changed.
        let cur: Vec<Seen> = parsed
            .entries
            .iter()
            .map(|e| Seen {
                id: crate::types::item_id(&e.canonical_url),
                title: e.title.clone(),
                summary: e.summary.clone(),
                published_at: e.published_at,
                text_hash: e.text_hash,
            })
            .collect();
        let prev_hashes: Vec<u64> = self
            .previous_listing(&feed.url)
            .await
            .iter()
            .map(|s| s.text_hash)
            .collect();

        let mut new_items = Vec::new();
        let mut seen_in_feed = HashSet::new();
        let mut dup_items = 0u32;
        let mut future_items = 0u32;
        let mut skews: Vec<i64> = Vec::new();
        let mut new_hashes: Vec<u64> = Vec::new();
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
            // Feeds emit dates in the future (wrong timezone math, embargo
            // slips). A future date would push the item outside every
            // cluster's window, so clamp it to when we saw it.
            let published_at = if e.published_at > fetched_at + MAX_FUTURE_SECS {
                st.inc(&st.future_dates);
                future_items += 1;
                fetched_at
            } else {
                e.published_at
            };
            if e.published_at > 0 {
                skews.push(fetched_at - e.published_at);
            }
            // Steward: a new URL carrying text we already saw in this feed's
            // last listing, or in another item of this fetch, is the feed
            // republishing itself.
            if prev_hashes
                .iter()
                .chain(new_hashes.iter())
                .any(|h| simhash::hamming(*h, e.text_hash) <= DUP_HAMMING)
            {
                dup_items += 1;
            }
            new_hashes.push(e.text_hash);
            new_items.push(Item {
                id,
                outlet_id,
                canonical_url: e.canonical_url,
                title: e.title,
                summary: e.summary,
                published_at,
                fetched_at,
                lang,
                text_hash: e.text_hash,
                cluster_id: None,
                cluster_rank: None,
                joined_at: None,
                snapshot_id: snap_id,
            });
        }

        // Steward (docs/steward.md §2): judge the feed on this fetch before
        // anything from it enters the store.
        skews.sort_unstable();
        let skew_secs = if skews.is_empty() {
            None
        } else {
            Some(skews[skews.len() / 2])
        };
        let mut health = feed.health.clone().unwrap_or_default();
        let decision = health.observe(
            FetchObs {
                at: fetched_at,
                parsed: true,
                new_items: new_items.len() as u32,
                dup_items,
                skew_secs,
                future_items,
            },
            fetched_at,
        );
        let held_items = if decision.ingest {
            // Revision tracking (plan §5.3a) only for a feed we are ingesting:
            // a quarantined feed's listing is not evidence of a withdrawal.
            self.track_revisions(feed, snap_id, fetched_at, cur).await;
            0
        } else {
            let n = new_items.len() as u32;
            if n > 0 || feed.health.as_ref().is_none_or(|h| h.quarantine.is_none()) {
                warn!(
                    url = %feed.url,
                    held = n,
                    reason = decision.reason.as_deref().unwrap_or("quarantined"),
                    "feed quarantined; items held"
                );
            }
            st.add(&st.items_held, n as u64);
            new_items.clear();
            n
        };

        st.add(&st.items_new, new_items.len() as u64);
        debug!(url = %feed.url, status = fetched.status, new = new_items.len(), held = held_items, "fetched");
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
            outcome: if new_items.is_empty() {
                Outcome::Unchanged
            } else {
                Outcome::Ok
            },
            etag: fetched.etag,
            last_modified: fetched.last_modified,
            resolved_url: fetched.resolved_url,
            error: None,
            new_items,
            health: Some(health),
        }
    }
}
