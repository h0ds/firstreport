//! Snapshot-to-snapshot diff (plan §5.3a). Given the entries a feed showed
//! last time and the entries it shows now, report items whose text changed
//! and items that vanished without simply aging out of the feed window.
//! Pure: the poller supplies both sets and persists the result.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub id: u64,
    pub title: String,
    pub summary: String,
    pub published_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Title or summary differs from the previous snapshot.
    Revised {
        id: u64,
        title: String,
        summary: String,
    },
    /// Present before, absent now, and newer than the oldest item still
    /// listed, so it did not fall off the end of the feed.
    Withdrawn { id: u64 },
}

pub fn diff(prev: &[Seen], cur: &[Seen]) -> Vec<Change> {
    if prev.is_empty() || cur.is_empty() {
        // Nothing to compare against, or the feed came back empty (which
        // is a feed problem, not a hundred withdrawals).
        return vec![];
    }
    let now: HashMap<u64, &Seen> = cur.iter().map(|s| (s.id, s)).collect();
    let oldest_present = cur.iter().map(|s| s.published_at).min().unwrap_or(0);
    let mut out = Vec::new();
    for p in prev {
        match now.get(&p.id) {
            Some(c) => {
                if c.title != p.title || c.summary != p.summary {
                    out.push(Change::Revised {
                        id: p.id,
                        title: c.title.clone(),
                        summary: c.summary.clone(),
                    });
                }
            }
            None => {
                // Items with no date can't be placed relative to the window;
                // never call those withdrawn.
                if p.published_at > 0 && p.published_at > oldest_present {
                    out.push(Change::Withdrawn { id: p.id });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: u64, title: &str, published: i64) -> Seen {
        Seen {
            id,
            title: title.into(),
            summary: String::new(),
            published_at: published,
        }
    }

    #[test]
    fn detects_revision_and_withdrawal_but_not_aging_out() {
        let prev = vec![
            s(1, "old headline", 100),
            s(2, "steady", 200),
            s(3, "vanishes", 300),
            s(4, "undated", 0),
        ];
        let cur = vec![
            s(1, "new headline", 100),
            s(2, "steady", 200),
            s(5, "fresh", 400),
        ];
        // Oldest still present is 100 (item 1), so: item 3 (300) vanished
        // while newer items remain → withdrawn; item 4 is undated → ignored.
        let d = diff(&prev, &cur);
        assert_eq!(
            d,
            vec![
                Change::Revised {
                    id: 1,
                    title: "new headline".into(),
                    summary: String::new()
                },
                Change::Withdrawn { id: 3 },
            ]
        );
    }

    #[test]
    fn aged_out_items_are_not_withdrawn() {
        let prev = vec![s(1, "a", 100), s(2, "b", 200)];
        let cur = vec![s(2, "b", 200), s(3, "c", 300)];
        assert!(
            diff(&prev, &cur).is_empty(),
            "item 1 is older than everything still listed"
        );
    }

    #[test]
    fn empty_sides_produce_nothing() {
        assert!(diff(&[], &[s(1, "a", 1)]).is_empty());
        assert!(diff(&[s(1, "a", 1)], &[]).is_empty());
    }
}
