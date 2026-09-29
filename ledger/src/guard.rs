//! Steward for the ledger (docs/steward.md §4): the change guard, the
//! review file it writes, and the conflict list. Every rule is a count
//! over claims and names the source or importer, never the outlet.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Claim;
use crate::store::{self, Ledger, StoreError};

/// How an import writes: not at all, through the guard, or past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Preview,
    Write,
    Force,
}

impl WriteMode {
    pub fn from_flags(write: bool, force: bool) -> Self {
        match (write, force) {
            (false, _) => Self::Preview,
            (true, false) => Self::Write,
            (true, true) => Self::Force,
        }
    }
    pub fn writes(self) -> bool {
        self != Self::Preview
    }
    pub fn force(self) -> bool {
        self == Self::Force
    }
}

/// The guard applies once a source has this many claims over the
/// outlets being imported; below it a refresh is too small to judge.
pub const GUARD_MIN_EXISTING: usize = 6;
/// Rule: more than a third of a source's existing claims removed or
/// changed in one run is held for review.
pub const GUARD_MAX_CHANGE: f64 = 1.0 / 3.0;

/// What a run would do to one source's claims, as counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub source_id: String,
    /// Outlets in the run.
    pub outlets: usize,
    /// The source's claims over those outlets before the run.
    pub existing: usize,
    /// Existing claims the run states again (same outlet, field, value).
    pub kept: usize,
    /// Existing claims the run drops or restates with another value.
    pub removed_or_changed: usize,
    /// Claims the run adds that were not there.
    pub added: usize,
}

impl ChangeSummary {
    pub fn ratio(&self) -> f64 {
        if self.existing == 0 {
            0.0
        } else {
            self.removed_or_changed as f64 / self.existing as f64
        }
    }
    /// The one-sentence rule.
    pub fn held(&self) -> bool {
        self.existing >= GUARD_MIN_EXISTING && self.ratio() > GUARD_MAX_CHANGE
    }
}

fn key(c: &Claim) -> (String, String, String) {
    (c.outlet_id.clone(), c.field.clone(), c.value.clone())
}

/// Compare a run's fresh claims against what the ledger holds from the
/// same source for the same outlets.
pub fn summarise(
    ledger: &Ledger,
    source_id: &str,
    batch: &[(String, Vec<Claim>)],
) -> ChangeSummary {
    let mut existing: BTreeSet<(String, String, String)> = BTreeSet::new();
    for (outlet, _) in batch {
        for c in ledger.claims.get(outlet).map(Vec::as_slice).unwrap_or(&[]) {
            if c.source_id == source_id {
                existing.insert(key(c));
            }
        }
    }
    let fresh: BTreeSet<(String, String, String)> =
        batch.iter().flat_map(|(_, v)| v.iter().map(key)).collect();
    let kept = existing.intersection(&fresh).count();
    ChangeSummary {
        source_id: source_id.to_string(),
        outlets: batch.len(),
        existing: existing.len(),
        kept,
        removed_or_changed: existing.len() - kept,
        added: fresh.difference(&existing).count(),
    }
}

/// The outcome of committing a run.
#[derive(Debug, Clone, PartialEq)]
pub enum Commit {
    Written {
        outlets: usize,
        summary: ChangeSummary,
    },
    Held {
        path: PathBuf,
        summary: ChangeSummary,
    },
}

/// Write a run through the guard: the store when the change is within
/// the rule or `force` is set, a review file otherwise. Hand edits from
/// other sources are untouched either way.
pub fn commit_source(
    data_dir: &Path,
    ledger: &Ledger,
    source_id: &str,
    batch: Vec<(String, Vec<Claim>)>,
    force: bool,
    now: i64,
) -> Result<Commit, StoreError> {
    let summary = summarise(ledger, source_id, &batch);
    if summary.held() && !force {
        let path = write_review(data_dir, &summary, &batch, now)?;
        return Ok(Commit::Held { path, summary });
    }
    let outlets = batch.len();
    for (outlet, fresh) in batch {
        let existing = ledger.claims.get(&outlet).cloned().unwrap_or_default();
        let merged = store::merge_source(&existing, source_id, fresh);
        store::write_outlet(data_dir, &outlet, merged)?;
    }
    Ok(Commit::Written { outlets, summary })
}

pub fn review_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("review")
}

#[derive(Debug, Serialize, Deserialize)]
struct ReviewFile {
    review: ReviewHeader,
    #[serde(default, rename = "claim")]
    claims: Vec<Claim>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewHeader {
    pub at: i64,
    #[serde(flatten)]
    pub summary: ChangeSummary,
}

fn write_review(
    data_dir: &Path,
    summary: &ChangeSummary,
    batch: &[(String, Vec<Claim>)],
    now: i64,
) -> Result<PathBuf, StoreError> {
    let dir = review_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| StoreError(format!("{}: {e}", dir.display())))?;
    let mut claims: Vec<Claim> = batch.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
    claims.sort_by(|a, b| {
        (&a.outlet_id, &a.field, &a.value).cmp(&(&b.outlet_id, &b.field, &b.value))
    });
    let file = ReviewFile {
        review: ReviewHeader {
            at: now,
            summary: summary.clone(),
        },
        claims,
    };
    let text = toml::to_string_pretty(&file).map_err(|e| StoreError(format!("serialize: {e}")))?;
    let header = format!(
        "# Held by the change guard (docs/steward.md §4): {} would remove or change {} of its {} existing claims over {} outlets, more than a third.\n# Nothing was written to claims/. Read the claims below against the store; if the source's page merely changed shape, fix the importer.\n# To apply as they are, re-run the import with --write --force, then delete this file.\n\n",
        summary.source_id, summary.removed_or_changed, summary.existing, summary.outlets
    );
    let p = dir.join(format!("{}-{now}.toml", summary.source_id));
    std::fs::write(&p, header + &text).map_err(|e| StoreError(format!("{}: {e}", p.display())))?;
    Ok(p)
}

/// Review files not yet dealt with, oldest first.
pub fn pending_reviews(data_dir: &Path) -> Vec<(PathBuf, ReviewHeader)> {
    let dir = review_dir(data_dir);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out: Vec<(PathBuf, ReviewHeader)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let f: ReviewFile = toml::from_str(&text).ok()?;
            Some((p, f.review))
        })
        .collect();
    out.sort_by_key(|(_, h)| h.at);
    out
}

/// Two sources disagreeing on one field of one outlet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub outlet_id: String,
    pub field: String,
    /// (source id, value), one per source, sorted by source.
    pub values: Vec<(String, String)>,
}

/// Every field where two sources give different values for the same
/// outlet. A source giving two values itself is not a conflict here.
pub fn conflicts(ledger: &Ledger) -> Vec<Conflict> {
    let mut out = Vec::new();
    for (outlet, claims) in &ledger.claims {
        let mut by_field: BTreeMap<&str, BTreeMap<&str, BTreeSet<&str>>> = BTreeMap::new();
        for c in claims {
            by_field
                .entry(&c.field)
                .or_default()
                .entry(&c.source_id)
                .or_default()
                .insert(&c.value);
        }
        for (field, by_source) in by_field {
            if by_source.len() < 2 {
                continue;
            }
            let distinct: BTreeSet<&&str> = by_source.values().flatten().collect();
            if distinct.len() < 2 {
                continue;
            }
            out.push(Conflict {
                outlet_id: outlet.clone(),
                field: field.to_string(),
                values: by_source
                    .iter()
                    .map(|(s, vs)| {
                        (
                            s.to_string(),
                            vs.iter().copied().collect::<Vec<_>>().join(" | "),
                        )
                    })
                    .collect(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(outlet: &str, field: &str, source: &str, value: &str) -> Claim {
        Claim {
            outlet_id: outlet.into(),
            field: field.into(),
            value: value.into(),
            data: None,
            source_id: source.into(),
            ref_url: "https://example.test/x".into(),
            retrieved_at: 1_700_000_000,
            as_of: None,
            note: None,
        }
    }

    fn ledger_with(claims: Vec<Claim>) -> Ledger {
        let mut l = Ledger::default();
        for c in claims {
            l.claims.entry(c.outlet_id.clone()).or_default().push(c);
        }
        l
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kg-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn summary_counts_kept_changed_and_added() {
        let l = ledger_with(vec![
            claim("a", "country", "wikidata", "UK"),
            claim("a", "owner", "wikidata", "X"),
            claim("a", "bias", "allsides", "Center"), // other source, ignored
            claim("b", "country", "wikidata", "US"),
            claim("c", "country", "wikidata", "FR"), // not in the run, ignored
        ]);
        let batch = vec![
            (
                "a".to_string(),
                vec![
                    claim("a", "country", "wikidata", "UK"),
                    claim("a", "owner", "wikidata", "Y"),
                ],
            ),
            ("b".to_string(), vec![]),
        ];
        let s = summarise(&l, "wikidata", &batch);
        assert_eq!(
            (s.outlets, s.existing, s.kept, s.removed_or_changed, s.added),
            (2, 3, 1, 2, 1)
        );
        assert!(!s.held(), "below the minimum, never held");
    }

    #[test]
    fn guard_holds_a_large_change_and_force_writes_it() {
        let d = tmp();
        let mut existing = Vec::new();
        for i in 0..9 {
            existing.push(claim(&format!("o{i}"), "country", "wikidata", "UK"));
        }
        let l = ledger_with(existing);
        // Four of nine change: 44%, over a third.
        let batch: Vec<(String, Vec<Claim>)> = (0..9)
            .map(|i| {
                let v = if i < 4 { "??" } else { "UK" };
                (
                    format!("o{i}"),
                    vec![claim(&format!("o{i}"), "country", "wikidata", v)],
                )
            })
            .collect();
        let c = commit_source(&d, &l, "wikidata", batch.clone(), false, 1_700_000_100).unwrap();
        let Commit::Held { path, summary } = c else {
            panic!("expected held, got {c:?}");
        };
        assert_eq!((summary.existing, summary.removed_or_changed), (9, 4));
        assert!(path.ends_with("review/wikidata-1700000100.toml"));
        assert!(!store::outlet_file(&d, "o0").exists(), "nothing written");
        let pending = pending_reviews(&d);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.summary.source_id, "wikidata");
        assert_eq!(pending[0].1.at, 1_700_000_100);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Held by the change guard"));
        assert!(text.contains("--write --force"));

        let c = commit_source(&d, &l, "wikidata", batch, true, 1_700_000_200).unwrap();
        assert!(matches!(c, Commit::Written { outlets: 9, .. }));
        assert!(store::outlet_file(&d, "o0").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn small_change_writes_straight_through() {
        let d = tmp();
        let l = ledger_with(vec![
            claim("a", "country", "wikidata", "UK"),
            claim("a", "bias", "allsides", "Center"),
        ]);
        let batch = vec![(
            "a".to_string(),
            vec![claim("a", "country", "wikidata", "United Kingdom")],
        )];
        let c = commit_source(&d, &l, "wikidata", batch, false, 1).unwrap();
        assert!(matches!(c, Commit::Written { outlets: 1, .. }));
        let written = store::load(&d).unwrap_or_default();
        let a = &written.claims["a"];
        assert!(
            a.iter().any(|c| c.source_id == "allsides"),
            "other sources kept"
        );
        assert!(a.iter().any(|c| c.value == "United Kingdom"));
        assert!(pending_reviews(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn conflicts_are_fields_where_sources_differ() {
        let l = ledger_with(vec![
            claim("a", "bias", "allsides", "Center"),
            claim("a", "bias", "mbfc", "Left-Center"),
            claim("a", "country", "wikidata", "UK"),
            claim("a", "country", "rdap", "UK"), // agree: no conflict
            claim("b", "owner", "wikidata", "X"),
            claim("b", "owner", "wikidata", "Y"), // one source, two values: not a conflict
        ]);
        let c = conflicts(&l);
        assert_eq!(c.len(), 1);
        assert_eq!(
            (c[0].outlet_id.as_str(), c[0].field.as_str()),
            ("a", "bias")
        );
        assert_eq!(
            c[0].values,
            vec![
                ("allsides".to_string(), "Center".to_string()),
                ("mbfc".to_string(), "Left-Center".to_string())
            ]
        );
    }
}
