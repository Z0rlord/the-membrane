//! Router session chain state (RFA-lite) for §4.2.2.

use crate::event::{MembraneEvent, MembranePayload};
use crate::rollup::{cp_hash_hex, is_cp_event, GENESIS_CP_HASH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChainState {
    pub last_cp_hash: String,
    pub last_event_id: Option<String>,
    pub session_nonce: u64,
    pub active_scope_id: Option<String>,
}

impl SessionChainState {
    pub fn genesis() -> Self {
        Self {
            last_cp_hash: GENESIS_CP_HASH.to_string(),
            last_event_id: None,
            session_nonce: 0,
            active_scope_id: None,
        }
    }

    pub fn from_bus_events(events: &[MembraneEvent], subject_pubkey: &str) -> Self {
        let mut state = Self::genesis();
        let mut last_router_nonce = 0u64;

        for event in events {
            if event.subject_pubkey != subject_pubkey {
                continue;
            }
            if is_cp_event(event.event_type) {
                if let Ok(hash) = cp_hash_hex(event) {
                    state.last_cp_hash = hash;
                }
            }
            if let MembranePayload::Router(payload) = &event.payload {
                last_router_nonce = payload.session_nonce;
            }
        }

        state.session_nonce = last_router_nonce;
        state
    }

    pub fn begin_scope(&mut self, scope_id: &str) -> bool {
        if self.active_scope_id.as_deref() == Some(scope_id) {
            return false;
        }
        self.active_scope_id = Some(scope_id.to_string());
        self.session_nonce = 0;
        true
    }

    pub fn next_parent_cp_hash(&self, iac_parent_cp_hash: &str) -> String {
        if self.last_cp_hash == GENESIS_CP_HASH {
            iac_parent_cp_hash.to_string()
        } else {
            self.last_cp_hash.clone()
        }
    }

    pub fn next_session_nonce(&mut self) -> u64 {
        self.session_nonce += 1;
        self.session_nonce
    }

    pub fn record_cp(&mut self, cp_hash: String, bus_event_id: Option<String>) {
        self.last_cp_hash = cp_hash;
        self.last_event_id = bus_event_id;
    }

    pub fn validate_iac_anchor(&self, iac_parent_cp_hash: &str, new_scope: bool) -> Result<(), String> {
        if !new_scope {
            return Ok(());
        }
        if iac_parent_cp_hash == GENESIS_CP_HASH {
            if self.last_cp_hash != GENESIS_CP_HASH {
                return Err(
                    "session IAC must set parent_cp_hash to current chain head (use `iac issue`)"
                        .into(),
                );
            }
            return Ok(());
        }
        if iac_parent_cp_hash != self.last_cp_hash {
            return Err(format!(
                "IAC parent_cp_hash does not match chain head (expected {})",
                self.last_cp_hash
            ));
        }
        Ok(())
    }
}

impl Default for SessionChainState {
    fn default() -> Self {
        Self::genesis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventType, MembranePayload, RouterSessionPayload};

    fn router_event(ts: i64, nonce: u64, subject: &str) -> MembraneEvent {
        MembraneEvent::new(
            EventType::CpRouter,
            subject,
            GENESIS_CP_HASH,
            ts,
            MembranePayload::Router(RouterSessionPayload {
                model_id: "demo".into(),
                context_merkle_root: "bb".repeat(32),
                session_nonce: nonce,
                parent_cp_hash: GENESIS_CP_HASH.into(),
                iac_hash: "dd".repeat(32),
            }),
        )
    }

    #[test]
    fn chains_parent_cp_hash_after_first_turn() {
        let mut state = SessionChainState::genesis();
        assert_eq!(
            state.next_parent_cp_hash(GENESIS_CP_HASH),
            GENESIS_CP_HASH
        );

        state.record_cp("aa".repeat(32), Some("event1".into()));
        assert_eq!(state.next_parent_cp_hash(GENESIS_CP_HASH), "aa".repeat(32));
    }

    #[test]
    fn scope_change_resets_nonce() {
        let mut state = SessionChainState::genesis();
        state.session_nonce = 5;
        state.active_scope_id = Some("old".into());
        assert!(state.begin_scope("new"));
        assert_eq!(state.session_nonce, 0);
        assert!(!state.begin_scope("new"));
    }

    #[test]
    fn bootstrap_from_bus_events() {
        let subject = "aa".repeat(32);
        let events = vec![router_event(100, 1, &subject), router_event(101, 2, &subject)];
        let state = SessionChainState::from_bus_events(&events, &subject);
        assert_ne!(state.last_cp_hash, GENESIS_CP_HASH);
        assert_eq!(state.session_nonce, 2);
    }
}
