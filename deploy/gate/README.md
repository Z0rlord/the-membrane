# Membrane gate + llama.cpp (relay-2)

IAC-gated local LLM proxy on Hetzner Nuremberg (`relay-2`).

| Service | Port | Access |
|---------|------|--------|
| llama.cpp | `8080` | `127.0.0.1` on host (gate only) |
| membrane gate | `8787` | `http://relay-2:8787` (Tailscale) |

## Deploy

1. Create local `.env` (never commit):

```bash
doppler run --project dojopop --config prd_zorie -- bash -c '
  printf "NOSTR_NSEC=%s\nMEMBRANE_RELAY_URL=wss://membrane-relay.dojopop.live\n" "$NOSTR_NSEC" \
    > deploy/gate/.env
'
```

2. Deploy:

```bash
chmod +x deploy/gate/deploy.sh
./deploy/gate/deploy.sh
```

First run downloads **Qwen2.5-0.5B-Instruct Q4** (~400MB) and builds the gate image on the server.

## Verify

```bash
curl -s http://relay-2:8787/health | python3 -m json.tool
curl -s http://relay-2:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H "X-Membrane-IAC: $(python3 -c 'import json; print(json.dumps(json.load(open("/tmp/session-iac.json"))))')" \
  -d '{"model":"qwen2.5-0.5b-instruct","messages":[{"role":"user","content":"membrane gate check"}]}'
```

Use compact JSON (single line) or base64 in `X-Membrane-IAC` — pretty-printed JSON breaks HTTP headers.

On relay-2 host, set `MEMBRANE_RELAY_URL=ws://127.0.0.1:7778` for `membrane iac issue` / rollup (the gate container uses `ws://host.docker.internal:7778`).

Each chat request validates session IAC, publishes `membrane.cp.router` (chained via `parent_cp_hash` + Nostr `e` tags), then proxies to llama.cpp.

For production traffic after the chain has left genesis, issue a fresh session IAC:

```bash
ssh relay-2 'doppler run --project dojopop --config prd_zorie -- \
  /opt/membrane/bin/membrane iac issue \
    --model qwen2.5-0.5b-instruct \
    --ttl-secs 3600 \
    --out /tmp/session-iac.json'
```
