//! Contract types (plan §4). `feed` and `ledger` objects are carried as
//! JSON: their shapes are those crates' contracts, and `reply` reads
//! them by path rather than re-declaring them.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTRACT_VERSION: u32 = 1;

/// A post the mention points at: the one it replies to, or quotes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Post {
    pub id: String,
    #[serde(default)]
    pub text: String,
    /// Links the post carries, as the platform gives them.
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mention {
    pub id: String,
    pub platform: String,
    pub author: String,
    pub at: i64,
    pub text: String,
    /// Links in the mention's own text.
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub parent: Option<Post>,
    #[serde(default)]
    pub quoted: Option<Post>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subject {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Where the subject came from: `mention`, `parent` or `quote`.
    pub from: String,
}

/// Ledger's view of one outlet: `{ id, claims: [Claim] }` as served.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutletFacts {
    pub outlet_id: String,
    #[serde(default)]
    pub claims: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub kind: String,
    pub subject: String,
    pub window_secs: u32,
    pub count: u32,
    pub denominator: u32,
    #[serde(default)]
    pub detail: Value,
    pub computed_at: i64,
}

/// Display names the resolver collected: outlet ids from `feed`'s
/// registry, source ids from `ledger`'s source list. Ids stand in for
/// anything missing, so a sentence is never blocked on a name.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Names {
    #[serde(default)]
    pub outlets: BTreeMap<String, String>,
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
}

/// Everything the resolver found for one mention. `lookup` is `feed`'s
/// `/v1/lookup` response verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactSet {
    pub mention: Mention,
    pub subject: Subject,
    #[serde(default)]
    pub lookup: Option<Value>,
    #[serde(default)]
    pub names: Names,
    #[serde(default)]
    pub outlets: Vec<OutletFacts>,
    #[serde(default)]
    pub observations: Vec<Observation>,
    /// What was looked for and not found, in words.
    #[serde(default)]
    pub gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cite {
    /// Index into `Draft::sentences`.
    pub sentence: u16,
    /// `feed:item:ID`, `ledger:OUTLET:FIELD`, `reply:lookup:MENTION`, `observation:KIND:SUBJECT`.
    pub reference: String,
    pub url: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Draft {
    pub contract_version: u32,
    pub mention_id: String,
    pub platform: String,
    pub template_ids: Vec<String>,
    /// One entry per rendered sentence, in order.
    pub sentences: Vec<String>,
    /// The sentences joined, split into posts at sentence boundaries.
    pub posts: Vec<String>,
    pub cites: Vec<Cite>,
    pub autonomous: bool,
}

/// Rule 6, enforced: every sentence has exactly one cite with a URL.
pub fn check_cites(d: &Draft) -> Result<(), String> {
    if d.sentences.is_empty() {
        return Err(format!("{}: draft has no sentences", d.mention_id));
    }
    for (i, s) in d.sentences.iter().enumerate() {
        let n = d.cites.iter().filter(|c| c.sentence as usize == i).count();
        if n != 1 {
            return Err(format!(
                "{}: sentence {i} has {n} cites, needs exactly one: {s:?}",
                d.mention_id
            ));
        }
    }
    for c in &d.cites {
        if c.sentence as usize >= d.sentences.len() {
            return Err(format!(
                "{}: cite points past the last sentence",
                d.mention_id
            ));
        }
        if c.url.is_empty() || c.reference.is_empty() {
            return Err(format!(
                "{}: empty cite on sentence {}",
                d.mention_id, c.sentence
            ));
        }
    }
    Ok(())
}

pub fn load_fixture(path: &Path) -> Result<FactSet, String> {
    load_json(path)
}

pub fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(sentences: &[&str], cites: &[u16]) -> Draft {
        Draft {
            contract_version: 1,
            mention_id: "m".into(),
            platform: "t".into(),
            template_ids: vec![],
            sentences: sentences.iter().map(|s| s.to_string()).collect(),
            posts: vec![],
            cites: cites
                .iter()
                .map(|&i| Cite {
                    sentence: i,
                    reference: "r".into(),
                    url: "u".into(),
                    kind: "k".into(),
                })
                .collect(),
            autonomous: false,
        }
    }

    #[test]
    fn every_sentence_needs_one_cite() {
        assert!(check_cites(&draft(&["a", "b"], &[0, 1])).is_ok());
        assert!(check_cites(&draft(&["a", "b"], &[0])).is_err());
        assert!(check_cites(&draft(&["a"], &[0, 0])).is_err());
        assert!(check_cites(&draft(&[], &[])).is_err());
        let mut d = draft(&["a"], &[0]);
        d.cites[0].url.clear();
        assert!(check_cites(&d).is_err());
    }
}
