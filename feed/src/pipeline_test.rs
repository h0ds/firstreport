//! Offline end-to-end test: fixture bytes → poller → store → API. This is
//! the shape the M3 replay harness generalizes.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use bytes::Bytes;
use tower::ServiceExt;

use crate::api::{router, AppState};
use crate::config::{Config, Feeds, OutletEntry, Outlets};
use crate::fetch::client::Fetched;
use crate::fetch::scheduler::Outcome;
use crate::parse::outlet::Resolver;
use crate::poller::Poller;
use crate::stats::Stats;
use crate::store::{self, Reader};

const FIXTURE: &[u8] = include_bytes!("../fixtures/feeds/example-rss.xml");
const REVISED: &[u8] = include_bytes!("../fixtures/feeds/example-rss-revised.xml");

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn fixture_through_store_and_api() {
    let dir = std::env::temp_dir().join(format!("kg-pipe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut cfg = Config::default();
    cfg.store.path = dir.join("feed.db");
    cfg.snapshots.dir = dir.join("snapshots");
    let cfg = Arc::new(cfg);

    let outlets = Outlets {
        outlets: vec![
            OutletEntry {
                id: "example".into(),
                name: "Example".into(),
                domains: vec!["example.com".into()],
            },
            OutletEntry {
                id: "bbc".into(),
                name: "BBC".into(),
                domains: vec!["bbc.co.uk".into()],
            },
        ],
    };
    let feeds: Feeds = toml::from_str(
        "[[feed]]\nurl = \"https://www.example.com/feed.xml\"\noutlet = \"example\"\n",
    )
    .unwrap();

    let (writer, thread) = store::spawn_writer(&cfg.store.path).unwrap();
    let reader = Reader::open(&cfg.store.path).unwrap();
    {
        let mut c = store::open(&cfg.store.path).unwrap();
        store::writer::sync_outlets(&mut c, &outlets).unwrap();
    }
    let stats = Arc::new(Stats::default());
    let resolver = Arc::new(Resolver::from_config(&outlets));
    let mut poller = Poller::new(
        cfg.clone(),
        resolver,
        reader.clone(),
        writer.clone(),
        stats.clone(),
    )
    .unwrap();
    let states = Poller::build_states(&feeds, vec![], 900, 1_704_100_000);
    let feed = states[0].clone();

    let fetched = || Fetched {
        status: 200,
        body: Some(Bytes::from_static(FIXTURE)),
        etag: Some("W/\"abc\"".into()),
        last_modified: None,
        resolved_url: None,
    };

    // First fetch: three entries, all new.
    let p = poller.process(&feed, 1_704_100_000, Ok(fetched())).await;
    assert_eq!(p.outcome, Outcome::Ok);
    assert_eq!(p.etag.as_deref(), Some("W/\"abc\""));
    assert_eq!(p.new_items.len(), 3);
    let rates = &p.new_items[0];
    assert_eq!(
        rates.canonical_url,
        "https://example.com/world/2024/01/01/rates"
    );
    assert_eq!(rates.outlet_id, "example");
    assert_eq!(rates.lang, "en");
    assert_eq!(rates.published_at, 1_704_099_600);
    assert!(rates
        .summary
        .starts_with("The central bank kept its benchmark rate"));
    assert_eq!(
        p.new_items[1].canonical_url,
        "https://example.com/world/2024/01/01/flooding"
    );
    assert_eq!(
        p.new_items[2].outlet_id, "bbc",
        "outlet resolved from item host, not feed"
    );

    // Second fetch of the same body: everything deduped; a snapshot row is
    // recorded but it points at the first file rather than writing a copy.
    let p2 = poller.process(&feed, 1_704_100_900, Ok(fetched())).await;
    assert_eq!(p2.new_items.len(), 0);

    // Revised listing: the rates headline changed and the flooding item
    // vanished while an older item (the wire story) is still listed, so it
    // was withdrawn rather than aged out.
    writer.flush().await;
    let revised = Fetched {
        status: 200,
        body: Some(Bytes::from_static(REVISED)),
        etag: None,
        last_modified: None,
        resolved_url: None,
    };
    // A fresh poller has no in-memory listing, so this also exercises the
    // restart path: the previous listing is parsed from the archived
    // snapshot on disk.
    let mut poller_after_restart = Poller::new(
        cfg.clone(),
        Arc::new(Resolver::from_config(&outlets)),
        reader.clone(),
        writer.clone(),
        stats.clone(),
    )
    .unwrap();
    poller_after_restart.seed_bloom(&p.new_items.iter().map(|i| i.id).collect::<Vec<_>>());
    let pr = poller_after_restart
        .process(&feed, 1_704_101_000, Ok(revised))
        .await;
    assert_eq!(pr.new_items.len(), 0, "revised and known items are not new");
    drop(poller_after_restart);

    // 304 and error paths.
    let p3 = poller
        .process(
            &feed,
            1_704_101_800,
            Ok(Fetched {
                status: 304,
                body: None,
                etag: None,
                last_modified: None,
                resolved_url: None,
            }),
        )
        .await;
    assert_eq!(p3.outcome, Outcome::Unchanged);
    assert_eq!(
        p2.outcome,
        Outcome::Unchanged,
        "identical body counts as unchanged for adaptive polling"
    );
    let p4 = poller
        .process(&feed, 1_704_102_700, Err("boom".into()))
        .await;
    assert_eq!(p4.outcome, Outcome::Err);
    // Unparseable body: error outcome, but the snapshot is still recorded.
    let p5 = poller
        .process(
            &feed,
            1_704_103_600,
            Ok(Fetched {
                status: 200,
                body: Some(Bytes::from_static(b"<html>nope</html>")),
                etag: None,
                last_modified: None,
                resolved_url: None,
            }),
        )
        .await;
    assert_eq!(p5.outcome, Outcome::Err);

    writer.send(store::WriteMsg::FeedState(feed.clone())).await;
    writer.flush().await;

    // API over the same store.
    let app = router(AppState {
        reader: reader.clone(),
        stats: stats.clone(),
        snapshot_dir: cfg.snapshots.dir.clone(),
        hamming_threshold: 3,
        ledger_url: "http://ledger.test".into(),
    });
    let get = |uri: &str| Request::builder().uri(uri).body(Body::empty()).unwrap();

    let v = body_json(app.clone().oneshot(get("/v1/items?limit=2")).await.unwrap()).await;
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
    let next = v["next"].as_str().unwrap().to_string();
    let v2 = body_json(
        app.clone()
            .oneshot(get(&format!("/v1/items?limit=2&cursor={next}")))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v2["items"].as_array().unwrap().len(), 1);
    assert!(v2["next"].is_null());

    let v = body_json(
        app.clone()
            .oneshot(get("/v1/items?outlet=bbc"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["items"][0]["title"], "Syndicated: agency wire story");

    let v = body_json(
        app.clone()
            .oneshot(get(&format!("/v1/items/{}", rates.id)))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        v["canonical_url"],
        "https://example.com/world/2024/01/01/rates"
    );
    assert_eq!(
        app.clone()
            .oneshot(get("/v1/items/1"))
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        app.clone()
            .oneshot(get("/v1/items/zzz"))
            .await
            .unwrap()
            .status(),
        400
    );

    // Lookup: a URL feed holds, in any of the forms the canonicalizer folds.
    let v = body_json(
        app.clone()
            .oneshot(get(
                "/v1/lookup?url=https://www.example.com/world/2024/01/01/rates?utm_source=x",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["matched"], "item", "{v}");
    assert_eq!(v["item"]["id"], rates.id);
    assert_eq!(v["host"], "example.com");
    assert_eq!(v["outlet"]["id"], "example");
    assert_eq!(
        v["outlet"]["ledger"],
        "http://ledger.test/v1/outlets/example"
    );
    assert_eq!(v["checked"].as_array().unwrap().len(), 2);
    // A URL feed has never seen on a host it follows, including a subdomain.
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/lookup?url=https://news.bbc.co.uk/never/seen"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["matched"], "outlet", "{v}");
    assert!(v["item"].is_null());
    assert_eq!(v["outlet"]["id"], "bbc");
    // A host feed knows nothing about.
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/lookup?url=https://nowhere.invalid/x"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["matched"], "none", "{v}");
    assert_eq!(v["host"], "nowhere.invalid");
    assert!(v["outlet"].is_null());
    // Quoted text with a near copy in the store; the fixture is dated 2024,
    // so the window is opened explicitly.
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/lookup?since=0&text=Syndicated:%20agency%20wire%20story%20Agency%20wire%20copy%20republished%20by%20a%20different%20outlet."))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["matched"], "text", "{v}");
    assert!(!v["text_matches"]["items"].as_array().unwrap().is_empty());
    for bad in [
        "/v1/lookup",
        "/v1/lookup?url=not%20a%20url",
        "/v1/lookup?url=ftp://x/y",
    ] {
        assert_eq!(
            app.clone().oneshot(get(bad)).await.unwrap().status(),
            400,
            "{bad}"
        );
    }

    let v = body_json(
        app.clone()
            .oneshot(get("/v1/snapshots?feed=https://www.example.com/feed.xml"))
            .await
            .unwrap(),
    )
    .await;
    let snaps = v["items"].as_array().unwrap();
    assert_eq!(
        snaps.len(),
        4,
        "three good bodies and one unparseable; 304 and error produce none"
    );
    assert_eq!(snaps[0]["item_count"], 0);
    let first_snap = snaps.iter().find(|s| s["item_count"] == 3).unwrap();
    let id = first_snap["id"].as_u64().unwrap();
    let resp = app
        .clone()
        .oneshot(get(&format!("/v1/snapshots/{id}/body")))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        &body[..],
        FIXTURE,
        "archived body round-trips byte for byte"
    );

    let v = body_json(app.clone().oneshot(get("/v1/feeds")).await.unwrap()).await;
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["url"], "https://www.example.com/feed.xml");
    assert_eq!(v[0]["base_interval_secs"], 900);
    assert!(v[0].get("etag").is_none(), "validators are not exposed");
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/feeds?filter=dead"))
            .await
            .unwrap(),
    )
    .await;
    assert!(v.as_array().unwrap().is_empty());

    let v = body_json(app.clone().oneshot(get("/v1/outlets")).await.unwrap()).await;
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1]["feeds"][0], "https://www.example.com/feed.xml");
    let id = v[1]["id"].as_str().unwrap().to_string();
    assert_eq!(
        v[1]["ledger"],
        format!("http://ledger.test/v1/outlets/{id}")
    );
    let one = body_json(
        app.clone()
            .oneshot(get(&format!("/v1/outlets/{id}")))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(one["id"], id);
    assert_eq!(
        one["ledger_lookup"],
        format!("http://ledger.test/lookup?q={id}")
    );
    let r = app.clone().oneshot(get("/v1/outlets/nope")).await.unwrap();
    assert_eq!(r.status(), 404);

    // Revisions: the rates item carries its original text and the new one;
    // the item itself now reads the new title. The flooding item was
    // withdrawn. Both show up in the event log.
    let v = body_json(
        app.clone()
            .oneshot(get(&format!("/v1/items/{}/revisions", rates.id)))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        v["current"]["title"],
        "Central bank holds rates, drops hint of cuts after data surprise"
    );
    let revs = v["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 2);
    assert_eq!(
        revs[0]["title"],
        "Central bank holds rates, signals cuts may come later in the year"
    );
    assert_eq!(
        revs[1]["title"],
        "Central bank holds rates, drops hint of cuts after data surprise"
    );
    assert!(revs[0]["seen_at"].as_i64().unwrap() < revs[1]["seen_at"].as_i64().unwrap());
    let flooding_id = p.new_items[1].id;
    let v = body_json(
        app.clone()
            .oneshot(get(&format!("/v1/items/{flooding_id}/revisions")))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        v["revisions"].as_array().unwrap().is_empty(),
        "unchanged text has no history"
    );
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/events?limit=50"))
            .await
            .unwrap(),
    )
    .await;
    let kinds: Vec<(String, u64)> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap().to_string(),
                e["item_id"].as_u64().unwrap(),
            )
        })
        .collect();
    assert!(kinds.contains(&("revised".into(), rates.id)), "{kinds:?}");
    assert!(
        kinds.contains(&("withdrawn".into(), flooding_id)),
        "{kinds:?}"
    );
    assert!(
        !kinds
            .iter()
            .any(|(k, id)| k == "withdrawn" && *id == p.new_items[2].id),
        "still listed"
    );

    // Syndication: the wire story's simhash finds itself; an unrelated hash finds nothing.
    let wire = &p.new_items[2];
    let v = body_json(
        app.clone()
            .oneshot(get(&format!(
                "/v1/syndication?text_hash={}&since=0",
                wire.text_hash
            )))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["id"], wire.id);
    let v = body_json(
        app.clone()
            .oneshot(get("/v1/syndication?text_hash=0x5555555555555555&since=0"))
            .await
            .unwrap(),
    )
    .await;
    assert!(v["items"].as_array().unwrap().is_empty());

    let v = body_json(app.clone().oneshot(get("/v1/stats")).await.unwrap()).await;
    assert_eq!(v["contract_version"], 1);
    assert_eq!(v["store"]["items_total"], 3);
    assert_eq!(v["store"]["snapshots_total"], 4);
    assert_eq!(v["process"]["fetches"], 6);
    assert_eq!(v["process"]["fetch_304"], 1);
    assert_eq!(v["process"]["fetch_errors"], 1);
    assert_eq!(v["process"]["parse_errors"], 1);
    assert_eq!(v["process"]["items_seen"], 8);
    assert_eq!(v["process"]["items_new"], 3);
    assert_eq!(v["process"]["revisions"], 1);
    assert_eq!(v["process"]["withdrawals"], 1);

    // Embedder: only when an ONNX Runtime library is available.
    if let Ok(lib) = std::env::var("KAGAMI_ORT_LIB") {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/models");
        let mut ecfg = Config::default();
        ecfg.embed.runtime_lib = lib.into();
        ecfg.embed.model_path = root.join("tiny.onnx");
        ecfg.embed.vocab_path = root.join("tiny-vocab.txt");
        ecfg.embed.batch_size = 2;
        ecfg.embed.batch_wait_ms = 20;
        let model = crate::embed::load_from_config(&ecfg, Some(1)).unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        let task = tokio::spawn(crate::embed::run(
            Arc::new(ecfg),
            model,
            rx,
            out_tx,
            writer.clone(),
            stats.clone(),
        ));
        let backlog = reader
            .with(|c| store::reader::unembedded_since(c, 0, 10))
            .unwrap();
        assert_eq!(backlog.len(), 3);
        for it in backlog {
            tx.send(it).await.unwrap();
        }
        drop(tx);
        let mut got = 0;
        while let Some(e) = out_rx.recv().await {
            assert_eq!(e.vector.len(), 16);
            got += 1;
        }
        assert_eq!(got, 3);
        task.await.unwrap();
        writer.flush().await;
        assert_eq!(
            reader
                .with(|c| store::reader::unembedded_since(c, 0, 10))
                .unwrap()
                .len(),
            0
        );
        let v = reader
            .with(|c| store::reader::get_embedding(c, rates.id))
            .unwrap()
            .unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
        let v = body_json(app.clone().oneshot(get("/v1/stats")).await.unwrap()).await;
        assert_eq!(v["store"]["items_embedded"], 3);
        assert_eq!(v["process"]["items_embedded"], 3);
        assert_eq!(
            v["process"]["embed_batches"], 2,
            "batch_size 2 over 3 items"
        );
        assert!(v["process"]["embed_latency"]["samples"].as_u64().unwrap() >= 1);
    } else {
        eprintln!("KAGAMI_ORT_LIB unset; embedder stage skipped");
    }

    drop(writer);
    drop(poller); // holds a writer clone; the channel must close for the thread to exit
    thread.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
