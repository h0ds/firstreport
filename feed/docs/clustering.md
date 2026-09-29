# Clustering: tuning notes

Plan §5.3 expanded: how assignment and merging behave, how to measure
them, and what the current defaults are based on.

## The two thresholds

- **`cluster.join_threshold`** (default 0.72) is applied per item: cosine
  between the item's embedding and each active centroid within the time
  window; the best match at or above the threshold wins, otherwise the
  item seeds a new cluster. The same-outlet guard raises the bar by 0.08
  when the best cluster already holds three or more items from the
  item's outlet, so one outlet's live-blog updates don't swallow
  unrelated stories.
- **`cluster.merge_threshold`** (default 0.80) is applied between
  centroids on the periodic merge pass. It is deliberately higher than
  the join threshold: a merge moves whole clusters, so a false merge
  costs more than a false join.

Both are plain config values. Everything below is about choosing them.

## Measuring: `feed eval`

```
cargo run -- eval --pairs fixtures/golden/pairs.tsv
cargo run -- eval --pairs fixtures/golden/pairs.tsv --json
```

The pair set is a TSV of `label  text_a  text_b`, label 1 for "same
story", 0 for "different story". The command embeds both texts with the
configured backend, computes cosine, and sweeps thresholds, reporting
precision, recall and F1 at each, the best F1, the metrics at the
configured `join_threshold`, and the misclassified pairs worst first.

What the pair set must contain to be useful:

- **Cross-outlet paraphrases** of the same event (positives). These set
  the recall floor.
- **Same topic, different event** (hard negatives): this month's rate
  decision against last month's, one earthquake against another, the
  bill passing against the bill being postponed. A sentence-similarity
  model scores these high, and they are the cases that decide the
  threshold. Time windows catch some of them; the threshold has to catch
  the rest.
- **Follow-up stories** (negatives by our definition): the minister
  reacting to the decision is a different story from the decision. This
  is a judgement call; the pair set encodes it explicitly so the
  threshold is tuned to it.
- **Unrelated** stories (easy negatives), a handful, as a sanity floor.

Two pair sets ship:

- `fixtures/golden/pairs.tsv`: sixty hand-written pairs covering all
  four cases. A smoke set.
- `fixtures/golden/aggregator/aggregator-pairs.tsv`: about a thousand
  pairs derived from 61 real stories that a news aggregator grouped by hand,
  1,800 distinct outlet headlines, with negatives weighted toward
  lexically similar cross-story pairs. This is the set to tune on. See
  the README there for provenance and what was deliberately left out.

Once `feed` has run live for a day, a third set sampled from its own
clusters and labelled by hand will catch whatever the seed feeds do
that the aggregator's mix does not.

## The Japanese and cross-language set

`fixtures/golden/ja-pairs.tsv` holds sixty pairs over ten stories:
Japanese paraphrases, agency-marked syndicated copies, the same story
in English and Japanese, Japanese hard negatives on the same topic,
English-Japanese hard negatives and unrelated stories. It is the set
that decides whether a join threshold tuned on English holds for
Japanese and across the two, which only the multilingual preset can
answer; the English model and the hash backend score it near chance
on the cross-language pairs by construction. Run it alongside the
English sets and expect the threshold to move. Sixty hand-written
pairs is a smoke set; grow it from real Japanese clusters once feed
polls the Japanese outlets.

## Current numbers

### Hash backend (lexical stand-in, no model)

Run on the sixty-pair smoke set with `embed.backend = "hash"`:

| threshold | precision | recall | F1 |
|---|---|---|---|
| 0.46 (best) | 0.895 | 0.810 | 0.850 |
| 0.56 | 1.000 | 0.667 | 0.800 |
| 0.72 (configured) | 1.000 | 0.143 | 0.250 |

This says nothing about the real model. It says the harness works and
that a bag-of-words similarity needs a threshold around 0.5, which is
why the replay golden test uses 0.5 with this backend. The misclassified
list is all paraphrases the lexical embedder can't see through, as
expected.

On the a news aggregator set (985 pairs) the hash backend manages F1 0.60
at the bottom of the sweep (0.30) and 0.08 at the configured 0.72: real
paraphrases across outlets share far fewer words than invented ones,
which is the whole reason the model exists. Any threshold this backend
suggests is meaningless for production.

### MiniLM-L6 (production backend)

Not yet measured here: the model weights could not be fetched in the
environment where this was written. Run

```
cargo run -- fetch-model
cargo run -- eval --pairs fixtures/golden/aggregator/aggregator-pairs.tsv
```

and record the table in this section. Expectations from the model's
published behaviour: paraphrase positives around 0.8 to 0.95, hard
negatives (same topic, different event) around 0.6 to 0.8, unrelated
below 0.4. If that holds, the best F1 lands somewhere between 0.65 and
0.78 and the 0.72 default is in range. If hard negatives sit above 0.75
the default is too low and the alternative models below are worth
trying before raising it, since raising it costs recall on paraphrases.

## Model alternatives

Same tokenizer, same pipeline, config change only:

| model | dims | pooling | notes |
|---|---|---|---|
| `all-MiniLM-L6-v2` | 384 | mean | current default; fastest; trained on general sentence pairs |
| `bge-small-en-v1.5` | 384 | cls | higher on clustering benchmarks; set `embed.pooling = "cls"` |
| `gte-small` | 384 | mean | similar to bge-small |
| `all-mpnet-base-v2` | 768 | mean | best quality of the four, about 5× slower, doubles memory per vector |

Compare with the same pair set and pick by F1 at the threshold you will
actually run. Do not compare F1 at each model's own best threshold only;
the shape of the curve near the operating point matters more.

## Input template

`embed.input_template` decides what the model sees. The default
`{title} . {summary}` embeds both. Two variants worth evaluating on real
pairs:

- `{title}`: summaries carry outlet boilerplate (bylines, "Read more")
  that pulls vectors toward outlet style rather than story content.
- `{title} . {title} . {summary}`: weights the headline without dropping
  the summary.

## Determinism

Assignment is order-dependent by design: the same items in the same
order give the same clusters. Replay feeds items in `fetched_at` order
with single-threaded inference, and expiry and merging run on the item
clock, so `fixtures/expected/clusters.json` is reproducible on any
machine. ONNX Runtime results can differ by a few ulps between CPUs
and thread counts; the golden test therefore compares cluster
membership, never embeddings, and uses the hash backend so it needs no
model.

## Merge pass

Every `merge_interval_secs` (default 600) of item time, all pairs of
active centroids are compared. Pairs at or above `merge_threshold` with
overlapping windows are merged, smaller into larger (older survives on
a tie), unless the union would exceed `max_cluster_size`. Each cluster
merges at most once per pass; later passes catch chains. The absorbed
cluster's row is kept, expired with zero items, and the `merged` event
carries `merged_into`. With ten thousand active clusters the pass is
about fifty million dot products, well under a second on one core; it
becomes worth indexing only far beyond that.

## What to look at first on real data

1. `feed eval` with the real model on the smoke set, then on a pair set
   sampled from real clusters.
2. The `/debug` page for clusters that are obviously two stories (join
   threshold too low or window too wide) and for the same story split
   across clusters (threshold too high, or the same-outlet guard biting).
3. `/v1/stats`: `merges` climbing steadily means the join threshold is
   leaving fragments for the merge pass to clean up; consider lowering
   it slightly rather than relying on merges.
