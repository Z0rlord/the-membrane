#!/usr/bin/env bash
# Deploy Membrane attestation relay to relay-2 (Hetzner Nuremberg). Idempotent.
#
# Usage: ./deploy.sh [ssh-host]   (default: relay-2)
set -euo pipefail

HOST="${1:-relay-2}"
REMOTE_DIR="/opt/membrane/relay"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Deploying Membrane relay to ${HOST}:${REMOTE_DIR}"

ssh -o BatchMode=yes "$HOST" "mkdir -p '$REMOTE_DIR'"

rsync -az --delete \
  --include='docker-compose.yml' \
  --include='config.toml' \
  --exclude='*' \
  "$SCRIPT_DIR/" "$HOST:$REMOTE_DIR/"

ssh -o BatchMode=yes "$HOST" "
  set -euo pipefail
  if ! docker compose version >/dev/null 2>&1; then
    echo '==> docker compose plugin missing; installing (apt)'
    apt-get update -qq && apt-get install -y -qq docker-compose-v2
  fi
  cd '$REMOTE_DIR'
  docker compose -p membrane up -d --pull always
  docker compose -p membrane ps
"

echo "==> Done. Membrane relay listening on ${HOST}:7778"
echo "    Tailnet: ws://${HOST}:7778"
echo "    Public:  wss://membrane-relay.dojopop.live (after tunnel + DNS)"
