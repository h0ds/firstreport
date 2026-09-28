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

    // Second fetch of the same body: everything deduped, snapshot still archived.
    let p2 = poller.process(&feed, 1_704_100_900, Ok(fetched())).await;
    assert_eq!(p2.new_items.len(), 0);

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
            }),
        )
        .await;
    assert_eq!(p3.outcome, Outcome::Ok);
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
        3,
        "two good bodies and one unparseable; 304 and error produce none"
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

    let v = body_json(app.clone().oneshot(get("/v1/outlets")).await.unwrap()).await;
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1]["feeds"][0], "https://www.example.com/feed.xml");

    let v = body_json(app.clone().oneshot(get("/v1/stats")).await.unwrap()).await;
    assert_eq!(v["contract_version"], 1);
    assert_eq!(v["store"]["items_total"], 3);
    assert_eq!(v["store"]["snapshots_total"], 3);
    assert_eq!(v["process"]["fetches"], 5);
    assert_eq!(v["process"]["fetch_304"], 1);
    assert_eq!(v["process"]["fetch_errors"], 1);
    assert_eq!(v["process"]["parse_errors"], 1);
    assert_eq!(v["process"]["items_seen"], 6);
    assert_eq!(v["process"]["items_new"], 3);

    // Embedder: only when an ONNX Runtime library is available.
    if let Ok(lib) = std::env::var("KAGAMI_ORT_LIB") {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/models");
        let mut ecfg = Config::default();
        ecfg.embed.runtime_lib = lib.into();
        ecfg.embed.model_path = root.join("tiny.onnx");
        ecfg.embed.vocab_path = root.join("tiny-vocab.txt");
        ecfg.embed.batch_size = 2;
        ecfg.embed.batch_wait_ms = 20;
        let model = crate::embed::load_from_config(&ecfg).unwrap();
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
