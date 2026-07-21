# Attestable landing site

Static public marketing page for **Attestable** — the fail-closed control point for AI agents with production access.

Copy source of truth: [`docs/attestable.md`](../docs/attestable.md).

This site is **marketing only**. It does not host the operator console, live demo controls, or the Rust demo dashboard (those stay local via `cargo run -p membrane-cli -- attestable`).

## Preview locally

```bash
# from repo root
python3 -m http.server 8080 --directory site
# open http://127.0.0.1:8080/
```

Or any static file server pointed at `site/`.

## Structure

```text
site/
  index.html      landing page
  styles.css      layout + motion
  app.js          gate canvas + scroll reveals
  assets/         favicon + OG image
  README.md       this file
```

## Live

| | |
|---|---|
| Custom domain | https://attestable.dojopop.live |
| Pages default | https://attestable-cti.pages.dev |
| CF Pages project | `attestable` |

## Deploy (Cloudflare Pages)

### Option A — Wrangler (already provisioned)

```bash
# from repo root; requires CLOUDFLARE_API_TOKEN + account access
doppler run --project dojopop --config prd_zorie -- bash -c '
  export CLOUDFLARE_API_TOKEN
  export CLOUDFLARE_ACCOUNT_ID=dfc6e38d5b254f0f8ffac8a0e554112a
  wrangler pages deploy site --project-name attestable --branch main
'
```

Custom domain `attestable.dojopop.live` is a proxied CNAME → `attestable-cti.pages.dev` on the `dojopop.live` zone.

### Option B — Git-connected Pages

1. Cloudflare Dashboard → Workers & Pages → Create → Pages → Connect to Git
2. Select `Z0rlord/the-membrane`
3. Build settings:
   - Framework preset: None
   - Build command: *(empty)*
   - Build output directory: `site`
4. Add custom domain `attestable.dojopop.live`

### Option C — Tunnel / existing CF ingress

If Pages is unavailable, serve `site/` from any static host behind the existing
`dojopop.live` Cloudflare tunnel and point `attestable.dojopop.live` at that origin.
See `deploy/grasp/update-tunnel-ingress.sh` for the tunnel pattern used elsewhere.

## What is intentionally not here

| Deferred | Why |
|----------|-----|
| Hosted interactive demo | Demo dashboard is local-only (`:8790`); keep operator controls off the marketing origin |
| Operator console | Production control surface is not a marketing concern |
| Nostr / BCI / ZK deep research pitch | Out of scope for Attestable product landing |
