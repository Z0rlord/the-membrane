//! Δt liveness watchdog — publishes `membrane.alert.degraded` on stale router CPs.

use std::sync::Arc;

use membrane_core::{ALERT_REASON_DELTA_T_EXCEEDED, SessionChainState};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{Gate, GateError};

const WATCHDOG_INTERVAL_SECS: u64 = 30;

pub fn spawn_delta_t_watchdog(
    gate: Arc<Gate>,
    session_chain: Arc<Mutex<SessionChainState>>,
) {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(WATCHDOG_INTERVAL_SECS);
        loop {
            tokio::time::sleep(interval).await;
            if let Err(err) = tick(&gate, &session_chain).await {
                warn!(error = %err, "Δt watchdog tick failed");
            }
        }
    });
}

async fn tick(
    gate: &Gate,
    session_chain: &Arc<Mutex<SessionChainState>>,
) -> Result<(), GateError> {
    let now = now_secs();
    let mut chain = session_chain.lock().await;
    let delta_t = gate.registry().delta_t_secs;

    let Some(scope_id) = chain.active_scope_id.clone() else {
        return Ok(());
    };

    if chain.is_scope_degraded(&scope_id) {
        return Ok(());
    }

    if !chain.is_router_stale(now, delta_t) {
        return Ok(());
    }

    let age = chain.last_router_cp_age_secs(now).unwrap_or(delta_t as i64);
    let prev_event_id = chain.last_event_id.clone();
    let last_cp_hash = chain.last_cp_hash.clone();

    gate
        .publish_alert_degraded(
            &scope_id,
            ALERT_REASON_DELTA_T_EXCEEDED,
            now,
            &last_cp_hash,
            Some(age),
            prev_event_id.as_deref(),
        )
        .await?;

    chain.mark_degraded(&scope_id, ALERT_REASON_DELTA_T_EXCEEDED, now);
    info!(
        scope_id = %scope_id,
        last_cp_age_secs = age,
        delta_t_secs = delta_t,
        "membrane.alert.degraded published (Δt exceeded)"
    );
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}
