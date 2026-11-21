#!/usr/bin/env bash
# First-boot setup for the box (docs/deploy.md 5). Run as root on
# Ubuntu 24.04; safe to run again. It installs and builds, and stops
# short of starting anything until /etc/kagami/env has been filled in.
#
#   sudo REPO_URL=https://github.com/h0ds/kagami.git bash setup.sh
set -euo pipefail

: "${REPO_URL:=https://github.com/h0ds/kagami.git}"
KAGAMI_USER=kagami
KAGAMI_HOME=/home/$KAGAMI_USER/kagami

if [ "$(id -u)" -ne 0 ]; then
  echo "run as root" >&2
  exit 1
fi

apt-get update -q
apt-get install -y -q build-essential pkg-config git curl ca-certificates rclone unattended-upgrades nodejs npm python3-fonttools python3-brotli

if ! command -v litestream >/dev/null; then
  echo "litestream is not installed: install the .deb for this machine from its releases page, then run this again" >&2
fi

id "$KAGAMI_USER" >/dev/null 2>&1 || useradd --create-home --shell /bin/bash "$KAGAMI_USER"

as_kagami() { sudo -u "$KAGAMI_USER" -H bash -lc "$1"; }

as_kagami 'command -v cargo >/dev/null || curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
[ -d "$KAGAMI_HOME/.git" ] || as_kagami "git clone '$REPO_URL' '$KAGAMI_HOME'"
as_kagami "cd '$KAGAMI_HOME' && cargo build --release --locked && git rev-parse HEAD > target/.built-rev"
as_kagami "cd '$KAGAMI_HOME/feed' && ../target/release/feed fetch-model"
ls "$KAGAMI_HOME"/design/fonts/*.woff2 >/dev/null 2>&1 || as_kagami "cd '$KAGAMI_HOME' && sh design/fonts/fetch.sh"

install -d -m 700 /etc/kagami
if [ ! -f /etc/kagami/env ]; then
  install -m 600 "$KAGAMI_HOME/deploy/env.example" /etc/kagami/env
  chown "$KAGAMI_USER" /etc/kagami /etc/kagami/env
  filled=no
else
  filled=yes
fi

install -m 644 "$KAGAMI_HOME"/deploy/systemd/*.service "$KAGAMI_HOME"/deploy/systemd/*.timer /etc/systemd/system/
systemctl daemon-reload

if [ "$filled" = no ]; then
  cat <<MSG

Installed and built. Nothing is running yet.
1. Fill in /etc/kagami/env.
2. Run this script again to start the services.
MSG
  exit 0
fi

systemctl enable --now kagami-feed.service kagami-site.timer kagami-daily.timer
command -v litestream >/dev/null && systemctl enable --now kagami-litestream.service
systemctl --no-pager --lines 0 status kagami-feed.service || true
