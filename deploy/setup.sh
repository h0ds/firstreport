#!/usr/bin/env bash
# First-boot setup for the box (docs/deploy.md 5). Run as root on
# Ubuntu 24.04; safe to run again. It installs and builds, and stops
# short of starting anything until /etc/firstreport/env has been filled in.
#
#   sudo REPO_URL=https://github.com/h0ds/firstreport.git bash setup.sh
set -euo pipefail

: "${REPO_URL:=https://github.com/h0ds/firstreport.git}"
FIRSTREPORT_USER=firstreport
FIRSTREPORT_HOME=/home/$FIRSTREPORT_USER/firstreport

if [ "$(id -u)" -ne 0 ]; then
  echo "run as root" >&2
  exit 1
fi

apt-get update -q
apt-get install -y -q build-essential pkg-config git curl ca-certificates rclone unattended-upgrades nodejs npm python3-fonttools python3-brotli

if ! command -v litestream >/dev/null; then
  echo "litestream is not installed: install the .deb for this machine from its releases page, then run this again" >&2
fi

id "$FIRSTREPORT_USER" >/dev/null 2>&1 || useradd --create-home --shell /bin/bash "$FIRSTREPORT_USER"

as_firstreport() { sudo -u "$FIRSTREPORT_USER" -H bash -lc "$1"; }

as_firstreport 'command -v cargo >/dev/null || curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
[ -d "$FIRSTREPORT_HOME/.git" ] || as_firstreport "git clone '$REPO_URL' '$FIRSTREPORT_HOME'"
as_firstreport "cd '$FIRSTREPORT_HOME' && cargo build --release --locked && git rev-parse HEAD > target/.built-rev"
as_firstreport "cd '$FIRSTREPORT_HOME/feed' && ../target/release/feed fetch-model"
# Outlet icons for the site; without them every badge is a letter tile.
as_firstreport "cd '$FIRSTREPORT_HOME/feed' && ../target/release/feed icons"
ls "$FIRSTREPORT_HOME"/design/fonts/*.woff2 >/dev/null 2>&1 || as_firstreport "cd '$FIRSTREPORT_HOME' && sh design/fonts/fetch.sh"

install -d -m 700 /etc/firstreport
if [ ! -f /etc/firstreport/env ]; then
  install -m 600 "$FIRSTREPORT_HOME/deploy/env.example" /etc/firstreport/env
  chown "$FIRSTREPORT_USER" /etc/firstreport /etc/firstreport/env
  filled=no
else
  filled=yes
fi

install -m 644 "$FIRSTREPORT_HOME"/deploy/systemd/*.service "$FIRSTREPORT_HOME"/deploy/systemd/*.timer /etc/systemd/system/
systemctl daemon-reload

if [ "$filled" = no ]; then
  cat <<MSG

Installed and built. Nothing is running yet.
1. Fill in /etc/firstreport/env.
2. Run this script again to start the services.
MSG
  exit 0
fi

systemctl enable --now firstreport-feed.service firstreport-site.timer firstreport-daily.timer
command -v litestream >/dev/null && systemctl enable --now firstreport-litestream.service
systemctl --no-pager --lines 0 status firstreport-feed.service || true
