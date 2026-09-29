# reply

Kagami pillar 4: a social media account people mention to ask where a
story came from. Answers with sourced provenance and propagation from
`feed` and `ledger`, never a verdict. The plan is in `docs/plan.md`.

```
cargo run -- check                                   # validate templates, render every fixture, compare goldens
cargo run -- check --update                          # rewrite the goldens; review the diff
cargo run -- render fixtures/mentions/origin_story.json --platform bluesky
cargo test
```

Runs from the `reply/` directory, where `templates/` and `fixtures/` are.
No network. `--feed-url`, `--ledger-url` and `--log-url` set where cites
resolve to; the defaults are the three local servers.

## How a reply is made

A `FactSet` (`fixtures/mentions/*.json`) is what the resolver will hand
the composer: the mention, the subject it was about, `feed`'s
`/v1/lookup` response verbatim, `ledger` claims for the outlet, display
names, observations, and the gaps. The composer turns it into a `Draft`
by walking the layout's template order and rendering each template
whose `needs` resolve and whose `when` holds. Every sentence carries a
cite; a draft with an uncited sentence fails `check` and is never
posted. Nothing in a draft comes from anywhere but a template.

## Templates

`templates/en/*.toml`, one `[id]` table per sentence:

```toml
[origin]
needs = ["d.origin_outlet", "d.origin_time", "lookup.cluster.id"]
when  = "lookup.matched == item"
text  = "First published by {d.origin_outlet} at {d.origin_time}."
cite  = "feed:item:{lookup.cluster.origin_item_id}"
```

Placeholders and `needs` are paths into the fact set as JSON. `d.` holds
derived values in the words a sentence needs: `d.origin_time` is a UTC
date, `d.t5` a duration, `d.syn_share_pct` a percentage, `d.ledger.<field>`
the first ledger claim for that field with its `value` and `source`,
`d.obs.<kind>` the first observation of that kind, `d.checked` and
`d.gaps` the misses in words. `when` is one comparison: `path op literal`.

Cites resolve as `feed:item:ID`, `feed:cluster:ID`, `feed:outlet:ID`,
`ledger:OUTLET:FIELD`, `reply:lookup:MENTION` and
`observation:KIND:SUBJECT`.

`templates/layout/<platform>.toml` gives the sentence order, the post
length limit, and the closing sentence that links the full record.

## Changing what the bot says

Edit a template or add one, run `cargo run -- check --update`, and
commit the fixture diffs with it. The diff is the review: it shows every
reply that changes. A new situation gets a new fixture first.
