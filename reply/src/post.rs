//! Posting an approved or earned draft and logging it: one path for the
//! reviewer's click and for autonomous posting, so limits and the log
//! entry are the same whoever decided.

use crate::api::{over_limit, Limits};
use crate::model::{Draft, Reply};
use crate::platform::Poster;
use crate::queue::{Entry, Queue};

pub enum Refused {
    OverLimit(String),
    Platform(String),
    Store(String),
}

/// Post `draft` for the pending entry `id` and move it to the log.
/// `reviewer` is `human` or `auto`.
#[allow(clippy::too_many_arguments)]
pub async fn post_and_log(
    q: &mut Queue,
    poster: &mut dyn Poster,
    id: &str,
    entry: &Entry,
    draft: Draft,
    limits: Limits,
    reviewer: &str,
    now: i64,
) -> Result<Entry, Refused> {
    let log = q.log().map_err(Refused::Store)?;
    if let Some(why) = over_limit(&log, &entry.facts.mention.author, limits, now) {
        return Err(Refused::OverLimit(why));
    }
    let (post_ids, failure) = match poster.post(&entry.facts.mention, &draft.posts).await {
        Ok(ids) => (ids, None),
        Err((ids, e)) => (ids, Some(e)),
    };
    if post_ids.is_empty() {
        return Err(Refused::Platform(format!(
            "{}: {}",
            poster.name(),
            failure.unwrap_or_default()
        )));
    }
    let edited = draft.sentences.len() != entry.draft.sentences.len();
    let reply = Reply {
        post_ids,
        posted_at: now,
        reviewer: reviewer.into(),
        edited,
    };
    let mut e = q
        .decide(id, "posted", draft, Some(reply), now)
        .map_err(Refused::Store)?;
    if let Some(f) = failure {
        e.corrections.push(format!("thread incomplete: {f}"));
    }
    Ok(e)
}

/// Whether a draft may post without a person (plan §9): its exact set of
/// templates has been approved unchanged by a person at least
/// `autonomous_after` times, it carries no observation, and
/// `autonomous_after` is on. Observations are always reviewed, because
/// those are the sentences that get quoted.
pub fn earned(
    draft: &Draft,
    has_observations: bool,
    log: &[Entry],
    autonomous_after: usize,
) -> bool {
    if autonomous_after == 0 || has_observations {
        return false;
    }
    let approvals = log
        .iter()
        .filter(|e| e.status == "posted")
        .filter(|e| {
            e.reply
                .as_ref()
                .is_some_and(|r| r.reviewer == "human" && !r.edited)
        })
        .filter(|e| e.draft.template_ids == draft.template_ids)
        .count();
    approvals >= autonomous_after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomy_is_earned_per_template_set_and_never_with_observations() {
        let mut d = crate::queue::tests::draft("m");
        d.template_ids = vec!["origin".into(), "trailer".into()];
        let mut e = Entry {
            id: "x".into(),
            facts: crate::queue::tests::facts("m"),
            draft: d.clone(),
            status: "posted".into(),
            queued_at: 0,
            decided_at: Some(1),
            reply: Some(Reply {
                post_ids: vec![],
                posted_at: 1,
                reviewer: "human".into(),
                edited: false,
            }),
            corrections: vec![],
        };
        let log = vec![e.clone(), e.clone()];
        assert!(earned(&d, false, &log, 2));
        assert!(!earned(&d, false, &log, 3));
        assert!(!earned(&d, true, &log, 2));
        assert!(!earned(&d, false, &log, 0));
        e.reply.as_mut().unwrap().edited = true;
        assert!(!earned(&d, false, &[e.clone(), e.clone()], 2));
        e.reply.as_mut().unwrap().edited = false;
        e.draft.template_ids.push("spread".into());
        assert!(!earned(&d, false, &[e.clone(), e], 2));
    }
}
