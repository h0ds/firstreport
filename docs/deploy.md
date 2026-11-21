# Taking Truth Decay live

The plan for putting the public site (`lens`) on the internet and keeping
`feed` and `ledger` running behind it, as cheaply and as fast as the
workload allows. It builds on `INFRA-PLAN.md` (local, gitignored), with
prices and limits checked in September 2026.

## 1. Shape

```
                ┌──────────────── Hetzner CAX11 (one box) ────────────────┐
  outlets' ───▶ │ feed run (systemd)        ledger (data in git)          │
  RSS feeds     │   SQLite + snapshots        claims/*.toml               │
                │        │                        │                       │
                │        └──── every 10 min: kagami site ─────────────────┤
                │               (export → lens build → lens check)        │
                │                         │ wrangler deploy (changed      │
                └─────────────────────────┼─ files only) ─────────────────┘
                                          ▼
                     Cloudflare Worker with static assets  ◀── readers
                     (recent pages, free, unlimited reads)
                                          │ asset missing
                                          ▼
                     R2 bucket: archived story pages (written once)
```

- **Nothing public runs on the box.** Readers only ever hit Cloudflare's
  edge: a static site with no server behind it, so no load, no attack
  surface, and no bill that grows with traffic. The box only makes
  outbound connections (feeds, Cloudflare's API, backups).
- **The debug page, `/feeds` and ledger's lookup stay private**, reached
  over Tailscale.
- **The site rebuilds every 10 minutes.** Only files that changed are
  uploaded.

## 2. Cost

| Item | Service | Cost |
|---|---|---|
| The box | Hetzner CAX11: 2 ARM vCPU, 4 GB, 40 GB NVMe, 20 TB traffic | €5.99/mo + €0.50 IPv4 = **€6.49/mo** |
| The site | Cloudflare Workers static assets, free plan | **$0**: static requests are free and unlimited; 20,000 files per deploy |
| Archive | Cloudflare R2 | **$0** within the free 10 GB, 1M writes, 10M reads a month |
| Database backup | Litestream to R2 | **$0** (same free tier) |
| Domain | Cloudflare Registrar, at cost | ~**$10/yr** for a `.org` |
| Monitoring | healthchecks.io free plan | **$0** |
| TLS, CDN, compression, DDoS | Cloudflare | **$0** |
| **Total** | | **≈ €6.50/mo + $10/yr, about $90 a year** |

The only upgrade on the horizon is Workers Paid at **$5/mo**, if the live
site needs more than 20,000 files. §4.1 keeps it under.

Even cheaper: run the box at home (a Pi 5 or an old laptop, ~$1/mo of
power). Everything below works the same, because the box only makes
outbound connections. The trade-off is uptime: a home connection or a
closed laptop stops the rebuilds, though the published site stays up.

## 3. Why these choices

- **Static and edge-served** is both the cheapest and the fastest
  option. Every page is a file on Cloudflare's network in 300+ cities, so
  time to first byte is tens of milliseconds, with no origin to be slow or
  go down. Cloudflare compresses with Brotli automatically.
- **Workers static assets over Pages**: same price, but Workers can run a
  small script when a file is missing, which is how the R2 archive (§4.1)
  keeps old URLs working without a separate server.
- **Hetzner ARM** is the cheapest reliable always-on box with enough RAM
  for the ONNX model (~500 MB steady), and its 20 TB of traffic covers
  feed polling many times over. The `fetch-model` command already picks
  the ARM runtime and model.
- **No Kubernetes, containers, managed database or queue.** One process,
  one SQLite file, one directory of snapshots, as `INFRA-PLAN.md` says.

## 4. Code changes before launch

In order. Each is small; together they keep the file count, page weight
and cost flat as the data grows.

### 4.1 Bound the number of files

Today lens writes three files (HTML, JSON, Markdown) for every story in
the export. With 96 feeds that's about 4,700 stories a day, or 14,000
files a day, which would pass even the paid plan's 100,000-file limit
within a week.

Items 1 and 2 are built and off by default: `kagami site
--multi-outlet-pages --pages-since 14` (or the same flags on `lens
build`). Do not turn on the window on a public site until the archive
in item 3 exists, or old story URLs stop working.

1. **Story pages only for stories more than one outlet carried.** 92% of
   stories (2,357 of 2,567 on the day this was measured) are single
   reports. Their card links straight to the outlet's article; they gain a
   page when a second outlet joins. That brings it to roughly 1,200 files
   a day.
2. **A rolling window.** lens builds pages only for stories active in the
   last 14 days (`lens build --pages-since 14d`), about 17,000 files,
   under the free plan's 20,000.
3. **An archive, so old URLs keep working** (design §11: "a URL that
   worked keeps working"). When a story leaves the window, a daily job
   writes its final pages to an R2 bucket, once. A roughly 20-line Worker
   runs only when a requested file isn't in the live site: it serves the
   page from R2 with a long cache time. Archive writes are about 1,200 a
   day and reads mostly hit the edge cache, both well inside R2's free
   tier.

### 4.2 Lighter pages

- *(Done: JSON-LD now lists only the page's own stories.)* **The front page was 615 KB of HTML**, mostly because its JSON-LD lists
  every story in the export, not the 50 on the page. Limit JSON-LD to the
  page's own stories. Expected result: under 100 KB, about 15 KB after
  Brotli.
- **A generated `_headers` file:**
  - `/fonts/*` and `/icons/*`: `Cache-Control: public, max-age=31536000, immutable`, with a content hash in the file name so a change gets a new name.
  - `/tokens.css` and `/lens.css`: hashed names too, and the same rule.
  - HTML, JSON and Markdown: leave Cloudflare's default (revalidate every time with an ETag). A rebuild shows up at once, and an unchanged page costs a 304 and no bytes.
  - Security headers on every response: a `Content-Security-Policy` allowing only the site's own origin, `X-Content-Type-Options: nosniff` and `Referrer-Policy: strict-origin-when-cross-origin`.
- **Preload** Inter and Instrument Serif regular, so the first paint uses the right fonts.

### 4.3 No localhost in public pages

*(Done in lens: a loopback `ref_url` is rendered as the source's name
without a link, and `lens check` fails on any link to this machine.)*

377 links in 80 built pages pointed at `http://127.0.0.1:8080`: feed
behaviour facts cite feed's API as their source. The public site must
cite something public. Give feed-behaviour claims `ref_url` =
`https://<domain>/outlet/<id>/`, or leave `ref_url` empty for sources
where Truth Decay is itself the source, and have lens render "Truth Decay feed" as
text instead of a link. Also make `lens check` fail on any link to
`127.0.0.1` or `localhost`, so this can't come back.

### 4.4 Deploy config in the repo

- `deploy/wrangler.jsonc`: a Worker with `assets.directory = "../site"`,
  an R2 binding for the archive, and `not_found_handling` routing misses
  to the Worker.
- `deploy/worker.js`: the archive fallback (§4.1).
- `deploy/systemd/`: `truthdecay-feed.service`, `kagami-site.service` and
  `.timer` (every 10 min), `kagami-daily.service` and `.timer` (archive,
  prune, ledger refresh), and `litestream.service`.
- `deploy/litestream.yml`: `data/feed.db` to R2, `sync-interval: 1m`, so
  writes stay around 45,000 a month.
- `deploy/setup.sh`: idempotent first-boot setup (§5).

## 5. Setting up the box

1. **Accounts:**
   - Hetzner Cloud, with an SSH key.
   - Cloudflare, with the domain on Cloudflare DNS and an R2 bucket
     `kagami-archive`.
   - An API token scoped to Workers and R2 only.
   - Tailscale.
   - healthchecks.io.
2. **Create the server:**
   - CAX11 in Falkenstein or Helsinki, Ubuntu 24.04.
   - A Hetzner firewall allowing **no inbound traffic at all**; SSH over Tailscale only.
   - `unattended-upgrades` on.
3. **`deploy/setup.sh` does the rest:**
   - Creates a `kagami` user.
   - Installs Rust (stable) and Node LTS (for wrangler).
   - Clones the repo with a read-only deploy key.
   - Builds with `cargo build --release`.
   - Runs `feed fetch-model` (arm64) and `design/fonts/fetch.sh`.
   - Runs `feed icons`.
   - Installs the systemd units and `litestream`.
   - Writes secrets to `/etc/kagami/env` (mode 600): the Cloudflare token and healthcheck ping URLs.
4. **Seed the data:**
   - Copy the current `data/` from the laptop if you want to keep today's history; otherwise let it start fresh.
   - Ledger data comes from git.
5. **First deploy by hand:** `kagami site`, then `wrangler deploy`, then
   check the `*.workers.dev` URL, then attach the custom domain.

## 6. Running it

| Job | When | What |
|---|---|---|
| `truthdecay-feed` | always | `feed run`; restarts on failure. |
| `kagami-site` | every 10 min | `git pull` (ledger data and code), then `kagami site`, then `wrangler deploy`. Pings healthchecks.io only on success, so a failed build or deploy alerts within 30 minutes. |
| `kagami-daily` | 03:00 UTC | Archive stories leaving the window to R2; `feed prune --embeddings`; snapshot retention (30 days on the box, older days shipped to R2 per `INFRA-PLAN.md` §5); `ledger refresh --write` for sources that are due. |
| `litestream` | always | Continuous `feed.db` replication to R2. |

- **Ledger stays reviewed in git.** The daily refresh writes to a branch,
  `ledger-refresh/<date>`, and opens a pull request with `gh`. The live
  site uses `main`, so a surprising import (the change guard already holds
  large ones) is reviewed before it's published.
- **Updating code:** merge to `main`. The next `kagami-site` run pulls
  it, but only rebuilds binaries when `Cargo.lock` or `src/` changed
  (about 2 minutes on the CAX11).
- **Alerts** (healthchecks.io, free):
  - No successful site deploy in 30 minutes.
  - `feed`'s `/v1/stats` showing zero items in the last hour, checked by the site job.
  - A daily job failure.
- **Restore:** a new CAX11, `setup.sh`, then `litestream restore`. The
  public site stays up throughout, because it's static on Cloudflare.

## 7. Before announcing it

- [ ] §4.1–4.3 done; `lens check` passes, including the new localhost check.
- [ ] Lighthouse on the front page and a story page: performance ≥ 95, accessibility ≥ 95.
- [ ] `/about/` names every source with its licence, including AllSides' required attribution sentence once its data is in.
- [ ] **AllSides is licensed for non-commercial use only**: no ads, paid tiers or data sales without their written consent. Robertson et al. is CC0; Wikipedia is CC BY-SA (attribution is already on every claim).
- [ ] No cookies and no third-party requests. Optional: Cloudflare Web Analytics, which is cookieless and free.
- [ ] A contact address for outlets and readers, for corrections and opt-outs (`config/opt-out.toml` is already honoured).
- [ ] `robots.txt`, `sitemap.xml` and `llms.txt` point at the real domain (`lens build --base-url https://<domain>`).
- [ ] Decide whether the repository is public. The site's "open-source" claim assumes it is.

## 8. Order of work

1. §4.3 localhost links and the `lens check` guard. Small, and blocks launch.
2. §4.1 single reports without pages, plus the 14-day window. Small.
3. §4.2 page weight and `_headers`. Small.
4. §4.4 deploy config, and §4.1's archive Worker. Medium.
5. §5 server setup and a first deploy to `*.workers.dev`.
6. §7 checklist, then attach the domain.

Roughly two to three days of work, then about €6.50 a month to run.
