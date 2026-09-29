//! The nightly job (plan §5.6): once a day at a configured UTC hour,
//! export the store into a dated directory and apply snapshot retention.
//! Runs inside `feed run` when `export.nightly` is true.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{error, info};

use crate::config::Config;
use crate::export::{self, Format};
use crate::snapshot::{civil_from_unix, unix_from_civil};
use crate::stats::now;
use crate::store::Reader;

/// Next `hour_utc:00` strictly after `now`.
pub fn next_run_after(now: i64, hour_utc: u8) -> i64 {
    let (y, m, d) = civil_from_unix(now);
    let today = unix_from_civil(y, m, d) + hour_utc as i64 * 3600;
    if today > now {
        today
    } else {
        today + 86_400
    }
}

/// `<out_dir>/<yyyy-mm-dd>` for the day containing `ts`.
pub fn dated_dir(out_dir: &std::path::Path, ts: i64) -> PathBuf {
    let (y, m, d) = civil_from_unix(ts);
    out_dir.join(format!("{y:04}-{m:02}-{d:02}"))
}

pub fn run_once(
    cfg: &Config,
    reader: &Reader,
    ts: i64,
) -> Result<export::Summary, Box<dyn std::error::Error>> {
    let format: Format = cfg.export.format.parse()?;
    let dir = dated_dir(&cfg.export.out_dir, ts);
    let summary = export::run(
        reader,
        format,
        &dir,
        ts,
        crate::cluster::propagation::Thresholds::from_config(cfg),
    )?;
    if cfg.snapshots.enabled && cfg.snapshots.dir.exists() {
        let removed = crate::snapshot::prune(&cfg.snapshots.dir, ts, cfg.snapshots.retention_days)?;
        if removed > 0 {
            info!(removed, "snapshot retention applied");
        }
    }
    Ok(summary)
}

pub async fn run(cfg: Arc<Config>, reader: Reader, mut shutdown: watch::Receiver<bool>) {
    info!(
        hour_utc = cfg.export.hour_utc,
        format = %cfg.export.format,
        out = %cfg.export.out_dir.display(),
        "nightly export scheduled"
    );
    loop {
        let t = now();
        let at = next_run_after(t, cfg.export.hour_utc);
        let wait = Duration::from_secs((at - t).max(1) as u64);
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(wait) => {}
        }
        let (cfg2, reader2) = (cfg.clone(), reader.clone());
        let ts = now();
        // Boxed errors aren't Send; carry the message across the thread.
        match tokio::task::spawn_blocking(move || {
            run_once(&cfg2, &reader2, ts).map_err(|e| e.to_string())
        })
        .await
        {
            Ok(Ok(s)) => info!(
                items = s.items,
                clusters = s.clusters,
                events = s.events,
                "nightly export complete"
            ),
            Ok(Err(e)) => error!("nightly export failed: {e}"),
            Err(e) => error!("nightly export task panicked: {e}"),
        }
    }
    info!("nightly job stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_next_hour_boundary() {
        let day = unix_from_civil(2024, 1, 1);
        assert_eq!(next_run_after(day, 2), day + 2 * 3600);
        assert_eq!(
            next_run_after(day + 2 * 3600, 2),
            day + 86_400 + 2 * 3600,
            "exactly on time means tomorrow"
        );
        assert_eq!(next_run_after(day + 5 * 3600, 2), day + 86_400 + 2 * 3600);
        assert_eq!(
            dated_dir(std::path::Path::new("out"), day + 100),
            PathBuf::from("out/2024-01-01")
        );
    }

    #[test]
    fn run_once_exports_into_a_dated_directory() {
        let dir = std::env::temp_dir().join(format!("kg-nightly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (reader, _) = crate::export::fixture::populate(&dir);
        let mut cfg = Config::default();
        cfg.export.out_dir = dir.join("export");
        cfg.snapshots.dir = dir.join("snapshots");
        let ts = unix_from_civil(2024, 3, 5) + 7200;
        let s = run_once(&cfg, &reader, ts).unwrap();
        assert_eq!(s.items, 3);
        assert!(dir.join("export/2024-03-05/items.ndjson").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
