# Local runbook: what the cloud sandbox could not do

Everything in this repository was built and tested in a sandbox whose
network policy blocks news sites, model hosts, Wikipedia, Wikidata,
registries, and most of the open web. All code paths that touch those
are verified offline against recorded responses only. This is the list
of what has never run for real, in the order to do it, with the exact
commands. Tick each off and note anything surprising in the relevant
`docs/plan.md`.

Requirements on the local machine: a Rust toolchain, Python 3 (for
one script), network access, and about 300 MB of disk for the model
and runtime.

## 0. Save the infra plan

`INFRA-PLAN.md` (the cheapest-infrastructure plan) is gitignored by
design and only existed in the sandbox and in the copy sent to you in
chat. Put your copy at the repository root so it stays next to the
code.

## 1. Push the release tag

The sandbox could push branches but not tags (403). The v0.1.0 tag
exists only locally there.

```
git tag -a v0.1.0 -m "feed v0.1.0: data contract v1 frozen" <commit>
git push origin v0.1.0
```

Use the commit that froze the contract (the M6 commit that added
`feed/docs/data-contract.md`; `git log --oneline -- feed/docs/data-contract.md | tail -1`).

## 2. Embedding model and ONNX Runtime

huggingface.co and GitHub release assets were blocked, so the real
embedding backend has only run in the sandbox with a runtime borrowed
from a Python wheel. Locally:

```
cd feed
cargo run -- fetch-model
```

This downloads the int8 MiniLM export, its vocabulary and the ONNX
Runtime shared library for your target triple into `models/`
(gitignored). Then confirm the backend is `onnx` in
`config/feed.toml` and that a replay embeds with the real model:

```
cargo run -- replay --fixtures fixtures/replay
cargo run -- eval --pairs fixtures/golden/pairs.tsv
cargo run -- eval --pairs fixtures/golden/aggregator/aggregator-pairs.tsv
cargo run -- eval --pairs fixtures/golden/ja-pairs.tsv      # meaningful with the multilingual preset only
```

Both `eval` runs print a precision/recall sweep over the join
threshold. The recorded golden clusters in `fixtures/expected/` were
produced with the real model in the sandbox, so `replay` should match
them; if it does not, the runtime version differs and
`feed/docs/clustering.md` explains what to check.

## 3. First live run of feed

No live fetch has ever succeeded: every news host returned 403 at the
sandbox proxy. This is the single most important thing to do.

```
cd feed
RUST_LOG=feed=info cargo run -- run
```

Then, in another terminal or a browser:

- `http://127.0.0.1:8080/debug` — clusters should start forming within
  two polling intervals.
- `http://127.0.0.1:8080/v1/feeds?filter=failing` — feeds that fail.
  Expect a few: dead URLs, hosts that reject the default User-Agent
  (the poller falls back to a browser-like one automatically), and
  hosts that rate-limit (handled by `Retry-After` and backoff). Anything
  that fails consistently for another reason is a bug; note the URL and
  the `last_error`.
- `http://127.0.0.1:8080/v1/stats` — `fetch_errors` and `parse_errors`
  should be a small fraction of `fetches`; `items_held` counts items
  the steward kept out; `budget_waits` should stay near zero with the
  default budgets, and a host that keeps hitting its budget has too
  many feeds configured for one hour.
- `http://127.0.0.1:8080/v1/feeds?filter=quarantined` — feeds the
  steward is holding (`docs/steward.md` §2), each with the one-sentence
  reason and the release time. Expect none on the first fetch and a
  few after a day: a feed that republishes its items under new URLs,
  or whose bodies stopped parsing. A quarantine is not a config
  problem and lifts itself; a feed that is quarantined again and again
  for the same reason is one to replace with `feeds --fix` or drop.
  `health.timestamps_unreliable` on a feed means its claimed publish
  times are days off; `lens` still shows both times.

Let it run for at least a day before judging clustering. Then:

```
cargo run -- export --format ndjson --out data/export/$(date +%F)
```

Things to watch in the first day, all discussed in
`feed/docs/plan.md` §11 and `feed/docs/clustering.md`:

- Cluster size distribution: many singletons is normal; a few huge
  clusters means the join threshold is too low for live data.
- Origin outlet: check a handful of clusters by hand against the
  outlets' own timestamps.
- Syndication groups: wire copy should group; different stories with
  similar boilerplate should not.

## 4. Grow the outlet pool

The pool is 95 outlets now: the original ten plus 85 added by hand
across the spectrum (wire, broadcasters, papers, partisan and fringe
sites, state media, and 19 Japanese outlets). Their feed URLs were
written from memory in a sandbox that could not fetch them, so the
first live poll is the verification:

```
curl -s 'localhost:8080/v1/feeds?filter=failing' | python3 -m json.tool | grep -E '"url"|last_error|last_status'
```

Reuters is in through its news sitemap; if `/v1/feeds` shows it as
"disallowed by the host's robots.txt", Reuters has said no and the
entry comes out. For the rest, one command does the discovery for every failing feed
at once and edits the config for review:

```
cd feed
cargo run -- feeds            # the failing list, from the store
cargo run -- feeds --fix      # replacement URLs, confirmed to fetch as feeds
cargo run -- feeds --write    # apply them to config/feeds.toml; git diff, then restart
```

A feed it cannot fix is listed with the reason; `cargo run -- discover
<homepage>` is the manual fallback. A host answering 403 or 404 to every UA has no
public feed; drop it or find a mirror. For Japanese clustering add
`"ja"` to `cluster.languages` and use the multilingual preset (step 8).
Then widen further with discovery over the aggregator domains:

`feed discover --hosts` batch mode was written to run over the 888
domains extracted from the aggregator scrape, but it needs to fetch
each host.

```
cd feed
cargo run -- discover --hosts fixtures/golden/aggregator/aggregator-domains.txt --limit 200 --max-per-host 2
cargo run -- discover --hosts fixtures/golden/aggregator/aggregator-domains.txt --limit 200 --max-per-host 2 --write
```

Review the proposed feeds and outlets before `--write`; some hosts
advertise comment feeds or per-section feeds that are not wanted. The
raw aggregator scrape is not in the repository (only the extracted
pairs and domains are); if you want to re-extract, `extract.py` in that
directory documents the input it expects.

## 5. Ledger imports

None of the four network importers has run live. Run them in this
order, each once with `--only` on one outlet to see the proposal, then
for everything, then with `--write`. Each import prints what it would
add and writes only with `--write`; the written files under
`ledger/data/claims/` are the review surface, so read the diff before
committing.

```
cd ledger
cargo run -- --outlets ../feed/config/outlets.toml import wikidata --only bbc
cargo run -- --outlets ../feed/config/outlets.toml import wikidata --write
cargo run -- --outlets ../feed/config/outlets.toml import wikipedia_rsp --write
cargo run -- --outlets ../feed/config/outlets.toml import rdap --write
cargo run -- --outlets ../feed/config/outlets.toml import site --write
cargo run -- --outlets ../feed/config/outlets.toml check
```

Notes per importer:

- **wikidata**: matches outlets to entities by official website against
  the outlet's domains, with label search as a fallback. A fallback
  match is recorded in the claim's `note`; reject wrong ones by editing
  the file. One SPARQL request per outlet, one second apart.
- **wikipedia_rsp**: fetches the perennial sources page once and
  matches rows by Wikipedia article (from the Wikidata import, so run
  that first), then domain, then name. Check that the table layout the
  parser expects has not changed; the parser fails loudly if it has.
- **rdap**: one lookup per registrable domain through rdap.org. Some
  country registries have no RDAP; those are skipped with a warning
  and get no claim.
- **site**: two requests per domain. A domain that answers on neither
  scheme is skipped, never recorded. Check a few `site_pages` claims
  by hand: the page-link detection is by link text and path, and a
  site with unusual labels will read as "not linked".

Then the observed-behaviour import from the export made in step 3:

```
cargo run -- --outlets ../feed/config/outlets.toml --feed-export ../feed/data/export/<date> import feed_behaviour --write
cargo run -- --outlets ../feed/config/outlets.toml check
cargo run -- serve
```

`http://127.0.0.1:8081/lookup` is the page; type an outlet and every
claim appears with its provenance line. Commit `ledger/data/claims/`.

If an import prints "held for review", the change guard tripped
(`docs/steward.md` §4): more than a third of that source's claims would
change. Read the file it names in `ledger/data/review/` against the
store. A source page that changed shape looks exactly like this, and
the fix is in the importer; a genuine change is applied with
`--write --force`. Delete the file either way, or `check` keeps
listing it. `check` also prints every field where two sources
disagree; those are correct as they stand and appear side by side on
the pages.

Put `ledger refresh --write` (with `--outlets` and `--feed-export`) on
a weekly schedule; it re-imports only sources that are due.

## 6. Fonts

`design/fonts/fetch.sh` downloads the three families (Instrument
Serif, Inter, JetBrains Mono, all OFL) from a pinned google/fonts
commit, subsets them and writes the four `.woff2` files into
`design/fonts/` (gitignored), about 164 KB together:

```
pip install fonttools brotli
sh design/fonts/fetch.sh
```

Then open `design/preview.html` and confirm Instrument Serif renders
the wordmark; without the files the serif falls back to the system
serif.

## 7. Terms to confirm before importing

These are decisions, not commands, and the sites were blocked so I
could not read the current terms.

- **AllSides** (`allsides.com/media-bias/ratings`): confirmed. AllSides'
  terms of use (October 2025) license the ratings CC BY-NC 4.0: free for
  research or noncommercial use with attribution; commercial use (ads, paid
  tiers, selling data) needs AllSides' written consent. The site blocks
  automated access, so do not scrape it: email admin@allsides.com asking
  for the ratings as CSV or JSON, then
  `ledger import allsides --file <file> --write`. The `allsides` source
  entry carries the licence and the required attribution text.
- **Robertson et al. 2018** partisan audience scores: CC0 on Harvard
  Dataverse, imported with `ledger import robertson2018 --write`. A measure
  of who shares an outlet's links, recorded as `audience_partisanship`.
- **Not usable as sources**: Media Bias/Fact Check (RapidAPI terms exclude
  redistribution, and Truth Decay's exports redistribute; a direct commercial
  licence would be needed), Ad Fontes Media (free licence covers the chart
  image for teaching, not the per-outlet data), Bakshy et al. 2015 (download
  terms forbid sharing).
- **Iffy Index** and similar lists: check licence before adding.

## 8. Multilingual embeddings: English and Japanese

Built and verified as far as the sandbox allowed: the SentencePiece
unigram tokenizer matches the Python reference on the sentencepiece
project's English and Japanese test models. What could not be done is
run the real multilingual model, whose files are on huggingface.co.

1. In `feed/config/feed.toml`, switch the `[embed]` block to the
   multilingual lines shown in the comment there (model path, vocab
   path, `tokenizer = "unigram"`, `preset = "multilingual-minilm-l12"`),
   and add `"ja"` to `cluster.languages`.
2. `cd feed && cargo run -- fetch-model`. It downloads the model's
   `sentencepiece.bpe.model` and int8 ONNX export for your machine.
   If the repository has no `onnx/` directory for your architecture,
   set `model_url` by hand to the `onnx/model.onnx` file.
3. Check the tokenizer against the real vocabulary once:

   ```
   pip install sentencepiece
   python3 -c "import sentencepiece as s; sp=s.SentencePieceProcessor(model_file='models/sentencepiece.bpe.model'); print(sp.encode('日銀、政策金利を据え置き'))"
   ```

   and compare with `RUST_LOG=feed=debug` output for the same headline,
   remembering the XLM-RoBERTa offset: feed's ids are those plus one,
   wrapped in 0 and 2. A difference on ordinary text is a bug to
   report with the headline.
4. Re-run both golden sets with the new model (`feed eval`). The join
   threshold will move; `feed/docs/clustering.md` says how to re-tune.
   Expect cross-language pairs to need a hand-labelled set of their
   own before the threshold is trusted for Japanese.

## 9. The model question

The links you sent about jev and System One models (typesafe.ai) and
the jeff weights on Hugging Face (mstrasser) were blocked. What I need
to place them: parameter count, whether the weights run locally,
licence, and what tasks they are built for. The three places a model
would be used first, all batch and none in the ingest path, are in the
conversation summary: golden-pair proposals for `feed eval`, an
`extracted` ledger source that reads about pages with the quote span
recorded, and Wikidata match adjudication.

## Verified in the sandbox, no local action needed

For completeness, these are fully verified offline and need no
special local step: the whole feed pipeline on recorded fixtures
(`replay`), export in NDJSON and Parquet, prune and the nightly job,
the golden evaluation with the hash backend, the ledger store and
`check`, the `feed_behaviour` importer end to end, both APIs, the
Markdown twins, `/llms.txt`, and the OpenAPI documents with schemas
generated from the types and paths from the route tables.
