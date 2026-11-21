#!/usr/bin/env bash
# Every 10 minutes: pull, rebuild the binaries if the code changed,
# build and check the site, deploy it. Pings the health check only on
# success, so a failure alerts by silence (docs/deploy.md 6).
set -euo pipefail

: "${FIRSTREPORT_HOME:=/home/firstreport/firstreport}"
: "${FIRSTREPORT_BASE_URL:?set FIRSTREPORT_BASE_URL in /etc/firstreport/env}"
: "${FIRSTREPORT_PAGES_SINCE:=14}"
cd "$FIRSTREPORT_HOME"
export PATH="$HOME/.cargo/bin:$PATH"

git pull --ff-only --quiet

# Rebuild only when something that compiles changed since the last build.
built_file=target/.built-rev
built=$(cat "$built_file" 2>/dev/null || true)
head=$(git rev-parse HEAD)
if [ -z "$built" ] || ! git diff --quiet "$built" "$head" -- Cargo.toml Cargo.lock '*/Cargo.toml' '*/src' design/tokens.css; then
  cargo build --release --locked
  echo "$head" > "$built_file"
fi

./target/release/firstreport site \
  --base-url "$FIRSTREPORT_BASE_URL" \
  --multi-outlet-pages \
  --pages-since "$FIRSTREPORT_PAGES_SINCE"

(cd deploy && npx --yes wrangler deploy)

if [ -n "${HC_SITE_URL:-}" ]; then
  curl -fsS -m 10 --retry 3 -o /dev/null "$HC_SITE_URL"
fi
