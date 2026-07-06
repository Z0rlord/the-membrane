#!/usr/bin/env bash
# Add membrane-relay.dojopop.live → localhost:7778 to the dojopop-relay Cloudflare Tunnel.
# Preserves existing ingress rules (GET → merge → PUT).
#
# Requires CLOUDFLARE_DNS_TOKEN (or CLOUDFLARE_API_TOKEN).
# Usage: doppler run -- ./update-tunnel-ingress.sh
set -euo pipefail

TOKEN="${CLOUDFLARE_DNS_TOKEN:-${CLOUDFLARE_API_TOKEN:-}}"
TUNNEL_ID="${DOJOPOP_TUNNEL_ID:-543b3cee-e3dd-422f-a619-7a34236a0ba0}"
ACCOUNT_ID="${CLOUDFLARE_ACCOUNT_ID:-dfc6e38d5b254f0f8ffac8a0e554112a}"
DOJOPOP_ZONE_ID="${DOJOPOP_ZONE_ID:-cf2b671698354bbaafb5c606945dbb2c}"
MEMBRANE_HOST="${MEMBRANE_RELAY_HOST:-membrane-relay.dojopop.live}"
MEMBRANE_PORT="${MEMBRANE_RELAY_PORT:-7778}"

if [[ -z "$TOKEN" ]]; then
  echo "ERROR: CLOUDFLARE_DNS_TOKEN or CLOUDFLARE_API_TOKEN required"
  exit 1
fi

echo "==> Fetching current tunnel ${TUNNEL_ID} configuration..."
curl -sS -H "Authorization: Bearer $TOKEN" \
  "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/cfd_tunnel/${TUNNEL_ID}/configurations" \
  -o /tmp/cf-tunnel-current.json

python3 - /tmp/cf-tunnel-current.json /tmp/cf-tunnel-merged.json "$MEMBRANE_HOST" "$MEMBRANE_PORT" <<'PY'
import json, sys
src, dst, host, port = sys.argv[1:5]
data = json.load(open(src))
cfg = data.get("result", {}).get("config", {})
ingress = cfg.get("ingress", [])
service = f"http://localhost:{port}"
filtered = [r for r in ingress if r.get("hostname") != host]
catch = [r for r in filtered if "hostname" not in r]
rest = [r for r in filtered if "hostname" in r]
rest.insert(0, {"hostname": host, "service": service})
if not catch:
    catch = [{"service": "http_status:404"}]
merged = {"config": {"ingress": rest + catch}}
json.dump(merged, open(dst, "w"))
print(f"merged ingress: {host} -> {service}")
PY

echo "==> Updating tunnel ingress..."
HTTP=$(curl -sS -o /tmp/cf-tunnel-resp.json -w "%{http_code}" \
  -X PUT \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/cfd_tunnel/${TUNNEL_ID}/configurations" \
  -d @/tmp/cf-tunnel-merged.json)

if [[ "$HTTP" != "200" ]]; then
  echo "ERROR: Cloudflare API returned HTTP $HTTP"
  python3 -m json.tool /tmp/cf-tunnel-resp.json || cat /tmp/cf-tunnel-resp.json
  exit 1
fi

echo "==> Tunnel ingress updated."

TARGET="${TUNNEL_ID}.cfargotunnel.com"
echo "==> DNS CNAME membrane-relay (zone ${DOJOPOP_ZONE_ID})..."
DNS_RESP=$(curl -sS -X POST "https://api.cloudflare.com/client/v4/zones/${DOJOPOP_ZONE_ID}/dns_records" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"type\":\"CNAME\",\"name\":\"membrane-relay\",\"content\":\"${TARGET}\",\"proxied\":true}")
python3 -c "
import json, sys
d = json.loads(sys.argv[1])
if d.get('success'):
    print('  membrane-relay: ok')
else:
    errors = d.get('errors') or []
    msg = errors[0].get('message', 'failed') if errors else 'failed'
    if 'already exists' in msg.lower():
        print('  membrane-relay: already exists (ok)')
    else:
        print('  membrane-relay:', msg)
        sys.exit(1)
" "$DNS_RESP"

echo ""
echo "Verify:"
echo "  curl -s -H 'Accept: application/nostr+json' https://${MEMBRANE_HOST} | python3 -m json.tool"
echo "  export MEMBRANE_RELAY_URL='wss://${MEMBRANE_HOST}'"
