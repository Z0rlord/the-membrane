#!/usr/bin/env bash
# Deploy the public simulation-only dashboard to an existing container host.
# Usage: ./deploy/demo/deploy.sh <ssh-host>
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <ssh-host>" >&2
  exit 2
fi

HOST="$1"
REMOTE_DIR="${MEMBRANE_DEMO_REMOTE_DIR:-/opt/membrane/demo}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ssh -o BatchMode=yes "$HOST" "mkdir -p '$REMOTE_DIR/repo'"
cd "$REPO_ROOT"
rsync -az --delete \
  --relative \
  --exclude target \
  --exclude .git \
  --exclude .env \
  ./Cargo.toml \
  ./Cargo.lock \
  ./membrane-core \
  ./membrane-gate \
  ./membrane-cli \
  ./deploy/demo \
  "$HOST:$REMOTE_DIR/repo/"

ssh -o BatchMode=yes "$HOST" bash -s -- "$REMOTE_DIR" <<'REMOTE'
set -euo pipefail
REMOTE_DIR="$1"
cd "$REMOTE_DIR/repo"
docker compose -f deploy/demo/docker-compose.yml up -d --build --remove-orphans
docker compose -f deploy/demo/docker-compose.yml ps
for attempt in $(seq 1 20); do
  if curl --fail --silent http://127.0.0.1:8790/health >/dev/null; then
    exit 0
  fi
  sleep 1
done
echo "demo health check did not become ready" >&2
exit 1
REMOTE

echo "Simulation dashboard deployed and healthy."
