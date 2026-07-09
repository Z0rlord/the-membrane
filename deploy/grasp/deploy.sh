#!/usr/bin/env bash
# Deploy Membrane GRASP (ngit-grasp) on relay-2. Idempotent.
#
# Requires deploy/grasp/.env on host with NGIT_RELAY_OWNER_NSEC (chmod 600).
# Usage: ./deploy.sh [ssh-host]
set -euo pipefail

HOST="${1:-relay-2}"
REMOTE_DIR="/opt/membrane/grasp"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Deploying Membrane GRASP to ${HOST}:${REMOTE_DIR}"

ssh -o BatchMode=yes "$HOST" "mkdir -p '$REMOTE_DIR'"

rsync -az \
  "$SCRIPT_DIR/Dockerfile" \
  "$SCRIPT_DIR/docker-compose.yml" \
  "$SCRIPT_DIR/env.example" \
  "$HOST:$REMOTE_DIR/"

if [[ -f "$SCRIPT_DIR/.env" ]]; then
  scp -q "$SCRIPT_DIR/.env" "$HOST:$REMOTE_DIR/.env"
  ssh -o BatchMode=yes "$HOST" "chmod 600 '$REMOTE_DIR/.env'"
elif ! ssh -o BatchMode=yes "$HOST" "test -f '$REMOTE_DIR/.env'"; then
  echo "ERROR: missing $REMOTE_DIR/.env (NGIT_RELAY_OWNER_NSEC required)"
  echo "  doppler run --project dojopop --config prd_zorie -- bash -c '"
  echo "    printf \"NGIT_RELAY_OWNER_NSEC=%s\\n\" \"\$NGIT_NSEC\" > deploy/grasp/.env"
  echo "    cat deploy/grasp/env.example >> deploy/grasp/.env"
  echo "  '"
  exit 1
fi

ssh -o BatchMode=yes "$HOST" bash -s <<REMOTE
set -euo pipefail
REMOTE_DIR='$REMOTE_DIR'

if ! docker compose version >/dev/null 2>&1; then
  apt-get update -qq && apt-get install -y -qq docker-compose-v2
fi

cd "\$REMOTE_DIR"
docker compose -p membrane-grasp build
docker compose -p membrane-grasp up -d --remove-orphans
docker compose -p membrane-grasp ps

echo "==> NIP-11 (local):"
sleep 2
curl -sf -H 'Accept: application/nostr+json' http://127.0.0.1:7334/ | head -c 400 || true
echo
REMOTE

echo "==> Done."
echo "    Tailnet:  ws://${HOST}:7334  |  git http://${HOST}:7334"
echo "    Public:   wss://membrane-grasp.dojopop.live (after tunnel + DNS)"
