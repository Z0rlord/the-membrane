use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    FeatureChunk = 0x00,
    BusEvent = 0x01,
    CpHash = 0x02,
    WitnessKey = 0x03,
}

impl Domain {
    pub fn prefix(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone)]
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
    root: Option<[u8; 32]>,
}

impl MerkleTree {
    pub fn from_domain_leaves(domain: Domain, raw_leaves: &[Vec<u8>]) -> Self {
        let leaves: Vec<[u8; 32]> = raw_leaves
            .iter()
            .map(|leaf| prefixed_hash(domain, leaf))
            .collect();
        let root = compute_root(&leaves);
        Self { leaves, root }
    }

    pub fn from_prefixed_leaves(mut leaves: Vec<[u8; 32]>) -> Self {
        leaves.sort();
        let root = compute_root(&leaves);
        Self { leaves, root }
    }

    pub fn root(&self) -> Option<[u8; 32]> {
        self.root
    }

    pub fn root_hex(&self) -> Option<String> {
        self.root.map(|r| hex::encode(r))
    }

    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }
}

pub fn prefixed_hash(domain: Domain, data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([domain.prefix()]);
    hasher.update(data);
    hasher.finalize().into()
}

fn compute_root(level: &[[u8; 32]]) -> Option<[u8; 32]> {
    if level.is_empty() {
        return None;
    }
    if level.len() == 1 {
        return Some(level[0]);
    }

    let mut current: Vec<[u8; 32]> = level.to_vec();
    while current.len() > 1 {
        let mut next = Vec::with_capacity(current.len().div_ceil(2));
        let mut i = 0;
        while i < current.len() {
            let left = current[i];
            let right = if i + 1 < current.len() {
                current[i + 1]
            } else {
                current[i]
            };
            next.push(parent_hash(&left, &right));
            i += 2;
        }
        current = next;
    }
    current.first().copied()
}

fn parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    if left <= right {
        hasher.update(left);
        hasher.update(right);
    } else {
        hasher.update(right);
        hasher.update(left);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_domain_prefix() {
        let leaf = prefixed_hash(Domain::BusEvent, b"event-a");
        let tree = MerkleTree::from_prefixed_leaves(vec![leaf]);
        assert!(tree.root_hex().is_some());
    }

    #[test]
    fn odd_leaf_duplicates_last() {
        let a = prefixed_hash(Domain::FeatureChunk, b"a");
        let b = prefixed_hash(Domain::FeatureChunk, b"b");
        let c = prefixed_hash(Domain::FeatureChunk, b"c");
        let root = MerkleTree::from_prefixed_leaves(vec![a, b, c]);
        let dup = MerkleTree::from_prefixed_leaves(vec![a, b, c, c]);
        assert_eq!(root.root(), dup.root());
    }

    #[test]
    fn leaves_sorted_lexicographically() {
        let hi = prefixed_hash(Domain::CpHash, b"z");
        let lo = prefixed_hash(Domain::CpHash, b"a");
        let unsorted = MerkleTree::from_prefixed_leaves(vec![hi, lo]);
        let sorted = MerkleTree::from_prefixed_leaves(vec![lo, hi]);
        assert_eq!(unsorted.root(), sorted.root());
    }
}
