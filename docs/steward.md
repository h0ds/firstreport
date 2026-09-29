# Steward: judging our own pipe, never the publishers

Kagami's standing rests on two things: that it never characterises an
outlet itself, and that what it does record is reliable. The first is
rule 6 in `feed/docs/plan.md`. The second needs an internal layer that
judges the quality of the data pipe, the behaviour of our own
algorithms, and our own conduct toward publishers, and acts on that
judgement in the open. This document is that layer's design. It is
called the steward because it keeps the house, not the guest list.

## 1. The line

The steward judges **feeds, algorithms and ourselves**. It never
judges **outlets**. Concretely:

- A feed can be quarantined for being technically broken: timestamps
  that lie, the same items republished under new URLs, floods, parse
  failures. The outlet's other feeds, its items already recorded, and
  its place in the pool are untouched, and the reason is shown.
- A story can carry how sure the clustering was, because that is a
  fact about Kagami's method.
- A ledger import can be held for review when it would change too
  much at once, because that is a fact about the importer or the
  source's page, not about the outlet.
- Kagami can throttle or stop fetching a host, because that is our
  conduct.

Nothing the steward decides is ever an input to how an outlet is
shown, ordered or coloured. If a rule here cannot be stated as "the
feed", "the algorithm" or "we", it does not belong here.

Every decision is a rule over counts, stated in one sentence, tested on
recorded fixtures, and visible where it applies: `/v1/feeds`, the
story's JSON and Markdown twins, `ledger check`, `kagami doctor`.
Nothing learned, nothing hidden, nothing silent.

## 2. Feed health

What the poller sees on every fetch is enough to tell a feed that works
from one that is junk. Per feed, over a rolling window of the last
twenty fetches with a body, the steward keeps:

| Signal | What it measures | Broken looks like |
|---|---|---|
| `parse_errors` | fetches whose body failed to parse | half or more |
| `dup_rate` | new items whose text fingerprint was already seen in this feed's recent items | half or more of a fetch's new items, over several fetches |
| `time_skew_secs` | median distance between an item's claimed publish time and the fetch that found it | days, on a feed polled every quarter hour |
| `future_rate` | items whose claimed publish time was ahead of the fetch and had to be clamped | a steady fraction rather than the odd one |
| `burst` | new items in a fetch against the feed's own median | ten times the median, with fifty or more items |
| `empty_run` | consecutive fetches with a body but no items at all | days of empty bodies |

Rules, each one sentence:

1. **Parse quarantine.** Ten or more fetches in the window and at
   least half failed to parse: quarantine for a day.
2. **Duplicate quarantine.** Five or more fetches with new items and
   the duplicate rate at or above half: quarantine for a day.
3. **Flood quarantine.** A fetch with at least fifty new items and ten
   times the feed's median: quarantine for a day, and the items of that
   fetch are held, not ingested.
4. **Timestamp flag.** Median skew above seven days, or future rate
   above a tenth: the feed is flagged `timestamps_unreliable`. It keeps
   polling and ingesting; the flag travels with the feed so a consumer
   can prefer pick-up time for its items.
5. **Release.** A quarantine ends after its day; the next fetch is
   evaluated afresh, so a feed that recovered is back without anyone
   touching it, and one that has not is quarantined again with the
   same reason.

Quarantine means: the feed is polled at its maximum interval so the
signals keep updating, its body is still archived as a snapshot, but no
items from it enter the store, and the reason and the release time are
on the feed row and in `/v1/feeds`. A dead feed (seven days of fetch
failures) is a different state that already exists.

Nothing here reads the items' content beyond the fingerprint and the
timestamps that were already computed. Nothing here knows which outlet
the feed belongs to.

## 3. Clustering confidence

Each story carries, in its JSON and Markdown twins:

- `join_margin`: the smallest margin over the join threshold among its
  members, so a story held together by one borderline join says so
  (with `weakest_join`, the threshold, and how many members are
  unscored: the seed, and items moved by a merge).
- `merged`: whether a merge pass produced it.
- `languages`: the set of item languages, so a mixed-language story is
  visible as such once the multilingual model runs.

Done: `confidence` on the contract `Cluster`, `join_similarity` on
each item and timeline entry, a Method section on every lens story
page and its twins. An export from before it was recorded says so.

These are facts about the method, shown as counts, never as a badge on
an outlet. Per-language thresholds are a tuning decision taken from the
golden sets, recorded in `feed/docs/clustering.md`.

## 4. Ledger import guards

`ledger import` and `refresh` already validate every claim against the
source registry and its licence. Added:

- **Change guard.** A refresh that would remove or change more than a
  third of a source's existing claims is written to a review file
  instead of the store, with the diff summarised, and `check` reports
  it. A source page whose structure changed produces exactly this.
- **Conflict list.** `check` lists fields where two sources disagree
  for the same outlet, so the side-by-side is known rather than
  discovered on a page.
- **Source decisions are human.** Whether a source is used at all is a
  line in `sources.toml` with a reason; the steward never adds or
  removes one.

## 5. Conduct and resources

- robots.txt and Crawl-delay on every request, the User-Agent naming
  the repository, no article fetching: done, `docs/publisher-rules.md`.
- **Budgets.** A per-host request budget per hour and a global one,
  enforced by the shared client; a host over budget waits. A disk
  budget for snapshots that shortens retention before the disk fills,
  reported by `doctor`.
- **Opt-outs.** A list in the repository of hosts that asked to be left
  alone, honoured before robots.txt is even read.

## 6. Ordering in lens

The front page orders stories by outlets joining in the last day, then
recency. The one change worth making is to count distinct syndication
groups rather than raw items wherever items are counted, so a wire
story carried by twenty subscribers does not look like twenty
independent reports. Whatever the rule, it is a count, and the about
page states it.

## 7. Milestones

- **S1** Feed health: signals, rules 1 to 5, quarantine on the feed
  row, `/v1/feeds` and `doctor` show it. Done in `feed/src/steward.rs`,
  tested rule by rule on synthetic windows and end to end through the
  poller; the recorded-feed fixtures (a lying feed, a duplicating feed,
  a flood) wait on live data to record.
- **S2** Clustering confidence in the contract as an addition, shown
  in lens twins. Done.
- **S3** Ledger change guard and conflict list.
- **S4** Budgets and the opt-out list.

S1 first, because the first live run over 96 feeds will meet junk
feeds before it meets anything else.
