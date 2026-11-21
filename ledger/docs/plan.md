# ledger — Outlet Facts Layer

Pillar 2. `ledger` holds what is known about each outlet: who owns it,
where it is, when it was founded, how third parties rate its bias and
reliability. It is keyed by the outlet ids in `feed`'s `outlets.toml`,
which is the only thing the two share. It never emits a First Report score.

The project rules in `feed/docs/plan.md` §1 apply. The one that matters
most here is rule 6, restated for this layer:

> `ledger` records claims and where they came from. It does not make
> claims of its own. Every value carries its source, the source's
> licence and the date it was retrieved, and sources that disagree are
> shown side by side, never averaged.

## 1. Why a ledger and not a database

Outlet facts are few (a handful per outlet, thousands of outlets at
most), change slowly, and every one of them is contestable. The right
storage is a set of plain files in git: one TOML file per outlet, one
claim per entry, each claim naming its source. Reviewable in a diff,
correctable by pull request, no runtime dependency. `ledger serve` reads
the files and answers HTTP; `ledger import` writes new claims from a
source; `ledger check` validates the lot.

## 2. Data contract (v1)

```
Source {
  id:       string    // "wikidata", "wikipedia_rsp", "allsides"
  name:     string
  url:      string
  licence:  string    // SPDX id or a short name: "CC0-1.0", "CC-BY-SA-4.0", "proprietary"
  kind:     "database" | "community" | "rating" | "observed"
  notes:    string    // how it is used, what its terms allow
}

Claim {
  outlet_id:    string   // feed outlet id
  field:        string   // see §3
  value:        string   // human-readable
  data:         object?  // structured form when one exists, e.g. { "qid": "Q9531" }
  source_id:    string
  ref_url:      string   // the page that states this
  retrieved_at: i64      // unix seconds
  as_of:        string?  // date the source says the fact holds, if it says
  note:         string?
}

Outlet view (API) {
  id:     string
  claims: [Claim]        // all claims, grouped by field client-side
}
```

There is no numeric aggregate anywhere in the contract. A `bias` claim
from AllSides is AllSides's label on AllSides's scale; a `reliability`
claim from Wikipedia's perennial sources list is that list's verdict
with a link to the discussion. Consumers weigh them; `ledger` doesn't.

## 3. Fields

| field | meaning | typical source |
|---|---|---|
| `wikidata_id` | the entity | wikidata |
| `name` | the outlet's own name in the source | wikidata |
| `country` | country of the organisation | wikidata |
| `founded` | inception date | wikidata |
| `owner` | direct owner (P127) | wikidata |
| `parent` | parent organisation (P749) | wikidata |
| `legal_form` | company, public broadcaster, nonprofit, state media… | wikidata |
| `website` | official website | wikidata |
| `wikipedia` | English article URL | wikidata |
| `bias` | a rater's political-lean label, on that rater's scale | allsides, others |
| `audience_partisanship` | who shares the outlet's links, on the rater's numeric scale; a measure of audience, not content | robertson2018 |
| `reliability` | a rater's or community's reliability verdict | wikipedia_rsp, others |
| `domain_registered` | registration date of a registrable domain, per registry | rdap |
| `registrar` | the domain's registrar of record | rdap |
| `posting_cadence` | items per day over the window | feed_behaviour |
| `revision_rate` | share of items revised after publication | feed_behaviour |
| `withdrawal_rate` | share of items withdrawn | feed_behaviour |
| `syndication_share` | share of items whose text first appeared elsewhere, and share carried by others | feed_behaviour |
| `origin_share` | share of multi-outlet stories the outlet published first | feed_behaviour |
| `site_https` | whether the site serves HTTPS and moves plain HTTP to it | site |
| `site_pages` | which accountability pages the home page links: about, contact, privacy, corrections, ethics, masthead | site |

The `feed_behaviour` fields are measurements First Report makes itself. Each
claim carries the window, the counts and the denominator, and no
outlet with fewer than ten items in the window gets one. They describe
what a feed does, not whether that is good: a wire service is meant to
be copied, a live blog is meant to be revised.

Fields are open: an importer may add one, and `ledger check` warns on
unknown fields rather than failing, so a new source doesn't need a
schema change.

## 4. Sources, in the order they are worth adding

1. **Wikidata** (CC0). Ownership, country, inception, legal form,
   website, Wikipedia link. Facts, not opinions, and unrestricted. First
   importer.
2. **Wikipedia: Reliable sources/Perennial sources** (CC BY-SA 4.0).
   Community consensus on reliability for several hundred outlets, each
   verdict linked to its discussions. Second importer.
3. **AllSides** (chart is CC BY-NC-ND; per-rating reuse to be confirmed
   before import). Left/right bias labels with published methodology.
4. **Ad Fontes, MBFC, NewsGuard** (proprietary). Only via their APIs
   under their terms, if ever. Not scraped.
5. **Domain registries via RDAP** (public registration data). Facts we
   observe ourselves rather than license: when each of the outlet's
   domains was registered, by which registrar, with what status. A
   three-month-old domain is a different thing from one from 1995, and
   the ledger says which without judging.
6. **Feed behaviour** (First Report's own measurement, from `feed`'s export).
   Cadence, revision and withdrawal rates, syndication in both
   directions, origin share. No network; refresh weekly from a fresh
   export.
7. **The outlet's own site** (First Report's own observation). HTTPS, and
   whether the home page links about, contact, privacy, corrections,
   ethics and masthead pages. Two requests per domain, monthly.
   Absence means "not linked from the home page", and the claim says
   so; it is not a finding that the page does not exist.

Aggregators' labels are an aggregation of 3 and 4 and are not a
source; they remain a private answer key for checking `ledger`'s own
sourced claims.

## 5. Repository layout

```
ledger/
  Cargo.toml
  docs/plan.md
  data/
    sources.toml            registry of sources with licences
    claims/<outlet_id>.toml one file per outlet, [[claim]] entries
  fixtures/
    wikidata/               recorded SPARQL responses for tests
  src/
    main.rs                 CLI: check | serve | export | import
    model.rs                Source, Claim, validation
    store.rs                load data/, index by outlet, write claims
    wikidata.rs             SPARQL query and response → claims
    perennial.rs            perennial sources table → claims
    rdap.rs                 registry RDAP record → claims
    behaviour.rs            feed export → observed-behaviour claims
    site.rs                 home page → site_https, site_pages claims
    html.rs                 shared HTML helpers
    api.rs                  read-only HTTP
    export.rs               NDJSON
```

## 6. CLI

```
ledger check                       validate data/ against sources and feed's outlets.toml
ledger serve [--bind]              read-only API on 127.0.0.1:8081
ledger export --out <dir>          claims.ndjson, sources.ndjson, outlets.ndjson
ledger import <wikidata|wikipedia_rsp|rdap|site|feed_behaviour> [--outlets <feed outlets.toml>] [--only <id>] [--write] [--force]
              feed_behaviour also needs --feed-export <dir> [--feed-api <url>] [--window <days>]
```

`import` prints what it would add and writes only with `--write`, like
`feed discover`. Imports replace an outlet's existing claims from the
same source, so re-running refreshes rather than duplicates.

**Change guard** (`docs/steward.md` §4). A run that would remove or
change more than a third of the source's existing claims over the
outlets in the run (once it has at least six) is not written: the
fresh claims go to `data/review/<source>-<time>.toml` with the counts
in its header, the run says so, and `check` lists the file until it is
deleted. A source page whose structure changed produces exactly this;
read the file against the store, fix the importer or re-run with
`--write --force`. `check` also lists every field where two sources
give different values for one outlet: the side by side, named.

## 7. API

```
GET /v1/outlets                       ids with claim counts
GET /v1/outlets/{id}                  all claims for one outlet
GET /v1/outlets/{id}/{field}          claims for one field
GET /v1/sources                       the source registry
GET /v1/stats
GET /v1/openapi.json                  OpenAPI 3.1 for the routes above
GET /llms.txt, /robots.txt            for machine readers (design §11)
```

## 8. Milestones

- **L0** crate, model, store, `check`, `serve`, `export`. Offline.
- **L1** Wikidata importer against recorded fixtures; live run on a
  machine with network.
- **L2** Wikipedia perennial sources importer. Done: parses the rendered table, matches rows by article (from Wikidata), domain or name, one claim per row with scope. Not yet run live.
- **L2b** Re-assessment (`refresh_days`, `check` stale report, `refresh`), `/v1/search`, and the `/lookup` page. Done.
- **L2c** RDAP domain registration importer: one lookup per registrable domain through the rdap.org redirector, `domain_registered` and `registrar` claims linking the registry record. Done, tested against a recorded record; not yet run live.
- **L2d** Feed-behaviour importer from `feed export`: cadence, revision and withdrawal rates, syndication share, origin share, with window and counts on every claim. Done and verified offline end to end, since it needs no network.
- **L2e** Site checks importer: HTTPS behaviour and accountability-page links from the home page. Done, parser tested against a recorded page; not yet run live.
- **L2f** Steward guards (`docs/steward.md` S3): the change guard with
  review files and `--force`, and the conflict list in `check`. Done.
- **L3** AllSides, once terms are confirmed; a `licence` gate in `check`
  refuses to serve claims from a source whose terms field says
  `no-redistribution`.

## 9. Open questions

- **Matching outlets to Wikidata entities.** By official website (P856)
  against the outlet's domains is precise but misses outlets whose
  Wikidata item lists a different host. Fallback: label search, with
  the match recorded in the claim's `note` so a reviewer can reject it.
- **Ownership chains.** P127 gives the direct owner; the ultimate owner
  needs a walk up P127/P749. Store the direct link and let `lens` walk;
  or store the chain as `data.chain`. `lens/docs/plan.md` §10 does not
  need the chain for N1; it links the direct owner when the owner is
  itself a registry outlet. Decide when N3 is designed.
- **Time.** Ownership changes. `as_of` carries what the source says;
  the retrieved date carries what we saw. Re-import periodically and
  keep old claims in git history rather than in the files.
