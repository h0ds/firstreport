#!/usr/bin/env bash
# Once a day: copy the live story pages to the archive bucket, then
# prune feed's store (docs/deploy.md 4.1 and 6).
#
# The archive is a daily copy of every story page the site currently
# has. A story drops out of the build window only after days without
# activity, so the last copy made is its final page; nothing has to
# work out which stories are "leaving".
set -euo pipefail

: "${FIRSTREPORT_HOME:=/home/firstreport/firstreport}"
: "${FIRSTREPORT_ARCHIVE:=r2:firstreport-archive}"
cd "$FIRSTREPORT_HOME"

if [ -d site/story ]; then
  # --checksum: every build rewrites the files, so times always differ;
  # only pages whose content changed are uploaded.
  rclone copy --checksum --transfers 16 site/story "$FIRSTREPORT_ARCHIVE/story"
fi

(cd feed && ../target/release/feed prune --embeddings)

if [ -n "${HC_DAILY_URL:-}" ]; then
  curl -fsS -m 10 --retry 3 -o /dev/null "$HC_DAILY_URL"
fi
