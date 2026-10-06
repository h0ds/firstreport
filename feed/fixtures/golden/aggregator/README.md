# Evaluation data from a news aggregator's story groupings

A news aggregator groups outlet articles into stories by hand-curated
process. That grouping is a ready-made answer key for `feed`'s
clustering, and its article URLs are a spectrum-spanning outlet list.
Both are derived here from the 72 stories in the `json/` directory of
a public scraper of that aggregator, scraped in August 2025.

What is here:

- `extract.py` regenerates the two data files from a local checkout of
  that repository. The raw scrape is not included in Truth Decay and its
  scraper is not used or recommended.
- `aggregator-pairs.tsv` for `feed eval --pairs`: outlet headline plus a
  short snippet on each side, labelled 1 when a news aggregator put both under
  the same story, 0 otherwise. Negatives are weighted toward pairs with
  high lexical overlap, since those decide the join threshold.
- `aggregator-domains.txt`: distinct article hosts with counts, a seed
  list for `outlets.toml` via `feed discover <host>`.

What is deliberately not here: the aggregator's bias labels (their
aggregation of third-party ratings, not ours to ship; `ledger` cites its
own sources) and their perspective summaries (editorial text).

```
cargo run -- eval --pairs fixtures/golden/aggregator/aggregator-pairs.tsv
```
