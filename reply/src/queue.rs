//! The review queue and the public log (plan §3, §9, §10): one JSON file
//! per mention under `data/queue/` while pending, moved to `data/log/`
//! with its decision. `data/seen.txt` holds every mention id handled, so
//! a restart never drafts twice.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Draft, FactSet, Reply};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// Queue id: the mention id hashed, safe for URLs and file names.
    #[serde(default)]
    pub id: String,
    pub facts: FactSet,
    pub draft: Draft,
    /// `pending` in the queue; `posted` or `declined` in the log.
    pub status: String,
    pub queued_at: i64,
    #[serde(default)]
    pub decided_at: Option<i64>,
    #[serde(default)]
    pub reply: Option<Reply>,
    /// Later notes: a fact revised upstream, a follow-up posted. Never a deletion.
    #[serde(default)]
    pub corrections: Vec<String>,
}

/// The queue id of a mention: FNV-1a of the id, 16 hex chars, so
/// platform URIs never touch the filesystem or a URL path.
pub fn queue_id(mention_id: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in mention_id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub struct Queue {
    dir: PathBuf,
    log_dir: PathBuf,
    seen_path: PathBuf,
    seen: HashSet<String>,
}

impl Queue {
    pub fn open(data: &Path) -> Result<Self, String> {
        let dir = data.join("queue");
        let log_dir = data.join("log");
        for d in [&dir, &log_dir] {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let seen_path = data.join("seen.txt");
        let seen = std::fs::read_to_string(&seen_path)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(Self {
            dir,
            log_dir,
            seen_path,
            seen,
        })
    }

    pub fn seen(&self, mention_id: &str) -> bool {
        self.seen.contains(mention_id)
    }

    fn write(path: &Path, entry: &Entry) -> Result<(), String> {
        let body = serde_json::to_string_pretty(entry).map_err(|e| e.to_string())?;
        std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn push(&mut self, facts: FactSet, draft: Draft, now: i64) -> Result<PathBuf, String> {
        let mention_id = facts.mention.id.clone();
        let id = queue_id(&mention_id);
        let path = self.dir.join(format!("{id}.json"));
        let entry = Entry {
            id,
            facts,
            draft,
            status: "pending".into(),
            queued_at: now,
            decided_at: None,
            reply: None,
            corrections: vec![],
        };
        Self::write(&path, &entry)?;
        self.mark_seen(&mention_id)?;
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

    /// One pending entry by queue id.
    pub fn get(&self, id: &str) -> Result<Option<Entry>, String> {
        let path = self.dir.join(format!("{}.json", safe(id)));
        if !path.exists() {
            return Ok(None);
        }
        crate::model::load_json::<Entry>(&path).map(Some)
    }

    /// Move a pending entry to the log with its decision. `draft` is the
    /// draft as posted (possibly with sentences dropped), `reply` the
    /// platform's record of it, absent for a decline.
    pub fn decide(
        &mut self,
        id: &str,
        status: &str,
        draft: Draft,
        reply: Option<Reply>,
        now: i64,
    ) -> Result<Entry, String> {
        let mut e = self
            .get(id)?
            .ok_or_else(|| format!("no pending entry {id}"))?;
        e.status = status.into();
        e.draft = draft;
        e.reply = reply;
        e.decided_at = Some(now);
        Self::write(&self.log_dir.join(format!("{}.json", e.id)), &e)?;
        let src = self.dir.join(format!("{}.json", e.id));
        std::fs::remove_file(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        Ok(e)
    }

    /// Every pending entry, oldest queued first.
    pub fn list(&self) -> Result<Vec<Entry>, String> {
        let mut out = Self::read_dir(&self.dir)?;
        out.sort_by_key(|e| (e.queued_at, e.facts.mention.id.clone()));
        Ok(out)
    }

    /// The public log, oldest decided first.
    pub fn log(&self) -> Result<Vec<Entry>, String> {
        let mut out = Self::read_dir(&self.log_dir)?;
        out.sort_by_key(|e| (e.decided_at.unwrap_or(0), e.id.clone()));
        Ok(out)
    }

    /// One log entry by mention id, or by queue id.
    pub fn log_entry(&self, key: &str) -> Result<Option<Entry>, String> {
        let id = if key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            key.to_string()
        } else {
            queue_id(key)
        };
        let path = self.log_dir.join(format!("{id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        crate::model::load_json::<Entry>(&path).map(Some)
    }

    fn read_dir(dir: &Path) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension().is_some_and(|x| x == "json") {
                out.push(crate::model::load_json::<Entry>(&p)?);
            }
        }
        Ok(out)
    }
}

fn safe(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(16)
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::model::{Mention, Names, Subject};

    pub fn facts(id: &str) -> FactSet {
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
                thread: None,
                lang: None,
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

    pub fn draft(id: &str) -> Draft {
        Draft {
            contract_version: 1,
            mention_id: id.into(),
            platform: "t".into(),
            template_ids: vec![],
            sentences: vec![],
            posts: vec![],
            max_chars: 300,
            lang: "en".into(),
            lang_fallback: false,
            cites: vec![],
            autonomous: false,
        }
    }

    #[test]
    fn push_list_decide_and_seen_survive_reopen() {
        let dir = std::env::temp_dir().join(format!("kg-reply-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut q = Queue::open(&dir).unwrap();
        assert!(!q.seen("at://a/1"));
        q.push(facts("at://a/2"), draft("at://a/2"), 20).unwrap();
        q.push(facts("at://a/1"), draft("at://a/1"), 10).unwrap();
        q.mark_seen("at://a/3").unwrap();
        let mut q = Queue::open(&dir).unwrap();
        assert!(q.seen("at://a/1") && q.seen("at://a/3"));
        let ids: Vec<String> = q
            .list()
            .unwrap()
            .into_iter()
            .map(|e| e.facts.mention.id)
            .collect();
        assert_eq!(ids, vec!["at://a/1", "at://a/2"]);

        let qid = queue_id("at://a/1");
        assert!(q.get(&qid).unwrap().is_some());
        let e = q
            .decide(&qid, "declined", draft("at://a/1"), None, 30)
            .unwrap();
        assert_eq!((e.status.as_str(), e.decided_at), ("declined", Some(30)));
        assert!(q.get(&qid).unwrap().is_none());
        assert_eq!(q.list().unwrap().len(), 1);
        assert_eq!(q.log().unwrap().len(), 1);
        assert_eq!(q.log_entry("at://a/1").unwrap().unwrap().id, qid);
        assert_eq!(q.log_entry(&qid).unwrap().unwrap().status, "declined");
        assert!(q.decide(&qid, "posted", draft("x"), None, 1).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
