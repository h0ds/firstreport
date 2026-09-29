//! Mention → Subject → FactSet (plan §6). Asks `feed` once for the
//! subject and `ledger` once per outlet named; every miss becomes a gap
//! in words rather than a silent absence.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::client::{query, Api};
use crate::compose::Urls;
use crate::facts::{get, text};
use crate::model::{FactSet, Mention, Names, OutletFacts, Post, Subject};

/// Fewer words than this and a mention's text is a summons, not a claim
/// worth matching ("@kagami prove it").
const MIN_TEXT_WORDS: usize = 5;

/// What the mention is about. Links win over text; the mention's own
/// content wins over what it quotes, which wins over what it answers.
pub fn subject(m: &Mention) -> Subject {
    let own_urls = urls_of(&m.text, &m.urls);
    if !own_urls.is_empty() {
        return Subject {
            urls: own_urls,
            text: None,
            from: "mention".into(),
        };
    }
    for (post, from) in [(&m.quoted, "quote"), (&m.parent, "parent")] {
        if let Some(p) = post {
            let u = urls_of(&p.text, &p.urls);
            if !u.is_empty() {
                return Subject {
                    urls: u,
                    text: None,
                    from: from.into(),
                };
            }
        }
    }
    let own = strip_summons(&m.text);
    if words(&own) >= MIN_TEXT_WORDS {
        return Subject {
            urls: vec![],
            text: Some(own),
            from: "mention".into(),
        };
    }
    for (post, from) in [(&m.quoted, "quote"), (&m.parent, "parent")] {
        if let Some(Post { text: t, .. }) = post {
            let t = strip_summons(t);
            if words(&t) >= MIN_TEXT_WORDS {
                return Subject {
                    urls: vec![],
                    text: Some(t),
                    from: from.into(),
                };
            }
        }
    }
    Subject {
        urls: vec![],
        text: None,
        from: "mention".into(),
    }
}

/// The platform's link list plus any http(s) token in the text, deduped
/// in order of appearance.
fn urls_of(text: &str, given: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |u: &str| {
        let u = u.trim_end_matches(['.', ',', ')', '"', '\'', '>']);
        if (u.starts_with("http://") || u.starts_with("https://")) && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
        }
    };
    for g in given {
        push(g);
    }
    for tok in text.split_whitespace() {
        push(tok);
    }
    out
}

/// Drop @handles and the phrases people summon the bot with, keeping
/// whatever claim is left.
fn strip_summons(t: &str) -> String {
    let kept: Vec<&str> = t
        .split_whitespace()
        .filter(|w| !w.starts_with('@'))
        .filter(|w| !(w.starts_with("http://") || w.starts_with("https://")))
        .collect();
    let mut s = kept.join(" ");
    for phrase in SUMMONS_STRIP {
        let lower = s.to_ascii_lowercase();
        if let Some(i) = lower.find(phrase) {
            s.replace_range(i..i + phrase.len(), "");
        }
    }
    s.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '“' | '”' | ':' | ',' | '?' | '!')
    })
    .to_string()
}

/// Phrases that are a summons rather than a claim; stripped before matching.
const SUMMONS_STRIP: &[&str] = &[
    "prove it",
    "is this real",
    "is this true",
    "source",
    "thoughts",
];
/// Words that mark a short summons as Japanese: true?, source, fake, evidence, lie, citation.
const SUMMONS_JA: &[&str] = &["本当", "ソース", "デマ", "証拠", "嘘", "出典"];
/// Words that mark a short summons as English.
const SUMMONS: &[&str] = &[
    "prove it",
    "is this real",
    "is this true",
    "source",
    "thoughts",
    "real",
    "true",
    "fake",
];

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// The language the bot was summoned in: the mention's own words first;
/// when those are too short to tell ("@kagami prove it" is under the
/// detector's floor) but are a known summons, English; else the quoted or
/// parent post; else unknown, which the composer treats as English.
pub fn language(m: &Mention) -> Option<String> {
    let own: String = m
        .text
        .split_whitespace()
        .filter(|w| !w.starts_with('@') && !w.starts_with("http"))
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(l) = crate::lang::detect(&own) {
        return Some(l);
    }
    let lower = own.to_ascii_lowercase();
    if words(&own) > 0 && SUMMONS.iter().any(|p| lower.contains(p)) {
        return Some("en".into());
    }
    if SUMMONS_JA.iter().any(|p| own.contains(p)) {
        return Some("ja".into());
    }
    for post in [&m.quoted, &m.parent].into_iter().flatten() {
        if let Some(l) = crate::lang::detect(&strip_summons(&post.text)) {
            return Some(l);
        }
    }
    None
}

pub async fn resolve<A: Api>(api: &A, urls: &Urls, m: &Mention) -> Result<FactSet, String> {
    let subject = subject(m);
    let mut m = m.clone();
    if m.lang.is_none() {
        m.lang = language(&m);
    }
    let mut fs = FactSet {
        mention: m.clone(),
        subject: subject.clone(),
        lookup: None,
        names: Names::default(),
        outlets: vec![],
        observations: vec![],
        gaps: vec![],
    };
    let feed = urls.feed.trim_end_matches('/');
    let ledger = urls.ledger.trim_end_matches('/');

    let q = match (subject.urls.first(), &subject.text) {
        (Some(u), _) => format!("url={}", query(u)),
        (None, Some(t)) => format!("text={}", query(t)),
        (None, None) => {
            fs.gaps
                .push("no link, quoted text or parent post to look up".into());
            return Ok(fs);
        }
    };
    if subject.urls.len() > 1 {
        fs.gaps.push(format!(
            "{} links in the post; only the first was checked",
            subject.urls.len()
        ));
    }
    // feed is the answer; without it there is no reply.
    let lookup = api
        .get(&format!("{feed}/v1/lookup?{q}"))
        .await?
        .ok_or_else(|| "feed answered 404 to /v1/lookup".to_string())?;

    // Names for every outlet id the lookup mentions.
    match api.get(&format!("{feed}/v1/outlets")).await {
        Ok(Some(Value::Array(list))) => {
            for o in list {
                if let (Some(id), Some(n)) = (get(&o, "id"), get(&o, "name")) {
                    fs.names.outlets.insert(text(id), text(n));
                }
            }
        }
        Ok(_) => {}
        Err(e) => fs.gaps.push(format!("outlet names unavailable: {e}")),
    }

    // Ledger: the subject's outlet, if the lookup named one.
    if let Some(id) = get(&lookup, "outlet.id").map(text) {
        match api
            .get(&format!("{ledger}/v1/outlets/{}", query(&id)))
            .await
        {
            Ok(Some(view)) => fs.outlets.push(OutletFacts {
                outlet_id: id,
                claims: flatten_claims(&view),
            }),
            Ok(None) => fs.gaps.push(format!("ledger has no record for {id}")),
            Err(e) => fs.gaps.push(format!("ledger unavailable: {e}")),
        }
        if !fs.outlets.is_empty() {
            match api.get(&format!("{ledger}/v1/sources")).await {
                Ok(Some(Value::Array(list))) => {
                    for s in list {
                        if let (Some(id), Some(n)) = (get(&s, "id"), get(&s, "name")) {
                            fs.names.sources.insert(text(id), text(n));
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => fs.gaps.push(format!("source names unavailable: {e}")),
            }
        }
    }
    fs.lookup = Some(lookup);
    Ok(fs)
}

/// Ledger serves `{ fields: { field: [claim...] } }`; the composer wants
/// a flat list, in field order then the server's order within a field.
fn flatten_claims(view: &Value) -> Vec<Value> {
    let Some(fields) = get(view, "fields").and_then(Value::as_object) else {
        return vec![];
    };
    let ordered: BTreeMap<&String, &Value> = fields.iter().collect();
    ordered
        .values()
        .filter_map(|v| v.as_array())
        .flat_map(|a| a.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Recorded;
    use serde_json::json;

    fn mention(text: &str, urls: &[&str], parent: Option<Post>) -> Mention {
        Mention {
            id: "m1".into(),
            platform: "t".into(),
            author: "a".into(),
            at: 0,
            text: text.into(),
            urls: urls.iter().map(|s| s.to_string()).collect(),
            parent,
            quoted: None,
            thread: None,
            lang: None,
        }
    }

    #[test]
    fn subject_prefers_links_then_own_text_then_parent() {
        let p = Post {
            id: "p".into(),
            text: "see https://x.example/a.".into(),
            urls: vec![],
        };
        let s = subject(&mention("@kagami prove it", &[], Some(p.clone())));
        assert_eq!(
            (s.urls.as_slice(), s.from.as_str()),
            (&["https://x.example/a".to_string()][..], "parent")
        );

        let s = subject(&mention(
            "@kagami https://y.example/b",
            &[],
            Some(p.clone()),
        ));
        assert_eq!(s.from, "mention");
        assert_eq!(s.urls, vec!["https://y.example/b"]);

        let s = subject(&mention(
            "@kagami \"Central bank holds rates, signals cuts\" source??",
            &[],
            None,
        ));
        assert_eq!(
            s.text.as_deref(),
            Some("Central bank holds rates, signals cuts")
        );

        let long = Post {
            id: "p".into(),
            text: "The bank held rates and hinted at cuts".into(),
            urls: vec![],
        };
        let s = subject(&mention("@kagami is this real", &[], Some(long)));
        assert_eq!((s.from.as_str(), s.text.is_some()), ("parent", true));

        let s = subject(&mention("@kagami thoughts?", &[], None));
        assert!(s.urls.is_empty() && s.text.is_none());
    }

    #[test]
    fn language_of_the_summons() {
        let p = |t: &str| {
            Some(Post {
                id: "p".into(),
                text: t.into(),
                urls: vec![],
            })
        };
        let ja = "この記事は完全に間違っている。出典をよく見てほしい、どこから出た話なのか";
        assert_eq!(
            language(&mention("@kagami prove it", &[], p(ja))).as_deref(),
            Some("en")
        );
        assert_eq!(
            language(&mention("@kagami これは本当？", &[], None)).as_deref(),
            Some("ja")
        );
        assert_eq!(
            language(&mention(
                "@kagami この話はどこから出てきたのか、本当のことなのか教えてほしい",
                &[],
                None
            ))
            .as_deref(),
            Some("ja")
        );
        assert_eq!(
            language(&mention("@kagami ?", &[], p(ja))).as_deref(),
            Some("ja")
        );
        assert_eq!(language(&mention("@kagami ?", &[], None)), None);
    }

    #[tokio::test]
    async fn resolves_with_gaps_for_what_is_missing() {
        let urls = Urls::default();
        let mut api = Recorded::default();
        api.responses.insert(
            "http://127.0.0.1:8080/v1/lookup?url=https%3A%2F%2Fx.example%2Fa".into(),
            json!({"matched": "outlet", "host": "x.example", "outlet": {"id": "x", "name": "X"}, "checked": []}),
        );
        api.responses.insert(
            "http://127.0.0.1:8080/v1/outlets".into(),
            json!([{"id": "x", "name": "X News"}]),
        );
        api.responses.insert(
            "http://127.0.0.1:8081/v1/outlets/x".into(),
            json!({"id": "x", "fields": {"owner": [{"field": "owner", "value": "O", "source_id": "wikidata"}],
                                          "country": [{"field": "country", "value": "C", "source_id": "wikidata"}]}}),
        );
        api.responses.insert(
            "http://127.0.0.1:8081/v1/sources".into(),
            json!([{"id": "wikidata", "name": "Wikidata"}]),
        );
        let m = mention(
            "@kagami https://x.example/a and https://x.example/b",
            &[],
            None,
        );
        let fs = resolve(&api, &urls, &m).await.unwrap();
        assert_eq!(fs.subject.urls.len(), 2);
        assert_eq!(
            get(fs.lookup.as_ref().unwrap(), "matched")
                .map(text)
                .as_deref(),
            Some("outlet")
        );
        assert_eq!(fs.names.outlets["x"], "X News");
        assert_eq!(fs.names.sources["wikidata"], "Wikidata");
        let fields: Vec<String> = fs.outlets[0]
            .claims
            .iter()
            .map(|c| text(&c["field"]))
            .collect();
        assert_eq!(fields, vec!["country", "owner"]);
        assert_eq!(
            fs.gaps,
            vec!["2 links in the post; only the first was checked"]
        );

        // Ledger down: still a fact set, with the gap said.
        api.down.push("http://127.0.0.1:8081".into());
        let fs = resolve(
            &api,
            &urls,
            &mention("@kagami https://x.example/a", &[], None),
        )
        .await
        .unwrap();
        assert!(fs.outlets.is_empty());
        assert!(
            fs.gaps[0].starts_with("ledger unavailable"),
            "{:?}",
            fs.gaps
        );

        // Feed down: no reply at all.
        api.down.push("http://127.0.0.1:8080".into());
        assert!(resolve(&api, &urls, &m).await.is_err());

        // Nothing to look up.
        let fs = resolve(&api, &urls, &mention("@kagami thoughts?", &[], None))
            .await
            .unwrap();
        assert!(fs.lookup.is_none());
        assert_eq!(fs.gaps.len(), 1);
    }
}
