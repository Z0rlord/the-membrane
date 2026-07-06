#!/usr/bin/env bash
# Deploy Membrane gate + llama.cpp on relay-2. Idempotent.
#
# Requires NOSTR_NSEC in remote /opt/membrane/gate/.env (chmod 600).
# Usage: ./deploy.sh [ssh-host]
set -euo pipefail

HOST="${1:-relay-2}"
REMOTE_DIR="/opt/membrane/gate"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MODEL_URL="https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf"
MODEL_FILE="qwen2.5-0.5b-instruct-q4_k_m.gguf"

echo "==> Deploying Membrane gate to ${HOST}:${REMOTE_DIR}"

ssh -o BatchMode=yes "$HOST" "mkdir -p '$REMOTE_DIR'"

rsync -az \
  "$SCRIPT_DIR/channel-registry.prod.yaml" \
  "$SCRIPT_DIR/docker-compose.yml" \
  "$SCRIPT_DIR/Dockerfile" \
  "$SCRIPT_DIR/iac.prod.json" \
  "$HOST:$REMOTE_DIR/"

rsync -az \
  "$REPO_ROOT/Cargo.toml" \
  "$REPO_ROOT/Cargo.lock" \
  "$REPO_ROOT/membrane-core" \
  "$REPO_ROOT/membrane-gate" \
  "$REPO_ROOT/membrane-cli" \
  "$HOST:$REMOTE_DIR/build-context/"

if [[ -f "$SCRIPT_DIR/.env" ]]; then
  scp -q "$SCRIPT_DIR/.env" "$HOST:$REMOTE_DIR/.env"
  ssh -o BatchMode=yes "$HOST" "chmod 600 '$REMOTE_DIR/.env'"
elif ! ssh -o BatchMode=yes "$HOST" "test -f '$REMOTE_DIR/.env'"; then
  echo "ERROR: missing $REMOTE_DIR/.env on host (NOSTR_NSEC=...)"
  echo "  doppler run --project dojopop --config prd_zorie -- bash -c 'printf \"NOSTR_NSEC=%s\\nMEMBRANE_RELAY_URL=wss://membrane-relay.dojopop.live\\n\" \"\$NOSTR_NSEC\" > deploy/gate/.env'"
  exit 1
fi

ssh -o BatchMode=yes "$HOST" bash -s <<REMOTE
set -euo pipefail
REMOTE_DIR='$REMOTE_DIR'
MODEL_URL='$MODEL_URL'
MODEL_FILE='$MODEL_FILE'

if ! docker compose version >/dev/null 2>&1; then
  apt-get update -qq && apt-get install -y -qq docker-compose-v2
fi

cd "\$REMOTE_DIR"

# Ensure GGUF model in docker volume
if ! docker run --rm -v membrane-llama-models:/models alpine \
  test -f "/models/\$MODEL_FILE" 2>/dev/null; then
  echo "==> Downloading \${MODEL_FILE} (~400MB) into llama-models volume..."
  docker run --rm --user root \
    -v membrane-llama-models:/models \
    curlimages/curl:8.12.1 -fL \
    "\$MODEL_URL" -o "/models/\$MODEL_FILE"
fi

# Build gate image from synced source
docker build -f Dockerfile -t membrane-gate:local build-context/

docker compose -p membrane-gate up -d --remove-orphans
docker compose -p membrane-gate ps

echo "==> Gate health (tailnet): curl http://${HOST}:8787/health"
REMOTE

echo "==> Done."
