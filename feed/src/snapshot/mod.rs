//! Raw feed snapshot archival (plan §5.1). Every non-304 body is written
//! zstd-compressed under `<dir>/<yyyy>/<mm>/<dd>/<id>.zst` before parsing,
//! so upstream edits and deletions remain provable later.

use std::path::{Path, PathBuf};

use xxhash_rust::xxh64::xxh64;

pub fn snapshot_id(feed_url: &str, fetched_at: i64) -> u64 {
    let mut buf = Vec::with_capacity(feed_url.len() + 8);
    buf.extend_from_slice(feed_url.as_bytes());
    buf.extend_from_slice(&fetched_at.to_le_bytes());
    xxh64(&buf, 0)
}

pub fn body_hash(body: &[u8]) -> u64 {
    xxh64(body, 0)
}

/// Relative path under the snapshot dir for a given id and fetch time.
pub fn relative_path(id: u64, fetched_at: i64) -> PathBuf {
    let (y, m, d) = civil_from_unix(fetched_at);
    PathBuf::from(format!("{y:04}/{m:02}/{d:02}/{id:016x}.zst"))
}

/// Compress and write `body`. Returns the path relative to `dir`.
pub fn write(
    dir: &Path,
    id: u64,
    fetched_at: i64,
    body: &[u8],
    level: i32,
) -> std::io::Result<PathBuf> {
    let rel = relative_path(id, fetched_at);
    let full = dir.join(&rel);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let compressed = zstd::encode_all(body, level)?;
    std::fs::write(&full, compressed)?;
    Ok(rel)
}

pub fn read(dir: &Path, rel: &Path) -> std::io::Result<Vec<u8>> {
    let compressed = std::fs::read(dir.join(rel))?;
    zstd::decode_all(compressed.as_slice())
}

/// Delete day directories older than `retention_days`. Returns files removed.
#[allow(dead_code)] // wired to `feed prune` and the nightly job (M6)
pub fn prune(dir: &Path, now: i64, retention_days: u32) -> std::io::Result<usize> {
    let cutoff = now - retention_days as i64 * 86_400;
    let mut removed = 0;
    let Ok(years) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    for y in years.flatten() {
        for m in std::fs::read_dir(y.path())?.flatten() {
            for d in std::fs::read_dir(m.path())?.flatten() {
                let parse =
                    |e: &std::fs::DirEntry| e.file_name().to_string_lossy().parse::<i64>().ok();
                let (Some(yy), Some(mm), Some(dd)) = (parse(&y), parse(&m), parse(&d)) else {
                    continue;
                };
                let day_start = unix_from_civil(yy, mm as u32, dd as u32);
                if day_start + 86_400 <= cutoff {
                    removed += std::fs::read_dir(d.path())?.count();
                    std::fs::remove_dir_all(d.path())?;
                }
            }
        }
    }
    Ok(removed)
}

/// Bytes and files under the snapshot directory.
pub fn usage(dir: &Path) -> std::io::Result<(u64, usize)> {
    let mut bytes = 0;
    let mut files = 0;
    for d in day_dirs(dir)? {
        for f in std::fs::read_dir(&d)?.flatten() {
            if let Ok(m) = f.metadata() {
                if m.is_file() {
                    bytes += m.len();
                    files += 1;
                }
            }
        }
    }
    Ok((bytes, files))
}

/// Day directories, oldest first.
fn day_dirs(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let Ok(years) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for y in years.flatten() {
        for m in std::fs::read_dir(y.path())?.flatten() {
            for d in std::fs::read_dir(m.path())?.flatten() {
                if d.path().is_dir() {
                    out.push(d.path());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The disk budget (docs/steward.md §5): remove whole days, oldest
/// first, until the directory is at or under `max_bytes`. 0 means no
/// budget. The newest day is never removed. Returns (files, bytes) freed.
pub fn enforce_budget(dir: &Path, max_bytes: u64) -> std::io::Result<(usize, u64)> {
    if max_bytes == 0 {
        return Ok((0, 0));
    }
    let (mut total, _) = usage(dir)?;
    let days = day_dirs(dir)?;
    let mut removed = (0, 0);
    for d in days.iter().take(days.len().saturating_sub(1)) {
        if total <= max_bytes {
            break;
        }
        let (bytes, files) = usage_of_day(d)?;
        std::fs::remove_dir_all(d)?;
        total = total.saturating_sub(bytes);
        removed.0 += files;
        removed.1 += bytes;
    }
    Ok(removed)
}

fn usage_of_day(day: &Path) -> std::io::Result<(u64, usize)> {
    let mut bytes = 0;
    let mut files = 0;
    for f in std::fs::read_dir(day)?.flatten() {
        if let Ok(m) = f.metadata() {
            if m.is_file() {
                bytes += m.len();
                files += 1;
            }
        }
    }
    Ok((bytes, files))
}

/// Howard Hinnant's civil-from-days, so we don't need a date crate.
pub fn civil_from_unix(ts: i64) -> (i64, u32, u32) {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[allow(dead_code)] // used by prune
pub fn unix_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1));
        assert_eq!(civil_from_unix(1_704_067_200), (2024, 1, 1));
        assert_eq!(civil_from_unix(951_782_400), (2000, 2, 29));
        for ts in [0, 86_399, 951_782_400, 1_704_067_200, 4_102_444_800] {
            let (y, m, d) = civil_from_unix(ts);
            assert_eq!(unix_from_civil(y, m, d), ts - ts.rem_euclid(86_400));
        }
    }

    #[test]
    fn write_read_prune() {
        let dir = std::env::temp_dir().join(format!("kg-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let id = snapshot_id("https://x/f", 1_704_067_200);
        let rel = write(&dir, id, 1_704_067_200, b"<rss>hello</rss>", 3).unwrap();
        assert_eq!(rel, PathBuf::from(format!("2024/01/01/{id:016x}.zst")));
        assert_eq!(read(&dir, &rel).unwrap(), b"<rss>hello</rss>");
        assert_eq!(prune(&dir, 1_704_067_200 + 10 * 86_400, 30).unwrap(), 0);
        assert_eq!(prune(&dir, 1_704_067_200 + 40 * 86_400, 30).unwrap(), 1);
        assert!(read(&dir, &rel).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_budget_removes_oldest_days_and_keeps_the_newest() {
        let dir = std::env::temp_dir().join(format!("kg-snapbudget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let day = 86_400;
        let body = vec![b'x'; 4096]; // compresses small; sizes are what matter
        for k in 0..4 {
            let t = 1_704_067_200 + k * day;
            write(&dir, snapshot_id("https://x/f", t), t, &body, 3).unwrap();
        }
        let (total, files) = usage(&dir).unwrap();
        assert_eq!(files, 4);
        let per = total / 4;
        assert_eq!(enforce_budget(&dir, 0).unwrap(), (0, 0), "0 is no budget");
        // Room for two days: the two oldest go.
        let (f, b) = enforce_budget(&dir, per * 2 + 1).unwrap();
        assert_eq!((f, b), (2, per * 2));
        assert_eq!(usage(&dir).unwrap().1, 2);
        // A budget no day fits still keeps the newest day.
        assert_eq!(enforce_budget(&dir, 1).unwrap().0, 1);
        assert_eq!(usage(&dir).unwrap().1, 1);
        assert!(dir.join("2024/01/04").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
