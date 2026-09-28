# Kagami 鏡

An open-source alternative to a news aggregator, built as a disinformation-analysis
network: where did a story originate, how fast did it spread, who amplified it,
was it syndicated or independently reported.

The project is split into two layers.

## Intelligence layer

Rust, single binary per tool, runs entirely locally. Three pillars, built in order:

| Tool | Pillar | Job |
|---|---|---|
| [`feed/`](feed/) | 1 | Ingest headlines, cluster them into stories, record propagation. Neutral by construction. |
| `ledger/` | 2 | Bias, factuality, ownership data keyed by outlet id. Not started. |
| `lens/` | 3 | Presentation over `feed` and `ledger`. Not started. |

`feed/docs/plan.md` is the full plan for pillar 1, including the project rules
that apply to every pillar.

## Visual layer

[`design/`](design/) holds the aesthetics document, the CSS tokens, and a
component preview page. Each tool copies `tokens.css` rather than depending
on it at runtime. The `feed` debug page is the first consumer.

## Getting started

```
cd feed
cargo run -- run
```

Requires a Rust toolchain. No Docker, no managed database, no cloud APIs.

## Status

M0 skeleton: config loading, data contract types, CLI stub, tracing. See
`feed/docs/plan.md` §11 for milestones.
