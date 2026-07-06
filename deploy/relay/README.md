# Membrane attestation relay (production)

Dedicated [nostr-rs-relay](https://github.com/scsibug/nostr-rs-relay) for The Membrane
attestation bus — **kinds 31990 and 31991 only**. Separate from
[`relay.dojopop.live`](https://github.com/Z0rlord/nostr-pop/tree/main/relay).

| | |
|---|---|
| **Host** | `relay-2` (Hetzner Nuremberg vol1) |
| **Port** | `7778` (DojoPop relay uses `7777`) |
| **Tailnet** | `ws://relay-2:7778` |
| **Public** | `wss://membrane-relay.dojopop.live` |
| **Remote path** | `/opt/membrane/relay` |

## Deploy relay

```bash
chmod +x deploy.sh
./deploy.sh              # default: relay-2
./deploy.sh relay-2
```

## Expose wss (Cloudflare Tunnel)

The script uses `docker compose -p membrane` so it does not collide with DojoPop's
relay stack at `/opt/dojopop/relay` (compose project `relay`, port 7777).

```bash
# From dojopop Doppler project (Cloudflare API token):
cd deploy/relay
doppler run --project dojopop --config prd -- ./update-tunnel-ingress.sh
```

Or export `CLOUDFLARE_DNS_TOKEN` manually.

## Verify

```bash
curl -s -H 'Accept: application/nostr+json' https://membrane-relay.dojopop.live | python3 -m json.tool
curl -s -H 'Accept: application/nostr+json' http://relay-2:7778 | python3 -m json.tool
```

Publish test (whitelisted key only):

```bash
export MEMBRANE_RELAY_URL='wss://membrane-relay.dojopop.live'
export NOSTR_NSEC='nsec1...'
cd ../.. && cargo run -- bus publish-test
```

## Client config

```bash
export MEMBRANE_RELAY_URL='wss://membrane-relay.dojopop.live'
# tailnet-only:
export MEMBRANE_RELAY_URL='ws://relay-2:7778'
```

## Adding pubkeys

Edit `config.toml` `[authorization].pubkey_whitelist`, then rerun `./deploy.sh`.
