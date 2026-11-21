# deploy

What puts the site on the internet. `docs/deploy.md` is the plan; these
files implement its §4.4, §5 and §6.

| File | Job |
|---|---|
| `wrangler.jsonc` | The Cloudflare Worker: the built site as static assets, the archive bucket bound as `ARCHIVE`. |
| `worker.js` | Runs only when a request matches no file. Serves an archived story page from R2, else the site's 404. |
| `worker.test.mjs` | `node deploy/worker.test.mjs`; CI runs it. |
| `bin/site.sh` | Every 10 minutes: pull, rebuild if the code changed, `firstreport site`, `wrangler deploy`, ping the health check. |
| `bin/daily.sh` | Daily: copy live story pages to the archive, prune feed's store. |
| `systemd/` | `firstreport-feed` (always on), `firstreport-site` and `firstreport-daily` (timers), `firstreport-litestream` (database backup). |
| `litestream.yml` | `feed.db` to R2, once a minute. |
| `env.example` | The settings and secrets the jobs read, to copy to `/etc/firstreport/env`. |
| `setup.sh` | First-boot setup of the box. Safe to run again. |

## Status

Live since 2026-11-21. The box is `hoard-alpha` (Ubuntu 24.04, x86_64,
on Tailscale), shared with other work; First Report runs there as the
`firstreport` user. The site is at
https://firstreport.cc (registered 2026-11-21 with Cloudflare Registrar;
Always Use HTTPS is on). The workers.dev address still answers.

| Piece | State |
|---|---|
| `wrangler deploy` from a laptop | done |
| R2 bucket `firstreport-archive` | created |
| `firstreport-feed` | running |
| `firstreport-site` timer | running; first run deployed from the box |
| `firstreport-daily` timer, `firstreport-litestream` | not started: they need the R2 keys in `/etc/firstreport/env` |
| Archive round trip (`bin/daily.sh`, then a story URL only in the bucket) | not tried |

## What the first setup ran into

- **onnxruntime moved.** `github.com/microsoft/onnxruntime` returns 404,
  so `feed fetch-model` fails to download the runtime. On hoard-alpha
  `models/libonnxruntime.so` came from Microsoft's NuGet package
  `Microsoft.ML.OnnxRuntime` 1.27.1
  (`runtimes/linux-x64/native/libonnxruntime.so`). `fetch-model` still
  points at GitHub.
- **Node.** Ubuntu's `nodejs` is 18; wrangler 4 runs on the NodeSource
  build of Node 22 installed on the box.
- **pyftsubset.** Ubuntu's `python3-fonttools` has no `pyftsubset`
  command; `design/fonts/fetch.sh` now falls back to
  `python3 -m fontTools.subset`.
- **Repository access.** The repository is private; the box reads it
  with a read-only deploy key (`hoard-alpha (read-only)`) held by the
  `firstreport` user.
- **Tokens.** The Workers token and the R2 token are both limited to
  hoard-alpha's two addresses, 62.238.106.245 and 2a01:4f9:c01d:bd2::1.
  A new box needs new tokens or a changed filter.
