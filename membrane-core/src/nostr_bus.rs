use crate::bus::bus_root_from_events;
use crate::canonical::canonical_json_bytes;
use crate::event::{EventType, MembraneEvent};
use crate::rollup::{is_cp_event, last_cp_hash_from_events};
use anyhow::{Context, Result, bail};
use nostr::event::builder::EventBuilder;
use nostr::secp256k1::Message;
use nostr::{Event, EventId, Filter, Keys, Kind, Tag, Timestamp, ToBech32};
use nostr_sdk::prelude::*;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tracing::{info, warn};

pub const KIND_MEMBRANE: u16 = 31990;
pub const KIND_ALERT: u16 = 31991;

pub fn membrane_kind_for(event_type: EventType) -> u16 {
    match event_type {
        EventType::AlertDegraded => KIND_ALERT,
        _ => KIND_MEMBRANE,
    }
}

#[derive(Debug, Clone)]
pub struct BusPublisherConfig {
    pub relay_url: String,
    pub keys: Keys,
}

pub struct BusPublisher {
    config: BusPublisherConfig,
}

impl BusPublisher {
    pub fn new(config: BusPublisherConfig) -> Self {
        Self { config }
    }

    pub fn keys(&self) -> &Keys {
        &self.config.keys
    }

    pub async fn publish(
        &self,
        event: &mut MembraneEvent,
        prev_event_id: Option<&str>,
    ) -> Result<EventId> {
        self.sign_membrane_event(event)?;
        let nostr_event = self.to_nostr_event(event, prev_event_id)?;
        let event_id = nostr_event.id;
        let client = self.connect().await?;
        client.send_event(nostr_event).await?;
        info!(
            event_id = %event_id.to_hex(),
            kind = membrane_kind_for(event.event_type),
            "published MembraneEvent to attestation bus"
        );
        Ok(event_id)
    }

    pub fn sign_membrane_event(&self, event: &mut MembraneEvent) -> Result<()> {
        let signable = event.signable_view();
        let bytes = canonical_json_bytes(&signable).context("canonical event bytes")?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let message = Message::from_digest(digest);
        let sig = self.config.keys.sign_schnorr(&message);
        event.signature = Some(hex::encode(sig.serialize()));
        event.subject_pubkey = self.config.keys.public_key().to_hex();
        Ok(())
    }

    pub fn to_nostr_event(
        &self,
        event: &MembraneEvent,
        prev_event_id: Option<&str>,
    ) -> Result<Event> {
        let kind = Kind::Custom(membrane_kind_for(event.event_type));
        let content = serde_json::to_string(event).context("serialize membrane event")?;

        let mut tags = vec![Tag::parse(vec![
            "k".to_string(),
            event.event_type.nostr_tag_suffix().to_string(),
        ])?];

        tags.push(Tag::public_key(self.config.keys.public_key()));

        if let Some(prev) = prev_event_id {
            tags.push(Tag::parse(vec!["e".to_string(), prev.to_string()])?);
        }

        let unsigned = EventBuilder::new(kind, content)
            .tags(tags)
            .custom_created_at(Timestamp::from(event.timestamp as u64))
            .sign_with_keys(&self.config.keys)?;

        Ok(unsigned)
    }

    async fn connect(&self) -> Result<Client> {
        let client = Client::new(self.config.keys.clone());
        client.add_relay(&self.config.relay_url).await?;
        client.connect().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(client)
    }
}

pub async fn fetch_membrane_events(
    relay_url: &str,
    since: Option<i64>,
    limit: usize,
) -> Result<Vec<MembraneEvent>> {
    Ok(fetch_membrane_bus_events(relay_url, since, limit)
        .await?
        .into_iter()
        .map(|e| e.event)
        .collect())
}

pub fn parse_membrane_event(event: &Event) -> Result<MembraneEvent> {
    let kind = event.kind.as_u16();
    if kind != KIND_MEMBRANE && kind != KIND_ALERT {
        bail!("unexpected kind {kind}");
    }
    let membrane: MembraneEvent =
        serde_json::from_str(&event.content).context("parse membrane event JSON")?;
    Ok(membrane)
}

pub async fn subscribe_and_compute_bus_root(
    relay_url: &str,
    since: Option<i64>,
) -> Result<(Vec<MembraneEvent>, Option<String>)> {
    let events = fetch_membrane_events(relay_url, since, 1_000).await?;
    let root = bus_root_from_events(&events)?;
    Ok((events, root))
}

pub fn keys_from_nsec(nsec: &str) -> Result<Keys> {
    Keys::parse(nsec).context("parse NOSTR_NSEC")
}

#[derive(Debug, Clone)]
pub struct MembraneBusEvent {
    pub id: EventId,
    pub event: MembraneEvent,
}

pub async fn fetch_membrane_bus_events(
    relay_url: &str,
    since: Option<i64>,
    limit: usize,
) -> Result<Vec<MembraneBusEvent>> {
    let client = Client::new(Keys::generate());
    client.add_relay(relay_url).await?;
    client.connect().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut filter = Filter::new()
        .kind(Kind::Custom(KIND_MEMBRANE))
        .kind(Kind::Custom(KIND_ALERT))
        .limit(limit);
    if let Some(s) = since {
        filter = filter.since(Timestamp::from(s as u64));
    }

    let events = client
        .fetch_events(vec![filter], Duration::from_secs(10))
        .await?;
    let mut parsed = Vec::new();

    for event in events {
        match parse_membrane_event(&event) {
            Ok(me) => parsed.push(MembraneBusEvent {
                id: event.id,
                event: me,
            }),
            Err(err) => warn!(event_id = %event.id.to_hex(), error = %err, "skip non-membrane event"),
        }
    }

    parsed.sort_by_key(|e| e.event.timestamp);
    Ok(parsed)
}

pub fn last_bus_event_id(
    bus_events: &[MembraneBusEvent],
    subject_pubkey: &str,
) -> Option<String> {
    bus_events
        .iter()
        .filter(|e| e.event.subject_pubkey == subject_pubkey)
        .filter(|e| is_cp_event(e.event.event_type))
        .last()
        .map(|e| e.id.to_hex())
}

pub async fn fetch_session_chain_bootstrap(
    relay_url: &str,
    subject_pubkey: &str,
) -> Result<(String, Option<String>, u64)> {
    let since = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64)
        - 86_400 * 7;
    let bus_events = fetch_membrane_bus_events(relay_url, Some(since), 5_000).await?;
    let membrane_events: Vec<_> = bus_events.iter().map(|e| e.event.clone()).collect();
    let last_cp_hash = last_cp_hash_from_events(&membrane_events, subject_pubkey);
    let last_event_id = last_bus_event_id(&bus_events, subject_pubkey);
    let session_nonce = crate::session::SessionChainState::from_bus_events(
        &membrane_events,
        subject_pubkey,
    )
    .session_nonce;
    Ok((last_cp_hash, last_event_id, session_nonce))
}

pub fn npub_from_keys(keys: &Keys) -> Result<String> {
    keys.public_key()
        .to_bech32()
        .context("encode npub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{MembranePayload, RouterSessionPayload};

    #[test]
    fn maps_router_kind() {
        assert_eq!(membrane_kind_for(EventType::CpRouter), KIND_MEMBRANE);
        assert_eq!(membrane_kind_for(EventType::AlertDegraded), KIND_ALERT);
    }

    #[test]
    fn builds_nostr_event() {
        let keys = Keys::generate();
        let publisher = BusPublisher::new(BusPublisherConfig {
            relay_url: "ws://localhost:7777".into(),
            keys: keys.clone(),
        });
        let mut event = MembraneEvent::new(
            EventType::CpRouter,
            "",
            "00".repeat(32),
            1_700_000_000,
            MembranePayload::Router(RouterSessionPayload {
                model_id: "demo".into(),
                context_merkle_root: "aa".repeat(32),
                session_nonce: 1,
                parent_cp_hash: "bb".repeat(32),
                iac_hash: "cc".repeat(32),
            }),
        );
        publisher.sign_membrane_event(&mut event).unwrap();
        let nostr = publisher.to_nostr_event(&event, None).unwrap();
        assert_eq!(nostr.kind, Kind::Custom(KIND_MEMBRANE));
    }
}
