# ledger

Kagami pillar 2: sourced facts about outlets, keyed by the outlet ids in
`feed`'s `config/outlets.toml`. Who owns an outlet, where it is, when it
was founded, and how third parties rate it, each claim naming its
source, the source's licence and when it was retrieved. Sources that
disagree are shown side by side. There is no Kagami score and no field
for one; `ledger check` rejects any. The plan is in `docs/plan.md`.

```
cargo run -p ledger -- --outlets ../feed/config/outlets.toml check
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikidata          # print proposal
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikidata --write  # store it
cargo run -p ledger -- --outlets ../feed/config/outlets.toml import wikipedia_rsp --write  # reliability verdicts
cargo run -p ledger -- serve                                                           # 127.0.0.1:8081
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
five-step status with scope, discussions and summary), AllSides (gated
until reuse terms are confirmed).

## API (v1, read-only)

| route | returns |
|---|---|
| `GET /v1/outlets` | ids with claim counts, fields and sources |
| `GET /v1/outlets/{id}` | all claims grouped by field |
| `GET /v1/outlets/{id}/{field}` | claims for one field, every source |
| `GET /v1/sources` | the registry |
| `GET /v1/stats` | counts by source and field |

`serve` refuses to start on a ledger with errors.

## Status

L2. Store, validation, API, export, and two importers (Wikidata,
Wikipedia perennial sources), all tested offline against responses in
each endpoint's format. Neither importer has run live: both endpoints
were unreachable where this was built. First thing to do on a networked
machine is `import wikidata`, then `import wikipedia_rsp`, then `check`.
