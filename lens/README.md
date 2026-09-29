# lens

Pillar 3 of Kagami: a static site generated from `feed` and `ledger`
exports. It arranges what the other two record and adds nothing: no
score, no lean colour, no summary in its own voice. Plan and page
definitions in `docs/plan.md`.

```
cd feed   && cargo run -- export --format ndjson --out ../export/feed
cd ledger && cargo run -- export --out ../export/ledger
cd lens   && cargo run -- build --feed-export ../export/feed --ledger-export ../export/ledger --out ../site
             cargo run -- check --out ../site        # links resolve, twins present
             cargo run -- serve --out ../site        # http://127.0.0.1:8082/
```

`build` writes into a fresh directory and renames it over the target,
so a half-built site is never served. The result is plain files: host
them anywhere static.

## Pages

- `/` stories in the window as cluster cards, then every outlet.
- `/story/<id>/` timeline in join order with headlines side by side,
  copies, changes, and a claim card per outlet.
- `/outlet/<id>/` every claim with its provenance line, then the
  outlet's stories and counts over 30 days.

Every page has `index.json` and `index.md` beside it with the same
facts in the same order, linked from `<head>`. `/llms.txt` and
`/robots.txt` are written too.

## Tests

```
cargo test -p lens
```

The build test builds the site from the recorded exports in
`../ledger/fixtures/feed_export` and `fixtures/ledger_export`, runs
`check` on it, and diffs three twins against `fixtures/expected/`.
After a deliberate change to a page, regenerate the golden copies and
review the diff:

```
LENS_UPDATE_GOLDEN=1 cargo test -p lens
```

## Status

N1. Story and outlet pages with twins, a first front page, `check`,
`serve`, `llms.txt`. Verified offline end to end; not yet built from
live data. N2 adds the "spreading now" list, search, about, sitemap and
JSON-LD on the front page.
