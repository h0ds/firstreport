//! `/story/<absorbed id>/` after a merge (plan §5): the old address
//! keeps working and points at the survivor, following a chain of
//! merges to its end. A stub carries a canonical link and a refresh
//! for browsers, the survivor's id in its twins for machines, and no
//! story content of its own.

use std::collections::HashMap;

use serde_json::json;

use super::Twins;
use crate::input::Input;
use crate::render::{datetime, esc, html, Page};

/// Absorbed cluster id → final survivor, from the event log. A survivor
/// that was itself absorbed later is followed to the end; a cycle or a
/// chain longer than the log is cut off where it stands.
pub fn merges(input: &Input) -> HashMap<u64, u64> {
    let direct: HashMap<u64, u64> = input
        .events
        .iter()
        .filter(|e| e.kind == "merged")
        .filter_map(|e| Some((e.cluster_id?, e.merged_into?)))
        .collect();
    let mut out = HashMap::with_capacity(direct.len());
    for (&from, &to) in &direct {
        let mut end = to;
        let mut hops = 0;
        while let Some(&next) = direct.get(&end) {
            if next == from || hops > direct.len() {
                break;
            }
            end = next;
            hops += 1;
        }
        out.insert(from, end);
    }
    out
}

pub fn stub(input: &Input, absorbed: u64, survivor: u64) -> Twins {
    let target = format!("/story/{survivor}/");
    let title = input
        .clusters
        .get(&survivor)
        .map(|c| c.primary_title.clone())
        .unwrap_or_else(|| format!("Story {survivor}"));
    let head = format!(
        "<link rel=\"canonical\" href=\"{t}\">\n<meta http-equiv=\"refresh\" content=\"0; url={t}\">\n",
        t = esc(&target)
    );
    let body = format!(
        "<h1>Merged</h1><p>Story {absorbed} was merged into <a href=\"{}\">{}</a>. This address now points there.</p>",
        esc(&target),
        esc(&title)
    );
    let page_html = html(&Page {
        title: &format!("Story {absorbed}, merged"),
        description: &format!("Story {absorbed} was merged into story {survivor}."),
        path: &format!("/story/{absorbed}/"),
        wide: false,
        head: &head,
        body: &body,
        status: &format!(
            "Updated {}",
            datetime(input.export_time)
        ),
    });
    Twins {
        html: page_html,
        json: json!({"id": absorbed, "merged_into": survivor, "url": target, "title": title}),
        md: format!("# Story {absorbed}, merged\n\nMerged into [{title}]({target}).\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn chains_are_followed_to_the_survivor() {
        let i = fixture_input();
        let m = merges(&i);
        assert_eq!(m[&900], 1001, "900 → 901 → 1001");
        assert_eq!(m[&901], 1001);
        assert_eq!(m.len(), 2);
        let t = stub(&i, 900, 1001);
        assert!(t
            .html
            .contains("<link rel=\"canonical\" href=\"/story/1001/\">"));
        assert!(t
            .html
            .contains("http-equiv=\"refresh\" content=\"0; url=/story/1001/\""));
        assert!(t
            .html
            .contains("merged into <a href=\"/story/1001/\">Story 1001</a>"));
        assert_eq!(t.json["merged_into"], 1001);
        assert!(t.md.contains("Merged into [Story 1001](/story/1001/)"));
    }
}
