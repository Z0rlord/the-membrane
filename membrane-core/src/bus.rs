use crate::event::MembraneEvent;
use crate::merkle::{prefixed_hash, Domain, MerkleTree};

/// Recompute `bus_root` from canonical MembraneEvent digests (§5.1).
pub fn bus_root_from_events(events: &[MembraneEvent]) -> anyhow::Result<Option<String>> {
    let mut leaves = Vec::with_capacity(events.len());
    for event in events {
        let digest = event.digest()?;
        leaves.push(prefixed_hash(Domain::BusEvent, &digest));
    }
    Ok(MerkleTree::from_prefixed_leaves(leaves).root_hex())
}

pub fn bus_root_from_digests(digests: &[[u8; 32]]) -> Option<String> {
    let leaves: Vec<[u8; 32]> = digests
        .iter()
        .map(|d| prefixed_hash(Domain::BusEvent, d))
        .collect();
    MerkleTree::from_prefixed_leaves(leaves).root_hex()
}

#[derive(Debug, Clone)]
pub struct BusSubscriberConfig {
    pub relay_url: String,
    pub since: Option<i64>,
}

pub struct BusSubscriber {
    config: BusSubscriberConfig,
}

impl BusSubscriber {
    pub fn new(config: BusSubscriberConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &BusSubscriberConfig {
        &self.config
    }
}
