#!/usr/bin/env bash
# Install Membrane daily rollup timer on relay-2.
#
# Prerequisite: /opt/membrane/bin/membrane (from deploy/gate/deploy.sh)
# Usage: ./install.sh [ssh-host]
set -euo pipefail

HOST="${1:-relay-2}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REMOTE_DIR="/opt/membrane/rollup"

echo "==> Installing rollup timer on ${HOST}"

ssh -o BatchMode=yes "$HOST" "mkdir -p '$REMOTE_DIR' /var/lib/membrane/rollup /opt/membrane/bin"

rsync -az \
  "$SCRIPT_DIR/membrane-rollup.service" \
  "$SCRIPT_DIR/membrane-rollup.timer" \
  "$HOST:$REMOTE_DIR/"

if [[ -f "$SCRIPT_DIR/../gate/.env" ]]; then
  scp -q "$SCRIPT_DIR/../gate/.env" "$HOST:$REMOTE_DIR/.env"
  ssh -o BatchMode=yes "$HOST" "chmod 600 '$REMOTE_DIR/.env'"
elif ! ssh -o BatchMode=yes "$HOST" "test -f '$REMOTE_DIR/.env'"; then
  echo "ERROR: missing $REMOTE_DIR/.env (copy from deploy/gate/.env)"
  exit 1
fi

ssh -o BatchMode=yes "$HOST" bash -s <<REMOTE
set -euo pipefail
# Use local attestation bus from host (systemd runs outside Docker)
if grep -q MEMBRANE_RELAY_URL "$REMOTE_DIR/.env"; then
  sed -i 's|^MEMBRANE_RELAY_URL=.*|MEMBRANE_RELAY_URL=ws://127.0.0.1:7778|' "$REMOTE_DIR/.env"
else
  echo 'MEMBRANE_RELAY_URL=ws://127.0.0.1:7778' >> "$REMOTE_DIR/.env"
fi

cp "$REMOTE_DIR/membrane-rollup.service" /etc/systemd/system/
cp "$REMOTE_DIR/membrane-rollup.timer" /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now membrane-rollup.timer
systemctl status membrane-rollup.timer --no-pager
REMOTE

echo "==> Rollup timer enabled (00:15 UTC daily)"
echo "    Manual run: ssh $HOST 'systemctl start membrane-rollup.service'"
