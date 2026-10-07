# ledger

First Report pillar 2: sourced facts about outlets, keyed by the outlet ids in
`feed`'s `config/outlets.toml`. Who owns an outlet, where it is, when it
was founded, and how third parties rate it, each claim naming its
source, the source's licence and when it was retrieved. Sources that
disagree are shown side by side. There is no First Report score and no field
for one; `ledger check` rejects any. The plan is in `docs/plan.md`.

```
cargo run -p ledger -- --outlets ../feed/config/outlets.toml check
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikidata          # print proposal
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikidata --write  # store it
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikipedia_rsp --write  # reliability verdicts
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import rdap --write           # domain registration dates
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import site --write           # https and accountability pages
(cd ../feed && cargo run -- export --format ndjson --out ../export)
cargo run -p ledger -- --outlets ../feed/config/outlets.toml --feed-export ../export import feed_behaviour --write  # observed behaviour
cargo run -p ledger -- refresh --write                                                 # re-import sources that are due
cargo run -p ledger -- serve                                                           # 127.0.0.1:8081, /lookup for the search page
cargo run -p ledger -- export --out out/
```

Run from `ledger/` so `data/` resolves; or pass `--data`.

## Data

- `data/sources.toml`: the source registry. Each source carries its
  licence; `no_redistribution = true` makes `check` refuse its claims.
- `data/claims/<outlet_id>.toml`: one file per outlet, one `[[claim]]`
  per fact. Written by importers, editable by hand, reviewable in git.

Current sources: Wikidata (CC0), Wikipedia's perennial sources list
(CC BY-SA 4.0; one `reliability` claim per matching row, the list's own
five-step status with scope, discussions and summary), domain registries,
the outlet's own site, feed behaviour, Robertson et al. 2018, State Media
Monitor and EU sanctions.

## API (v1, read-only)

| route | returns |
|---|---|
| `GET /v1/outlets` | ids with claim counts, fields and sources |
| `GET /v1/outlets/{id}` | all claims grouped by field |
| `GET /v1/outlets/{id}/{field}` | claims for one field, every source |
| `GET /v1/search?q=` | outlets by id, name or domain, with claim and stale counts; covers the whole registry so "not assessed yet" is an answer |
| `GET /v1/sources` | the registry |
| `GET /v1/stats` | counts by source and field |

Every claim in a response carries `age_days` and `stale` against its
source's `refresh_days`. `serve` refuses to start on a ledger with
errors. `/lookup` is a search page over the same API: type an outlet,
see every claim grouped by field with its source, licence, retrieval
date and a stale marker.

## Re-assessment

Each source in `sources.toml` has `refresh_days`. `ledger check` lists
claims older than that and the sources due; `ledger refresh --write`
re-imports them. Put it on a schedule (weekly is plenty; the sources
move slowly) and review the diff before merging: the files are the
review surface.

A run that would change more than a third of a source's claims is held
in `data/review/` instead of written, and `check` says so until the
file is dealt with; `--force` writes it anyway. `check` also lists
fields where two sources disagree about an outlet (`../docs/steward.md`
§4).

## Status

L2e. Store, validation, refresh tracking, API with search and `/lookup`,
export, and five importers: Wikidata, Wikipedia perennial sources,
domain registration via RDAP, the outlet's own site (HTTPS and
accountability pages), and feed behaviour measured from `feed`'s own
export. The network importers are tested offline against responses in
each endpoint's format and have not run live, since the endpoints were
unreachable where this was built; feed behaviour needs no network and
is verified end to end. First thing to do on a networked
machine is `import wikidata`, then `import wikipedia_rsp`, then
`import rdap`, then `import site`, then `check`.
