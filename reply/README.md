# reply

Kagami pillar 4: a social media account people mention to ask where a
story came from. Answers with sourced provenance and propagation from
`feed` and `ledger`, never a verdict. The plan is in `docs/plan.md`.

```
cargo run -- check                                   # validate templates, render every fixture, compare goldens
cargo run -- check --update                          # rewrite the goldens; review the diff
cargo run -- render fixtures/mentions/origin_story.json --platform bluesky
cargo run -- resolve mention.json                    # ask a running feed and ledger; print the FactSet
cargo run -- resolve mention.json --render           # ...and the draft it renders to
REPLY_X_ACCESS_TOKEN=... cargo run -- listen         # dry run on X: mentions in, drafts to data/queue/
REPLY_BSKY_PASSWORD=... cargo run -- --platform bluesky --handle kagami.example listen   # same, on Bluesky
cargo run -- queue                                   # print the queued drafts
cargo run -- serve                                   # review page and public log on 127.0.0.1:8082
REPLY_X_ACCESS_TOKEN=... cargo run -- serve          # ...with posting to X on approval
cargo test
```

Runs from the `reply/` directory, where `templates/` and `fixtures/` are.
`render` and `check` need no network. `resolve` asks `feed` and `ledger`
at `--feed-url` and `--ledger-url` (default: the local servers) and
`--log-url` is where the reply log will live; cites resolve against all
three. A `resolve` output saved under `fixtures/mentions/` is a fixture.

## Platforms

X is the primary platform and the default for `--platform`; Bluesky is
the live testbed, since it needs no paid tier or automation approval.
Both implement the same two traits, so every other part of `reply` is
platform-neutral.

- **X** needs `REPLY_X_ACCESS_TOKEN`, an OAuth 2.0 user token for the
  bot's account with `tweet.read tweet.write users.read offline.access`.
  For refresh, also `REPLY_X_REFRESH_TOKEN` and `REPLY_X_CLIENT_ID` (and
  `REPLY_X_CLIENT_SECRET` for a confidential client). X rotates the
  refresh token on each use, so the current pair is kept in
  `data/x_token.json` after the first refresh and read from there. The
  mentions endpoint needs a paid API tier.
- **Bluesky** needs `--handle` and `REPLY_BSKY_PASSWORD`, an app
  password. `--api` overrides the PDS, default `https://bsky.social`.

## Listening (dry run)

`listen` polls the platform for mentions of the bot every `--interval`
seconds (default 30), with the parent and quoted posts, resolves and
drafts each mention, and writes the draft with its FactSet to
`data/queue/<hash>.json` as `pending`. `data/seen.txt` holds every
mention id handled, so a restart drafts nothing twice. Nothing is
posted. `--once` polls a single time, for cron.

On X, links are taken expanded past t.co, and a quoted tweet's own
status link is not a subject. On Bluesky only `reason == "mention"`
notifications count. Replies to the bot's own posts and quotes of them
are not summonses on either.

## Reviewing and posting

`serve` shows every pending draft on `http://127.0.0.1:8082/`: the
mention, the subject, each sentence with its cite and the template it
came from, and the gaps. The reviewer unticks sentences to leave them
out (the closing line always stays, and cites follow their sentences so
rule 6 holds), then posts or declines. Posting replies under the
mention, as a thread when the draft is more than one post, with link
facets so URLs are clickable. Both decisions move the entry from
`data/queue/` to `data/log/` with the decision, the draft as posted and
the platform post ids.

Without credentials for `--platform`, `serve` still reviews and
declines, but approving answers 409: nothing can post. Limits are
`--per-account-per-day` (default 3) and `--global-per-hour` (default
60), counted over the log; a post past either answers 429.

The public log is `GET /v1/log` and `GET /v1/log/{mention id or queue
id}`, which is where every reply's closing link points. The summoner's
handle is dropped from it. The queue routes are for the local reviewer
and not part of the contract. `listen` and `serve` share `data/`
through the filesystem and run as two processes.

## Resolving a mention

A mention file is `{ id, platform, author, at, text, urls, parent?, quoted? }`
where `parent` and `quoted` are `{ id, text, urls }`. The subject is chosen
in this order: links in the mention itself, links in the quoted post, links
in the parent, then the mention's own text once the summons is stripped
and enough words remain, then the quoted or parent text. Only the first
link is looked up, and the gap says so. `feed` unreachable means no reply;
`ledger` unreachable means a reply with that gap in it.

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

Read `docs/voice.md` first: it is the tone guide every template is
held to. Then edit a template or add one, run `cargo run -- check --update`, and
commit the fixture diffs with it. The diff is the review: it shows every
reply that changes. A new situation gets a new fixture first.
