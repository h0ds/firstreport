# Algorithms: every rule Kagami runs, stated in public

Kagami has no editors and no hidden model of what matters. Everything
that orders, groups, holds or flags is a rule over counts, and every
such rule is written here, in the about page, and next to the number
it produces. This file is the register. A rule that is not in it does
not run; a rule in it can be checked against the JSON twin of any page.

The line that all of them keep (`feed/docs/plan.md` rule 6,
`docs/steward.md` §1): a rule may count coverage, time, text and our
own behaviour. It may never take an outlet's identity, reputation, lean
or any ledger label as an input. Two outlets are interchangeable to
every algorithm below.

## 1. Same story: clustering

Where: `feed`, `feed/docs/clustering.md`. Inputs: headline and summary
text, publish time, language. An item joins the active cluster whose
centroid is nearest in embedding space, if the cosine similarity is at
or above `join_threshold` (0.72, tuned on the golden pair sets) and
the cluster was active within the window. Two clusters merge when
their centroids are at or above `merge_threshold` (0.80). The
thresholds are in `config/feed.toml`; the precision and recall they
give are printed by `feed eval`. Each story will carry `join_margin`
(steward S2) so a story held together by one borderline join says so.

## 2. Same text: syndication

Where: `feed`. Inputs: text fingerprint (simhash of headline and
summary). Items whose fingerprints are within Hamming distance 3 are
the same copy; the group's first outlet by published time is the one
that "had it first". A copy is not coverage: nothing below counts a
syndication group more than once.

## 3. Who was first: origin

Where: `feed`, shown on every story. The origin outlet is the member
with the earliest claimed publish time; `joined_at` is when Kagami
saw it, shown beside it because feeds lie about times. A feed the
steward flags `timestamps_unreliable` (§5) keeps its claimed time on
the page and the flag beside it; Kagami does not correct it.

## 4. Front page order: reach

Where: `lens`, front page and its JSON twin. This is the one place
Kagami says one story matters more than another, so the rule is the
strictest. A story's **reach** over the last 24 hours is:

    reach = independent reports + languages − 1

- **independent reports**: outlets that joined in the last 24 hours,
  counted once per syndication group, so a wire story carried by
  twenty subscribers is one report plus the subscribers that rewrote
  it.
- **languages**: distinct item languages among those reports, so a
  story reported in two languages counts one more than a story the
  same size in one. Language is detected from the item's own text, not
  from the outlet.

Stories are ordered by reach, then by outlets joining in the last six
hours, then by recency, then by id. Every card shows the three counts
that made its place. Stories with one independent report sit in a
separate list, "single reports", below, in recency order: a story that
only one outlet has told is shown, not hidden, and not mixed in.

The "spreading now" list is unchanged: outlets joining in the last six
hours, most first, shown only when it differs from the top of the
main list.

What reach does not do: it does not read the ledger, so a story
covered by outlets any list rates badly ranks exactly as one covered
by outlets it rates well. It has no words in it: nothing about
"politics" or "celebrity" is known to it, and a trivial story that
many outlets report independently in several languages will rank
high. That is the honest outcome of a count of coverage; a page that
disagreed with it would be an editor.

## 5. Judging our own pipe: the steward

Where: `feed`, `docs/steward.md` §2. Over a feed's last twenty fetches
with a body, five rules over parse failures, duplicate rate, publish
time skew, clamped future dates and burst decide whether the feed is
quarantined for a day or flagged. The reason is on the feed row and
at `/v1/feeds?filter=quarantined`. No rule reads the outlet.

## 6. Outlet pages: counts, never rank

Where: `lens` outlet page, `ledger` feed_behaviour importer. Per
outlet in the window: stories it was in, how many it was first to,
how many of its items were copies and how many were copied. The same
definitions in both places so the numbers agree. Outlets are listed
alphabetically or by join time and never ordered by any of these.

## 7. Label bars: the source's map, not ours

Where: `lens` story page. For a ledger source whose labels cover at
least two of the story's outlets, the outlets are grouped under that
source's own labels, headed with the source and licence. Kagami adds
no label, merges no sources, and orders the groups as the source
publishes them.

## Changing a rule

A change to any rule above is a change to this file in the same
commit, with the old rule kept under a dated heading at the end, so a
reader of an old JSON twin can find the rule that produced it.
