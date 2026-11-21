# mcp

`firstreport-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server over `feed` and `ledger`. An agent that has it can ask where a
story came from and what is on record about an outlet, and gets the
same sourced answer a person gets from the site: every fact with its
source, licence and date, and no verdict.

It is read-only. Each tool is one GET against one of the two local
APIs; it fetches nothing else.

| Tool | Reads | Answers |
|---|---|---|
| `lookup` | feed `/v1/lookup` | Everything on record about a URL or a passage: the item, its story, whether it was first, near-copies. |
| `stories` | feed `/v1/clusters` | Stories, by recency or by how fast outlets are joining. |
| `story` | feed `/v1/clusters/{id}` | One story: every report in join order, copies, confidence. |
| `item_revisions` | feed `/v1/items/{id}/revisions` | How one report changed after publication. |
| `outlet` | ledger `/v1/outlets/{id}` | Sourced facts about one outlet. |
| `find_outlet` | ledger `/v1/search` | Outlets by name, id, domain or URL. |
| `sources` | ledger `/v1/sources` | Where outlet facts come from, with licences. |

## Use

Start the two servers (`firstreport run`), then point a client at the binary.
For Claude Code:

```
claude mcp add firstreport -- /path/to/target/release/firstreport-mcp
```

For a client configured by JSON:

```json
{ "mcpServers": { "firstreport": { "command": "/path/to/target/release/firstreport-mcp" } } }
```

`FIRSTREPORT_FEED_URL` (default `http://127.0.0.1:8080`) and
`FIRSTREPORT_LEDGER_URL` (default `http://127.0.0.1:8081`) say where the
servers are.

## Status

The protocol handling and the routing of every tool are unit-tested.
The ledger tools have been run against a live `ledger`; the feed tools
have not yet been run against a live `feed`. Transport is stdio only:
a hosted endpoint over HTTP, for agents that cannot run a local binary,
is the next step once the site is public.
