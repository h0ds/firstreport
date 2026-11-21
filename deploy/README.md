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

Nothing here has run against a real server or a Cloudflare account. The
Worker's logic is tested with stand-ins for R2 and the asset store; the
shell scripts are syntax-checked only. Expect to fix small things on
the first real run, in this order:

1. `wrangler deploy` by hand from a laptop, against `*.workers.dev`.
   This proves `wrangler.jsonc` and the Worker.
2. `bin/daily.sh` by hand, then request a story URL that is in the
   bucket but not in the site.
3. `setup.sh` on a fresh box.

## Before the first deploy

- Create the R2 bucket `firstreport-archive`, or change the name in
  `wrangler.jsonc`, `litestream.yml` and `env.example` together.
- Set the Worker's `name` and the bucket to whatever the project ends
  up being called.
- The site job needs `--base-url`; it refuses to run without
  `FIRSTREPORT_BASE_URL`.
