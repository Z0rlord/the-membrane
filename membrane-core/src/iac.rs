use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IacVerifyError {
    #[error("IAC missing signature")]
    MissingSignature,
    #[error("invalid signature hex: {0}")]
    InvalidSignatureHex(String),
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("canonical JSON error: {0}")]
    Canonical(#[from] crate::canonical::CanonicalError),
    #[error("invalid signer pubkey: {0}")]
    InvalidSignerPubkey(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAuthorizationCredential {
    pub version: String,
    pub scope_id: String,
    pub permitted_channels: Vec<String>,
    pub model_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stimulation_policy: Option<String>,
    pub context_merkle_bound: String,
    pub forbidden_exports: Vec<String>,
    pub valid_until: i64,
    pub parent_cp_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignableIac {
    pub version: String,
    pub scope_id: String,
    pub permitted_channels: Vec<String>,
    pub model_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stimulation_policy: Option<String>,
    pub context_merkle_bound: String,
    pub forbidden_exports: Vec<String>,
    pub valid_until: i64,
    pub parent_cp_hash: String,
}

impl IntentAuthorizationCredential {
    pub const SCHEMA_VERSION: &str = "0.9.14";

    pub fn signable_view(&self) -> SignableIac {
        SignableIac {
            version: self.version.clone(),
            scope_id: self.scope_id.clone(),
            permitted_channels: self.permitted_channels.clone(),
            model_allowlist: self.model_allowlist.clone(),
            decoder_version: self.decoder_version.clone(),
            stimulation_policy: self.stimulation_policy.clone(),
            context_merkle_bound: self.context_merkle_bound.clone(),
            forbidden_exports: self.forbidden_exports.clone(),
            valid_until: self.valid_until,
            parent_cp_hash: self.parent_cp_hash.clone(),
        }
    }

    pub fn hash_hex(&self) -> Result<String, crate::canonical::CanonicalError> {
        let bytes = crate::canonical::canonical_json_bytes(&self.signable_view())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    pub fn is_valid_at(&self, now: i64) -> bool {
        self.valid_until >= now
    }

    pub fn permits_channel(&self, channel: &str) -> bool {
        self.permitted_channels.iter().any(|c| c == channel)
    }

    pub fn model_allowed(&self, model_id: &str) -> bool {
        self.model_allowlist.iter().any(|m| m == model_id)
    }

    /// Sign the IAC with the subject's Nostr key (same digest binding as MembraneEvent).
    pub fn sign(&mut self, keys: &nostr::Keys) -> Result<(), IacVerifyError> {
        let bytes = crate::canonical::canonical_json_bytes(&self.signable_view())?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let message = nostr::secp256k1::Message::from_digest(digest);
        let sig = keys.sign_schnorr(&message);
        self.signature = Some(hex::encode(sig.serialize()));
        Ok(())
    }

    /// Verify Schnorr signature over canonical signable fields.
    pub fn verify_signature(&self, expected_signer_pubkey_hex: &str) -> Result<(), IacVerifyError> {
        let sig_hex = self
            .signature
            .as_ref()
            .ok_or(IacVerifyError::MissingSignature)?;
        let sig_bytes = hex::decode(sig_hex).map_err(|e| IacVerifyError::InvalidSignatureHex(e.to_string()))?;
        if sig_bytes.len() != 64 {
            return Err(IacVerifyError::InvalidSignatureHex(format!(
                "expected 64 bytes, got {}",
                sig_bytes.len()
            )));
        }

        let pubkey = nostr::PublicKey::from_hex(expected_signer_pubkey_hex)
            .map_err(|e| IacVerifyError::InvalidSignerPubkey(e.to_string()))?;

        let bytes = crate::canonical::canonical_json_bytes(&self.signable_view())?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let message = nostr::secp256k1::Message::from_digest(digest);
        let sig = nostr::secp256k1::schnorr::Signature::from_slice(&sig_bytes)
            .map_err(|e| IacVerifyError::InvalidSignatureHex(e.to_string()))?;

        let secp = nostr::secp256k1::Secp256k1::verification_only();
        secp.verify_schnorr(&sig, &message, &pubkey)
            .map_err(|_| IacVerifyError::InvalidSignature)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupBundle {
    pub version: String,
    pub subject_pubkey: String,
    pub period_start: i64,
    pub period_end: i64,
    pub cp_chain_root: String,
    pub last_bus_root: String,
    pub last_cp_hash: String,
}

impl RollupBundle {
    pub const SCHEMA_VERSION: &str = "0.9.14";
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    fn sample_iac() -> IntentAuthorizationCredential {
        IntentAuthorizationCredential {
            version: IntentAuthorizationCredential::SCHEMA_VERSION.to_string(),
            scope_id: "test-scope".into(),
            permitted_channels: vec!["local-llm".into()],
            model_allowlist: vec!["demo".into()],
            decoder_version: None,
            stimulation_policy: None,
            context_merkle_bound: "f".repeat(64),
            forbidden_exports: vec!["cloud-telemetry".into()],
            valid_until: 4_102_444_800,
            parent_cp_hash: "0".repeat(64),
            signature: None,
        }
    }

    #[test]
    fn iac_sign_and_verify_roundtrip() {
        let keys = Keys::generate();
        let mut iac = sample_iac();
        iac.sign(&keys).unwrap();
        iac.verify_signature(&keys.public_key().to_hex())
            .unwrap();
    }

    #[test]
    fn iac_rejects_tampered_payload() {
        let keys = Keys::generate();
        let mut iac = sample_iac();
        iac.sign(&keys).unwrap();
        iac.scope_id = "tampered".into();
        assert!(iac
            .verify_signature(&keys.public_key().to_hex())
            .is_err());
    }

    #[test]
    fn iac_rejects_missing_signature() {
        let iac = sample_iac();
        assert!(matches!(
            iac.verify_signature(&Keys::generate().public_key().to_hex()),
            Err(IacVerifyError::MissingSignature)
        ));
    }
}
