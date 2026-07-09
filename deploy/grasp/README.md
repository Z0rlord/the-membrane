# Membrane GRASP backup (relay-2)

Self-hosted [GRASP](https://ngit.dev/grasp/) instance for **the-membrane** git backup on gitworkshop.dev / Nostr.

| | |
|---|---|
| Tailnet | `ws://relay-2:7334` · `http://relay-2:7334` |
| Public | `wss://membrane-grasp.dojopop.live` (after tunnel + DNS) |
| Remote path | `/opt/membrane/grasp` |
| Deploy | `./deploy/grasp/deploy.sh` |

## Identity keys (do not mix)

| Doppler secret | Role | Pubkey (hex) |
|---|---|---|
| `NOSTR_NSEC` | Membrane attestation bus, gate, rollup signer | `b3d8544ddd5896f75ef66c210f5c0d6ded9f7925163ebcbc89e678bdc1e48c6a` |
| `NGIT_NSEC` | ngit.dev / gitworkshop maintainer (`ngit init`, Grasp pushes) | `6b497b5b4e29623c6166c3f114cfb8344c01a536a45af84fdd4c5072d2083bd2` (`npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9`) |

- **Never** use `NGIT_NSEC` for membrane gate or relay deploys.
- **Never** commit nsec values; inject via Doppler or remote `.env` (chmod 600).

## Deploy grasp server

```bash
doppler run --project dojopop --config prd_zorie -- bash -c '
  printf "NGIT_RELAY_OWNER_NSEC=%s\n" "$NGIT_NSEC" > deploy/grasp/.env
  cat deploy/grasp/env.example >> deploy/grasp/.env
'
./deploy/grasp/deploy.sh relay-2
./deploy/grasp/update-tunnel-ingress.sh   # public HTTPS/WSS via Cloudflare
```

`NGIT_RELAY_OWNER_NSEC` on the server should be the **ngit maintainer** key (`NGIT_NSEC`), not the membrane operator.

`env.example` values with spaces must stay quoted (e.g. `NGIT_RELAY_NAME="Membrane GRASP"`).

## Publish / back up the-membrane

One-time init (already done for this repo):

```bash
cd /path/to/the-membrane
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

doppler run --project dojopop --config prd_zorie -- bash -c '
  ngit -n "$NGIT_NSEC" init \
    --identifier the-membrane \
    --grasp-server membrane-grasp.dojopop.live \
    --grasp-server gitworkshop.dev
  git remote rename origin grasp 2>/dev/null || true
  git remote add github https://github.com/Z0rlord/the-membrane.git 2>/dev/null || true
'
```

**Push backup** (after commits on `main`):

```bash
doppler run --project dojopop --config prd_zorie -- bash -c '
  export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
  ngit -n "$NGIT_NSEC" account login --defaults
  # Drop stale tracking refs from earlier failed pushes (important):
  git for-each-ref refs/remotes/origin --format="%(refname)" | while read ref; do
    git update-ref -d "$ref" 2>/dev/null || true
  done
  ngit -d -f -n "$NGIT_NSEC" repo edit -g membrane-grasp.dojopop.live --relay ws://relay-2:7334 --clean
  git push grasp main
  git push grasp cursor/v0.9.14-paun-biollm-lineage  # if that branch still exists locally
'
```

If public DNS for `membrane-grasp.dojopop.live` fails locally, push from relay-2 instead:

```bash
rsync -az --delete -e ssh ./ relay-2:/opt/membrane/grasp/repo-worktree/
ssh relay-2 'eval "$(grep ^NGIT_RELAY_OWNER_NSEC= /opt/membrane/grasp/.env)"; ...'  # see push block above
```

## Clone

| Method | URL |
|--------|-----|
| Nostr | `nostr://npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/membrane-grasp.dojopop.live/the-membrane` |
| HTTPS (public) | `https://membrane-grasp.dojopop.live/npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/the-membrane.git` |
| HTTP (tailnet) | `http://relay-2:7334/npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/the-membrane.git` |
| gitworkshop | [gitworkshop.dev/.../the-membrane](https://gitworkshop.dev/npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/membrane-grasp.dojopop.live/the-membrane) |

```bash
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
git clone nostr://npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/membrane-grasp.dojopop.live/the-membrane
```

## Local ngit identity

Use the **ngit.dev account** key only for `ngit` / Grasp — not for membrane gate or relay:

```bash
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
doppler run --project dojopop --config prd_zorie -- bash -c '
  ngit -n "$NGIT_NSEC" account login --defaults
'
```

## Verify

```bash
curl -s -H 'Accept: application/nostr+json' http://relay-2:7334/ | python3 -m json.tool
git clone http://relay-2:7334/npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/the-membrane.git /tmp/membrane-test
```

## Troubleshooting

- **State / push mismatch** (`refs/heads/main would be at fe2a32f7 but state declares ce297ee6`): delete `refs/remotes/origin/*` then `ngit repo edit --clean` and push again.
- **Authentication required**: run `ngit -n "$NGIT_NSEC" account login --defaults` before `git push`.
- **Wrong npub in remote**: repo owner must be `npub1ddyhkk...` (NGIT maintainer), not the membrane `NOSTR_NSEC` operator key.
- **Mac DNS**: if `membrane-grasp.dojopop.live` does not resolve, use tailnet `relay-2:7334` or push from the server.
