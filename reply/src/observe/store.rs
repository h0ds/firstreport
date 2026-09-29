//! Sightings on disk: one NDJSON file per UTC day under `data/sightings/`,
//! appended as they arrive, pruned after `retain_days`. Observations in
//! `data/observations.ndjson`, latest per (kind, subject) winning. Account
//! ages in `data/accounts.json`, keyed by account hash.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::Observation;

use super::Sighting;

pub struct Store {
    dir: PathBuf,
    obs_path: PathBuf,
    ages_path: PathBuf,
    pub ages: HashMap<String, u32>,
}

fn day(ts: i64) -> String {
    crate::facts::date_time(ts)[..10].to_string()
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, String> {
        let dir = data.join("sightings");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let ages_path = data.join("accounts.json");
        let ages = crate::model::load_json(&ages_path).unwrap_or_default();
        Ok(Self {
            dir,
            obs_path: data.join("observations.ndjson"),
            ages_path,
            ages,
        })
    }

    pub fn append(&self, s: &Sighting) -> Result<(), String> {
        let path = self.dir.join(format!("{}.ndjson", day(s.at)));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let line = serde_json::to_string(s).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| e.to_string())
    }

    /// Every sighting with `at >= since`, oldest first, ages filled in from
    /// the cache where the sighting has none.
    pub fn load(&self, since: i64) -> Result<Vec<Sighting>, String> {
        let mut out = Vec::new();
        let first_day = day(since);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map_err(|e| format!("{}: {e}", self.dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().as_ref() >= first_day.as_str())
            })
            .collect();
        files.sort();
        for p in files {
            for line in std::fs::read_to_string(&p)
                .map_err(|e| format!("{}: {e}", p.display()))?
                .lines()
            {
                if let Ok(mut s) = serde_json::from_str::<Sighting>(line) {
                    if s.at >= since {
                        if s.account_age_days.is_none() {
                            s.account_age_days = self.ages.get(&s.account).copied();
                        }
                        out.push(s);
                    }
                }
            }
        }
        out.sort_by_key(|s| s.at);
        Ok(out)
    }

    /// Delete day files older than `retain_days` before `now`.
    pub fn prune(&self, now: i64, retain_days: u32) -> Result<usize, String> {
        let cutoff = day(now - retain_days as i64 * 86_400);
        let mut n = 0;
        for e in std::fs::read_dir(&self.dir).map_err(|e| e.to_string())? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.file_name()
                .is_some_and(|f| f.to_string_lossy().as_ref() < cutoff.as_str())
            {
                std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn remember_age(&mut self, account: &str, age_days: u32) {
        self.ages.insert(account.to_string(), age_days);
    }

    pub fn save_ages(&self) -> Result<(), String> {
        let body = serde_json::to_string(&self.ages).map_err(|e| e.to_string())?;
        std::fs::write(&self.ages_path, body)
            .map_err(|e| format!("{}: {e}", self.ages_path.display()))
    }

    pub fn append_observations(&self, obs: &[Observation]) -> Result<(), String> {
        if obs.is_empty() {
            return Ok(());
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.obs_path)
            .map_err(|e| format!("{}: {e}", self.obs_path.display()))?;
        for o in obs {
            writeln!(
                f,
                "{}",
                serde_json::to_string(o).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Latest observation per (kind, subject).
    pub fn observations(&self) -> Result<Vec<Observation>, String> {
        let Ok(text) = std::fs::read_to_string(&self.obs_path) else {
            return Ok(vec![]);
        };
        let mut latest: BTreeMap<(String, String), Observation> = BTreeMap::new();
        for line in text.lines() {
            if let Ok(o) = serde_json::from_str::<Observation>(line) {
                latest.insert((o.kind.clone(), o.subject.clone()), o);
            }
        }
        Ok(latest.into_values().collect())
    }
}

/// Observations about one subject, for the resolver: the URL as a subject
/// and, when given, the text hash.
pub fn observations_for(
    data: &Path,
    url: Option<&str>,
    text_hash: Option<u64>,
) -> Vec<Observation> {
    let Ok(store) = Store::open(data) else {
        return vec![];
    };
    let Ok(all) = store.observations() else {
        return vec![];
    };
    let subjects: Vec<String> = url
        .and_then(super::subject_url)
        .into_iter()
        .chain(text_hash.map(|h| format!("text:{h:016x}")))
        .collect();
    all.into_iter()
        .filter(|o| o.kind != "first_movers" && subjects.contains(&o.subject))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_load_prune_and_observations() {
        let dir = std::env::temp_dir().join(format!("kg-reply-obs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = Store::open(&dir).unwrap();
        let s = |at: i64, acc: &str| Sighting {
            platform: "t".into(),
            account: acc.into(),
            account_age_days: None,
            at,
            url: Some("https://x.example/a".into()),
            text_hash: None,
        };
        st.append(&s(1_704_099_600, "a")).unwrap(); // 2024-01-01
        st.append(&s(1_704_186_000, "b")).unwrap(); // 2024-01-02
        st.append(&s(1_704_272_400, "a")).unwrap(); // 2024-01-03
        st.remember_age("a", 7);
        st.save_ages().unwrap();
        let st = Store::open(&dir).unwrap();
        let all = st.load(0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].account_age_days, Some(7));
        assert_eq!(st.load(1_704_186_000).unwrap().len(), 2);
        assert_eq!(st.prune(1_704_272_400, 1).unwrap(), 1);
        assert_eq!(st.load(0).unwrap().len(), 2);

        let o = |kind: &str, subject: &str, count: u32| Observation {
            kind: kind.into(),
            subject: subject.into(),
            window_secs: 600,
            count,
            denominator: 10,
            detail: serde_json::Value::Null,
            computed_at: 1,
        };
        st.append_observations(&[
            o("burst", "https://x.example/a", 5),
            o("burst", "https://x.example/a", 6),
            o("first_movers", "acc", 3),
        ])
        .unwrap();
        let obs = st.observations().unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs.iter().find(|o| o.kind == "burst").unwrap().count, 6);
        let mine = observations_for(&dir, Some("https://www.x.example/a?utm_source=q"), None);
        assert_eq!(mine.len(), 1);
        assert!(observations_for(&dir, Some("https://other.example/"), None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
