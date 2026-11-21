# reply — Social Answer Layer

Pillar 4. `reply` is First Report on social media: an account people mention
with "is this real?" or "prove it", which answers from `feed` and
`ledger` with where a story came from, how it spread, who carried it,
and what is known about the outlets, every sentence linked to its
source. It never says true or false. It says what happened and lets
the reader decide, and it says plainly when it cannot answer.

The project rules in `feed/docs/plan.md` §1 apply. Two are restated
for this layer, and a third is added.

> **Rule 6 for `reply`.** A reply is a list of sourced facts arranged
> into sentences. No fact appears in a reply that is not a `feed`
> record or a `ledger` claim with a URL. No sentence is a verdict on
> the story, the poster or an account. "This domain was registered 19
> days ago" is a reply. "This is fake" is not, and neither is "this
> account is a bot".

> **Rule 2 for `reply`.** Runs locally, one binary, same as the rest.
> Network access is to the social platforms and to `feed` and
> `ledger`. No hosted model, no cloud inference, no third-party
> fact-checking API in the answer path.

> **Rule 7, new: open by construction.** Every template the bot can
> say, every threshold that decides what it says, every signal it
> looks at, and every reply it has ever posted is in the open, in
> this repository or in a public log, so anyone can check how an
> answer was made and propose a better one by pull request. A bot
> that asks to be trusted has to be inspectable to the last byte.

## 1. Why this exists

`feed` and `ledger` answer the questions that matter in a
disinformation campaign, but they answer them to whoever runs the
binaries. The people who need the answer are on the platform where
the claim is spreading, at the moment it spreads. `reply` puts the
answer there.

People will mention the bot to win arguments. They will write "@firstreport
prove it" under a post they think is a lie, and under a post they
think is true. That is the use case, not a misuse. The goal is to be
worth summoning: to give both sides the same provenance, fast enough
to matter, in a form that is hard to argue with because every part of
it links somewhere. When the bot cannot prove anything, because the
story is not in `feed` or the claim is not a story at all, it says so
in as many words. An honest "no data" is part of being worth
summoning.

The threat model is coordinated campaigns: many accounts, one message,
short window, fresh domains, syndicated copies dressed as independent
confirmation. `feed` already measures syndication and spread at the
outlet level. `reply` measures the same shapes at the account level
and reports them as observations.

## 2. Scope

### In scope
- Listening for mentions of the bot's account on each supported platform
- Resolving a mention to what is being asked about: the post it replies
  to, the links in it, the quoted post, or the text itself
- Matching links and text to `feed` items and clusters
- Joining matched outlets to `ledger` claims
- Composing a reply from templates, from those facts only
- Posting the reply, after a human approves it until the templates earn autonomy
- Recording account-level propagation of a link or text: who posted it,
  when, in what order, how close together
- Coordination observations over that record, reported as counts and
  windows, never as labels
- A public log of every mention received and every reply posted, with
  the facts each reply was built from
- Platform adapters: X as the primary platform, Bluesky as the live testbed only

### Out of scope (deliberately)
- Judging whether a claim is true
- Labelling accounts as bots, trolls, state actors, or anything else
- Replying where not mentioned
- Direct messages
- Claim extraction from free text (a future pillar; until then unlinked
  posts are matched to stories by embedding and the match confidence is shown)
- Storing post bodies or account profiles beyond what a reply needs
- Any ranking, scoring or leaderboard of accounts or outlets

## 3. Architecture

Single Rust binary, single process, `tokio` runtime.

```
┌──────────┐ mentions ┌──────────┐ facts ┌──────────┐ draft ┌────────┐ post ┌──────────┐
│ Listener │ ───────▶ │ Resolver │ ────▶ │ Composer │ ────▶ │ Review │ ───▶ │  Poster  │
└──────────┘          └──────────┘       └──────────┘       └────────┘      └──────────┘
     │                     │  ▲                                                   │
     │ subject             │  │ /v1/lookup                                        │
     ▼                     ▼  │                                                   ▼
┌──────────┐          ┌────────┴───┐                                        ┌──────────┐
│ Observer │          │ feed+ledger│                                        │  Public  │
│ (search) │          └────────────┘                                        │   log    │
└──────────┘                                                                └──────────┘
```

- **Listener** — one adapter per platform; yields `Mention`s and, for the
  observer, every post it sees that carries a link or matches a tracked text hash.
- **Resolver** — turns a mention into a `Subject` (URLs, quoted text, the
  parent post) and asks `feed` and `ledger` for everything known about it.
  Returns a `FactSet`.
- **Composer** — picks templates by what the `FactSet` contains and renders
  a `Draft`. Pure function of the facts and the templates; same in, same out.
- **Review** — a queue on the local web page. A human posts, edits or
  declines. A template that has been approved unchanged N times may be
  marked autonomous in config; the log shows which replies were.
- **Poster** — posts the draft as a reply to the mention, records the
  platform's post id, appends to the public log.
- **Observer** — when a mention names a subject, searches the platform
  for recent posts carrying it, records them as `Sighting`s (hashed
  account, age, time, link or text hash), runs the coordination signals
  over the retained window, and hands the subject's `Observation`s to the
  composer. No firehose: the bot is summoned.

`feed` gains one endpoint for this, `GET /v1/lookup?url=|text=`, which
joins item, cluster, timeline, syndication and outlet in one call.
`ledger` already serves what `reply` needs.

## 4. Data contract (v1)

```
Mention {
  id:           string     // platform post id
  platform:     "x" | "bluesky"
  author:       string     // platform handle; never stored beyond the log
  at:           i64
  text:         string
  parent_id:    string?    // post being replied to
  quoted_id:    string?
  urls:         [string]   // canonicalized, from mention, parent, quote
}

Subject {
  urls:         [string]
  text:         string?    // text to match when there is no url
  from:         "mention" | "parent" | "quote"
}

FactSet {
  subject:      Subject
  matches:      [Match]    // Match { item_id, cluster_id, how: "url"|"text", confidence: f32 }
  cluster:      Cluster?   // feed contract, with timeline and syndication
  outlets:      [OutletFacts] // { outlet_id, claims: [Claim] } from ledger
  observations: [Observation]
  gaps:         [string]   // what was looked for and not found; rendered, not hidden
}

Draft {
  mention_id:   string
  template_ids: [string]
  text:         string     // ≤ platform limit, split into a thread when needed
  cites:        [Cite]     // Cite { sentence: u16, url: string, kind: "feed"|"ledger"|"observation" }
  autonomous:   bool
}

Reply {
  draft:        Draft
  post_id:      string
  posted_at:    i64
  reviewer:     "human" | "auto"
  edited:       bool
}

Sighting {
  platform:     string
  account_hash: u64        // xxhash64(platform + account id + local salt); never the handle
  account_age_days: u32?
  at:           i64
  url:          string?    // canonical
  text_hash:    u64?       // simhash, same function as feed
}

Observation {
  kind:         "burst" | "syndicated_text" | "cohort" | "first_movers"
  subject:      string     // url or text_hash
  window_secs:  u32
  count:        u32
  denominator:  u32
  detail:       object     // per kind, see §7
  computed_at:  i64
}
```

Every sentence in a `Draft` has a `Cite`. A draft with an uncited
sentence fails validation and is never posted. That check is the
whole enforcement of rule 6, and it is a unit test.

## 5. Platforms, in the order they are worth adding

1. **X**, the primary platform: where most campaigns run and where the
   bot is most wanted. Mentions are polled from `GET /2/users/:id/mentions`,
   which needs a paid API tier; replies go through `POST /2/tweets`, in
   reply to the mention, as a thread when needed. Auth is an OAuth 2.0
   user token with `offline.access`, refreshed on 401 and the rotated
   refresh token kept under `data/`. Automated replies are allowed only
   to mentions under X's automation rules, which is what `reply` does
   anyway. Written against the API terms as they stand; never scraped.
   The observer (R4) is the hard part here: there is no free firehose,
   and the filtered stream is a higher tier again.
2. **Bluesky**, the testbed: open, free, documented, no automation
   approval needed, so it is where every stage runs live first. Mentions
   are polled from the account's notifications with an app password;
   the firehose, which needs a websocket and CAR decoding, is only worth
   it for the observer (R4). Replies go through the same authenticated
   API as any client.
No other platform is planned. Disinformation campaigns run on X; the
bot's job is there, and Bluesky exists in this plan only as the place
to run each stage live before it runs on X.

An adapter implements `listen() -> Stream<Mention | Sighting>` and
`post(reply_to, text) -> post_id`. Everything else is platform-neutral.

## 6. Resolving and matching

- URLs go through `feed`'s canonicalizer, then `/v1/lookup?url=`. An
  exact item match is `confidence: 1.0`.
- A URL `feed` has never seen is itself a fact: the domain is checked
  against the outlet registry and, if unknown, an RDAP lookup gives the
  registration date, which the reply states. "Not an outlet First Report
  follows; domain registered 2026-09-04" is a useful answer.
- Text without a link is embedded with the same MiniLM model `feed`
  uses and matched to the nearest active cluster. The reply names the
  matched headline and the cosine score in plain words ("closest story
  First Report has, 0.71 similar") so the reader can reject the match.
- Below `match.min_confidence` the reply says no story matched, and
  says what it did check.
- The parent post is the subject when the mention has no link and no
  text of its own beyond the summons ("@firstreport prove it").

## 7. Coordination signals

All computed over `Sighting`s, all emitted as `Observation`s with
window, count and denominator, all with thresholds in config and
therefore in the open.

| kind | what is counted | detail |
|---|---|---|
| `burst` | distinct accounts posting the same url or text hash inside `burst.window_secs` | first and last time, median gap |
| `syndicated_text` | posts whose text is within the simhash threshold of each other, across accounts | the earliest post, share that are near-copies |
| `cohort` | share of accounts in a burst created within `cohort.span_days` of each other | span, share, versus the platform-wide baseline for that window |
| `first_movers` | accounts that were first or second to post in many distinct bursts over `movers.window_days` | count of bursts, share of all bursts |

A reply renders an observation as it is: "41 accounts posted this
link within 6 minutes; 28 of them were created in the same nine-day
span." What that means is the reader's call. The baseline in `cohort`
exists so the number is honest: some platforms have signup waves.

Accounts are stored as salted hashes. An observation can say "28
accounts" but the store cannot say which, and the public log never
names one. The sighting store is pruned after `sightings.retain_days`.

## 8. Composing

Templates are TOML files in `reply/templates/`, one per fact kind, with
a sentence, the facts it needs, and the cite it emits:

```toml
[origin]
needs  = ["cluster.origin_outlet_id", "cluster.origin_item.published_at"]
text   = "First published by {origin_outlet} at {origin_time}."
cite   = "feed:item:{cluster.origin_item_id}"

[spread]
needs  = ["cluster.outlet_count", "cluster.velocity.time_to_5_outlets_secs"]
text   = "Reached {outlet_count} outlets; five within {t5}."
cite   = "feed:cluster:{cluster.id}"

[syndication]
needs  = ["cluster.syndication"]
when   = "syndication.max_group_share >= 0.5"
text   = "{share} of those copies carry the same text, first from {first_outlet}."
cite   = "feed:cluster:{cluster.id}/syndication"

[domain_age]
needs  = ["ledger.domain_registered"]
text   = "Domain registered {domain_registered} per {registrar_source}."
cite   = "ledger:{outlet_id}:domain_registered"

[no_match]
text   = "First Report has no story for this. Checked {checked}."
cite   = "reply:lookup:{mention_id}"
```

The composer is deterministic. Order of sentences is the order in a
per-platform layout file. Threads are split at sentence boundaries. A
final sentence links the full `FactSet` on the public log so a reader
can see everything, not only what fit in the post.

`docs/voice.md` is the tone guide: what the bot sounds like, what it
never says, and the tests a sentence has to pass before it becomes a
template. The bot answers in the language it was summoned in: the
mention's language is detected, and the draft is composed from that
language's templates when the set is complete, else from English with
the fallback recorded on the draft and shown to the reviewer. A
translation is the same ids, facts and cites with only the sentences
changed. Templates are the community's surface. A better sentence, a new fact
kind, a translation: all are pull requests against `reply/templates/`,
tested by `reply render` over the fixtures. There is no other place
wording comes from. If an LLM is ever used, it is to choose among
rendered templates, not to write, and the chosen text must equal a
rendered one byte for byte.

## 9. Posting policy

- Reply only to a mention. Never to a post that did not summon the bot.
- One reply per mention. A second mention on the same post from the same
  account gets the same answer's link, not a new answer.
- Per-account and global rate limits in config, below the platform's.
- Every reply links its `FactSet` on the public log.
- Human review for every draft until a template's `autonomous_after`
  count is reached; even then, a draft containing an `Observation` is
  reviewed, because those are the ones that get quoted.
- Corrections: if `feed` later revises the facts (an item withdrawn, a
  cluster merged), the log entry is marked and a follow-up reply is
  drafted for review. The original is never deleted.

## 10. The public log

`reply serve` exposes it and `reply export` writes it. Every mention,
every draft, every reply, the `FactSet` behind each, which templates
fired, which thresholds were in force, who approved (human or auto),
and any later correction. Handles of the people who mentioned the
bot are kept only as long as the platform post exists and are dropped
from the export. This log is rule 7 made concrete, and it is also the
training set for improving the templates: what people asked, what the
bot had, where it fell short.

## 11. Repository layout

```
reply/
  Cargo.toml
  docs/plan.md
  docs/voice.md             the tone guide every template is held to
  templates/
    <lang>/*.toml           one file per fact kind, per language; en is the fallback, ja ships
    layout/<platform>.toml  sentence order and length rules (x: 280, bluesky: 300)
  fixtures/
    mentions/*.json         recorded mentions with the FactSet they resolved to
    mentions/*.<platform>.draft.json
                            golden drafts, rewritten by `check --update`
    bluesky/*.json, x/*.json
                            recorded platform responses for the parsers
    firehose/*.ndjson       recorded platform streams for the observer
  src/
    main.rs                 CLI: listen | render | review | serve | observe | export | check | queue
    cli.rs                  argument parsing
    model.rs                contract types, validation, the cite check
    facts.rs                FactSet → the view templates read, with derived values under d.
    platform/mod.rs         Listener trait
    platform/x.rs           API v2: mentions, tweets, token refresh; parsing is pure
    platform/bluesky.rs     XRPC: listNotifications, getPosts; parsing is pure
    queue.rs                data/queue/*.json, data/log/*.json and data/seen.txt
    client.rs               the Api trait: HTTP, and recorded responses for tests
    resolve.rs              mention → Subject → FactSet, via feed and ledger; language of the summons
    lang.rs                 copy of feed's language detector
    compose.rs              templates → Draft
    template.rs             TOML template loading and rendering
    observe/mod.rs          Sighting, salted account hash, thresholds, subject URLs
    observe/store.rs        data/sightings/<day>.ndjson, observations, ages, pruning
    observe/signals.rs      burst, syndicated_text, cohort, first_movers
    observe/simhash.rs      copy of feed's simhash
    api.rs                  review page, queue actions, the public log, limits
```

## 12. CLI

```
reply listen [--platform <p>] [--dry-run]   receive mentions, resolve, draft, queue
reply render <fixture>                       render a recorded mention through the templates, print draft and cites
reply review                                 open the queue page (also part of serve)
reply serve [--bind]                         review page, public log, API on 127.0.0.1:8082
reply observe [--from <ndjson>]              run signals over sightings, live or recorded
reply export --out <dir>                     log.ndjson, observations.ndjson
reply check                                  validate templates, layouts, and fixture renders
```

`render` and `check` need no network and run in CI. A template change
that alters any fixture's output shows up as a diff in the pull request.

## 13. API

```
GET /v1/log                                     every decided mention with its FactSet, draft, reply, corrections
GET /v1/log/{mention_id | queue_id}             one entry; the target of every reply's closing link
GET /v1/observations?subject=&kind=&since=      (R4)
GET /v1/queue, GET /v1/queue/{id}               pending drafts (local only)
POST /v1/queue/{id}/approve {keep: [i…]}        post, keeping only these sentences (local only)
POST /v1/queue/{id}/decline                     (local only)
GET /v1/stats, /llms.txt, /robots.txt
```

Editing is subtractive: a reviewer can drop sentences, not write new
ones, so a posted reply is always made of template sentences with their
cites. Anything else belongs in a template change.

## 14. Configuration

```toml
[feed]    url = "http://127.0.0.1:8080"
[ledger]  url = "http://127.0.0.1:8081"

[platform.bluesky]
handle = "firstreport.example"
# credentials from the environment, never from this file

[match]
min_confidence = 0.60

[burst]    window_secs = 600   min_accounts = 20
[cohort]   span_days = 14      min_share = 0.5
[movers]   window_days = 30    min_bursts = 5

[post]
per_account_per_day = 3
global_per_hour = 60
autonomous_after = 50           # unchanged approvals before a template may post alone

[sightings]
retain_days = 90
salt_env = "REPLY_SALT"
```

## 15. Testing

- Fixture mentions with their resolved `FactSet`s; `render` output is
  golden and any change is a reviewed diff.
- The cite check: a template without a `cite`, or a rendered sentence
  without one, fails `check`.
- Signals run over a recorded firehose with known planted bursts and
  known signup waves; the cohort baseline must absorb the wave.
- Adapters tested against recorded streams; no test touches a platform.
- A replay of the public log through new templates shows what would
  have been said differently.

## 16. Milestones

- **R0** crate, contract, templates, `render`, `check`, the cite check.
  Done, offline: 13 English templates over two layouts, six fixture
  mentions with golden drafts, and the cite check on every draft.
- **R1** `feed` gains `/v1/lookup`; the resolver works end to end.
  Done: `url` and `text` lookups with `matched` and `checked`; the
  resolver picks the subject, asks `feed` once and `ledger` once per
  outlet, collects display names, and records every miss as a gap.
  Verified against a `feed` serving the replay fixtures; `ledger` not
  yet run alongside.
- **R2** Bluesky listener, dry run: mentions in, drafts to the queue,
  nothing posted. Done: `listen` polls `listNotifications` for mentions
  (the firehose is left to the observer), fetches parent and quoted
  posts, drafts each mention into `data/queue/` and never drafts one
  twice; `queue` prints them. Parsing is tested on recorded responses;
  not yet run against a live account. Next: run it for weeks and read
  what people ask.
- **R3** Review page and poster. Done: `serve` lists pending drafts
  with every sentence's cite, the reviewer keeps or drops sentences
  (cites follow; the closing line stays) and posts or declines; posting
  threads under the mention with link facets; both decisions move the
  entry to the public log, which hides the summoner's handle; per-account
  and global limits are counted over the log. Tested with a fake poster
  and against a scratch queue; not yet run against a live account.
  First live replies are the next step, all human-approved.
- **R4** Observer and the four signals, shown on the public log before
  any reply carries one. Done: the four signals over sightings with
  salted account hashes, tested on planted bursts, a copied text, a
  young-account cohort against a background signup wave, and two first
  movers; `serve` lists observations at `/v1/observations`. Sightings
  are gathered on demand (R6), not from a firehose.
- **R5** Observations in replies, still reviewed; autonomous posting
  for templates that have earned it. Done: burst, syndicated_text and
  cohort templates (the cohort sentence always carries the baseline),
  rendered after the story and the misses and before the closing line;
  a draft with any observation is always reviewed. `listen
  --autonomous-after N` posts a draft by itself when its exact template
  set has N unchanged human approvals in the log; the same limits
  apply, a refusal leaves it for a person, and the log says `auto`.
  Off by default until the templates have settled.
- **R6** Observation on demand. Done: the bot is summoned, so no
  firehose. When a mention names a subject and the salt is set,
  `listen` searches the platform for recent posts carrying it (X's
  recent search, seven days; Bluesky's post search), records them as
  sightings with the author's age from the profile, runs the signals
  over the retained window so the baseline covers every account seen so
  far, and attaches the subject's observations. A failed search or an
  unset salt means a reply without observations and a gap that says so.
  Parsers tested on recorded responses; not yet run live.

## 17. Open questions

- **"Prove it" with no story.** Many summons will be under posts that
  assert a claim, not share a story, and `feed` works at story
  granularity. Until the claim pillar exists the honest answer is the
  nearest story and its confidence. Whether that is useful enough to
  post, or only to log, is decided by reading R2's queue.
- **Being the evidence.** A reply saying "41 accounts in 6 minutes"
  will be screenshotted as proof of a bot network. The observation is
  true and the inference is the reader's, but the bot's name is on it.
  The counter is the baseline in every cohort sentence and the log link
  in every reply. Whether that is enough is an open question that only
  running it answers.
- **Salt and subpoenas.** Salted hashes mean First Report cannot name
  accounts. Whether the salt should rotate, making old sightings
  unlinkable even with the salt, is a retention decision to make before R4.
- **Who runs the account.** The code is open and anyone can run their
  own bot from it. The canonical account's operator, its review
  policy and its public log are a governance question for the
  community, not a code question, and this plan does not settle it.
