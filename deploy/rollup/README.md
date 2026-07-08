# Daily OTS rollup (relay-2)

Systemd timer runs `membrane rollup daily` at **00:15 UTC**:

1. Export yesterday's `RollupBundle` from the attestation bus
2. Sign with `NOSTR_NSEC`
3. Stamp via OpenTimestamps and publish `membrane.anchor.ots`

## Install

```bash
# 1. Ensure gate deploy installed /opt/membrane/bin/membrane
./deploy/gate/deploy.sh

# 2. Install timer (reuses deploy/gate/.env for secrets)
chmod +x deploy/rollup/install.sh
./deploy/rollup/install.sh
```

## Manual run

```bash
ssh relay-2 'systemctl start membrane-rollup.service'
ssh relay-2 'journalctl -u membrane-rollup.service -n 30 --no-pager'
```

Artifacts land in `/var/lib/membrane/rollup/rollup-YYYY-MM-DD.*`
