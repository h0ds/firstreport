//! The review queue (plan §3, §9): one JSON file per mention under
//! `data/queue/`, plus `data/seen.txt`, the mention ids already handled,
//! so a restart never drafts twice. Nothing here posts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Draft, FactSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub facts: FactSet,
    pub draft: Draft,
    /// `pending` until a reviewer acts (R3). Dry-run entries never leave it.
    pub status: String,
    pub queued_at: i64,
}

pub struct Queue {
    dir: PathBuf,
    seen_path: PathBuf,
    seen: HashSet<String>,
}

impl Queue {
    pub fn open(data: &Path) -> Result<Self, String> {
        let dir = data.join("queue");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let seen_path = data.join("seen.txt");
        let seen = std::fs::read_to_string(&seen_path)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(Self {
            dir,
            seen_path,
            seen,
        })
    }

    pub fn seen(&self, mention_id: &str) -> bool {
        self.seen.contains(mention_id)
    }

    /// File name for a mention id: the id's xxhash-free stand-in, a
    /// 16-hex FNV-1a, so platform URIs never touch the filesystem.
    fn file_for(&self, mention_id: &str) -> PathBuf {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in mention_id.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.dir.join(format!("{h:016x}.json"))
    }

    pub fn push(&mut self, facts: FactSet, draft: Draft, now: i64) -> Result<PathBuf, String> {
        let id = facts.mention.id.clone();
        let path = self.file_for(&id);
        let entry = Entry {
            facts,
            draft,
            status: "pending".into(),
            queued_at: now,
        };
        let body = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
        std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        self.mark_seen(&id)?;
        Ok(path)
    }

    /// Remember a mention without queueing it (resolve failed, say), so
    /// it is not retried forever.
    pub fn mark_seen(&mut self, mention_id: &str) -> Result<(), String> {
        if self.seen.insert(mention_id.to_string()) {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&self.seen_path)
                .map_err(|e| format!("{}: {e}", self.seen_path.display()))?;
            writeln!(f, "{mention_id}").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Every entry, oldest queued first.
    pub fn list(&self) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension().is_some_and(|x| x == "json") {
                out.push(crate::model::load_json::<Entry>(&p)?);
            }
        }
        out.sort_by_key(|e| (e.queued_at, e.facts.mention.id.clone()));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mention, Names, Subject};

    fn facts(id: &str) -> FactSet {
        FactSet {
            mention: Mention {
                id: id.into(),
                platform: "t".into(),
                author: "a".into(),
                at: 1,
                text: "x".into(),
                urls: vec![],
                parent: None,
                quoted: None,
            },
            subject: Subject {
                urls: vec![],
                text: None,
                from: "mention".into(),
            },
            lookup: None,
            names: Names::default(),
            outlets: vec![],
            observations: vec![],
            gaps: vec![],
        }
    }

    fn draft(id: &str) -> Draft {
        Draft {
            contract_version: 1,
            mention_id: id.into(),
            platform: "t".into(),
            template_ids: vec![],
            sentences: vec![],
            posts: vec![],
            cites: vec![],
            autonomous: false,
        }
    }

    #[test]
    fn push_list_and_seen_survive_reopen() {
        let dir = std::env::temp_dir().join(format!("kg-reply-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut q = Queue::open(&dir).unwrap();
        assert!(!q.seen("at://a/1"));
        q.push(facts("at://a/2"), draft("at://a/2"), 20).unwrap();
        q.push(facts("at://a/1"), draft("at://a/1"), 10).unwrap();
        q.mark_seen("at://a/3").unwrap();
        let q = Queue::open(&dir).unwrap();
        assert!(q.seen("at://a/1") && q.seen("at://a/3"));
        let ids: Vec<String> = q
            .list()
            .unwrap()
            .into_iter()
            .map(|e| e.facts.mention.id)
            .collect();
        assert_eq!(ids, vec!["at://a/1", "at://a/2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
