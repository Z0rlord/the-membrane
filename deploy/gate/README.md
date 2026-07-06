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
  -d '{"model":"qwen2.5-0.5b-instruct","messages":[{"role":"user","content":"membrane gate check"}]}'
```

Each chat request validates IAC, publishes `membrane.cp.router`, then proxies to llama.cpp.
