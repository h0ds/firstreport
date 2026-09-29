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
  should be a small fraction of `fetches`.

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

For each failing feed, `cd feed && cargo run -- discover <homepage>`
finds the feeds the site advertises; replace the URL in
`config/feeds.toml`. A host answering 403 or 404 to every UA has no
public feed; drop it or find a mirror. For Japanese clustering add
`"ja"` to `cluster.languages` and use the multilingual preset (step 8).
Then widen further with discovery over the a news aggregator domains:

`feed discover --hosts` batch mode was written to run over the 888
domains extracted from the a news aggregator scrape, but it needs to fetch
each host.

```
cd feed
cargo run -- discover --hosts fixtures/golden/aggregator/aggregator-domains.txt --limit 200 --max-per-host 2
cargo run -- discover --hosts fixtures/golden/aggregator/aggregator-domains.txt --limit 200 --max-per-host 2 --write
```

Review the proposed feeds and outlets before `--write`; some hosts
advertise comment feeds or per-section feeds that are not wanted. The
raw a news aggregator scrape is not in the repository (only the extracted
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

Put `ledger refresh --write` (with `--outlets` and `--feed-export`) on
a weekly schedule; it re-imports only sources that are due.

## 6. Fonts

`design/fonts/` holds only a README. It mentions a `fetch.sh` that was
never written because the GitHub releases it would download from were
blocked. Fetch the three families by hand from the sources in that
README (Instrument Serif, Inter, JetBrains Mono, all OFL), subset them
with the `pyftsubset` line given there, and drop the `.woff2` files in
`design/fonts/` (gitignored). Then open `design/preview.html` and
confirm Instrument Serif renders the wordmark; until then the serif
falls back to the system serif. Writing the script once the URLs are
confirmed is a five-minute follow-up.

## 7. Terms to confirm before importing

These are decisions, not commands, and the sites were blocked so I
could not read the current terms.

- **AllSides** (`allsides.com/media-bias/ratings`): the chart is
  CC BY-NC-ND; whether individual ratings may be redistributed with
  attribution is not stated on the chart page. The `allsides` source
  entry has `no_redistribution = true` until this is confirmed, and
  `ledger check` will refuse to serve its claims until that flag is
  cleared. No importer exists yet.
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
