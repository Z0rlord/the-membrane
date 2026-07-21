pub mod bus;
pub mod canonical;
pub mod event;
pub mod iac;
pub mod merkle;
pub mod nostr_bus;
pub mod ots;
pub mod rollup;
pub mod session;

pub use bus::bus_root_from_events;
pub use bus::{BusSubscriber, BusSubscriberConfig};
pub use event::{EventType, MembraneEvent, MembranePayload, RouterSessionPayload, SCHEMA_VERSION};
pub use iac::{IacVerifyError, IntentAuthorizationCredential, RollupBundle};
pub use merkle::{Domain, MerkleTree};
pub use nostr_bus::{
    fetch_membrane_bus_events, fetch_membrane_events, fetch_session_chain_bootstrap,
    keys_from_nsec, last_bus_event_id, membrane_kind_for, npub_from_keys,
    subscribe_and_compute_bus_root, BusPublisher, BusPublisherConfig, MembraneBusEvent,
};
pub use ots::{HttpOtsStamper, MockOtsStamper, OtsError, OtsStampResult, OtsStamper};
pub use rollup::{
    build_rollup_bundle, cp_chain_root_from_events, cp_hash_hex, day_bounds_utc,
    filter_events_in_period, is_cp_event, last_cp_hash_from_events, validate_rollup_bundle,
    SignedRollupBundle, GENESIS_CP_HASH,
};
pub use session::{
    alert_degraded_fields, alert_degraded_payload, SessionChainState,
    ALERT_REASON_DELTA_T_EXCEEDED, ALERT_REASON_SUBJECT_SEVER,
};
