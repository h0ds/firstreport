#!/usr/bin/env bash
# Every 10 minutes: pull, rebuild the binaries if the code changed,
# build and check the site, deploy it. Pings the health check only on
# success, so a failure alerts by silence (docs/deploy.md 6).
set -euo pipefail

: "${KAGAMI_HOME:=/home/kagami/kagami}"
: "${KAGAMI_BASE_URL:?set KAGAMI_BASE_URL in /etc/kagami/env}"
: "${KAGAMI_PAGES_SINCE:=14}"
cd "$KAGAMI_HOME"
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

./target/release/kagami site \
  --base-url "$KAGAMI_BASE_URL" \
  --multi-outlet-pages \
  --pages-since "$KAGAMI_PAGES_SINCE"

(cd deploy && npx --yes wrangler deploy)

if [ -n "${HC_SITE_URL:-}" ]; then
  curl -fsS -m 10 --retry 3 -o /dev/null "$HC_SITE_URL"
fi
