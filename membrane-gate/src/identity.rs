//! Production-only identity enforcement. Demo credentials never bypass this path.
use crate::{ChannelRegistry, Gate, GateError};
use membrane_core::{caller::CallerProof, iac::IntentAuthorizationCredential};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityGrant {
    #[serde(default)]
    pub revoked: bool,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub permitted_channels: Vec<String>,
    #[serde(default)]
    pub model_allowlist: Vec<String>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub github_repo_allowlist: Vec<String>,
}
impl Gate {
    /// Public, per-process challenge binds a proof to this gate lifetime.
    pub fn caller_audience(&self) -> String {
        format!("{}:{}", self.publisher_pubkey_hex(), self.caller_challenge)
    }

    pub fn authenticate_caller<T: Serialize>(
        &self,
        iac: &IntentAuthorizationCredential,
        proof: &CallerProof,
        path: &str,
        body: &T,
        now: i64,
    ) -> Result<String, GateError> {
        // Verify operator authority before trusting the bound caller key.
        self.validate_iac(Some(iac), now)?;
        if proof.timestamp > now
            || now.saturating_sub(proof.timestamp) > 60
            || proof.nonce.len() != 64
            || !proof.nonce.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(GateError::IdentityAuthentication(
                "expired, future or malformed proof".into(),
            ));
        }
        let identity = proof
            .verify(&self.caller_audience(), path, body, iac)
            .map_err(|e| GateError::IdentityAuthentication(e.to_string()))?;
        let mut replay = self
            .replay
            .lock()
            .map_err(|_| GateError::IdentityAuthentication("replay cache unavailable".into()))?;
        replay.retain(|_, issued| now.saturating_sub(*issued) <= 60);
        let key = format!("{identity}:{}", proof.nonce.to_ascii_lowercase());
        if replay.contains_key(&key) || replay.len() >= 10_000 {
            return Err(GateError::IdentityAuthentication(
                "replay or replay capacity exceeded".into(),
            ));
        }
        replay.insert(key, proof.timestamp);
        Ok(identity)
    }
    pub fn authorize_identity(
        &self,
        identity: &str,
        iac: &IntentAuthorizationCredential,
        model: &str,
        tool_repo: Option<(&str, &str)>,
    ) -> Result<(), GateError> {
        let live;
        let registry = if let Some(path) = &self.identity_registry_path {
            live = ChannelRegistry::load(path)
                .map_err(|e| GateError::IdentityGrant(format!("registry unavailable: {e}")))?;
            &live
        } else {
            &self.registry
        };
        let grant = registry
            .identities
            .get(identity)
            .ok_or_else(|| GateError::IdentityGrant("unknown identity".into()))?;
        if grant.revoked {
            return Err(GateError::IdentityGrant("revoked identity".into()));
        }
        if !grant.scopes.contains(&iac.scope_id)
            || iac
                .permitted_channels
                .iter()
                .any(|c| !grant.permitted_channels.contains(c))
        {
            return Err(GateError::IdentityGrant(
                "scope or channel not granted".into(),
            ));
        }
        if !iac.model_allowed(model)
            || !registry.model_allowlist.iter().any(|m| m == model)
            || !grant.model_allowlist.iter().any(|m| m == model)
        {
            return Err(GateError::ModelDenied(model.into()));
        }
        if let Some((tool, repo)) = tool_repo {
            if !iac.tool_allowed(tool) || !grant.tool_allowlist.iter().any(|t| t == tool) {
                return Err(GateError::ToolDenied(tool.into()));
            }
            if !registry.github_repo_allowlist.iter().any(|r| r == repo)
                || !grant.github_repo_allowlist.iter().any(|r| r == repo)
            {
                return Err(GateError::RepoDenied(repo.into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_core::{BusPublisher, BusPublisherConfig};
    use nostr::Keys;
    fn fixture() -> (Gate, Keys, IntentAuthorizationCredential) {
        let operator = Keys::generate();
        let caller = Keys::generate();
        let mut iac = IntentAuthorizationCredential::new_session_with_tools(
            "s",
            "m",
            "0".repeat(64),
            500,
            vec!["local-llm".into()],
            vec![],
            vec!["github.comment".into()],
        );
        iac.caller_pubkey = Some(caller.public_key().to_hex());
        iac.sign(&operator).unwrap();
        let grant = IdentityGrant {
            revoked: false,
            scopes: vec!["s".into()],
            permitted_channels: vec!["local-llm".into()],
            model_allowlist: vec!["m".into()],
            tool_allowlist: vec!["github.comment".into()],
            github_repo_allowlist: vec!["a/b".into()],
        };
        let registry = ChannelRegistry {
            identities: [(caller.public_key().to_hex(), grant)].into(),
            permitted_channels: vec!["local-llm".into()],
            forbidden_exports: vec![],
            model_allowlist: vec!["m".into()],
            delta_t_secs: 300,
            model_api_url: None,
            github_repo_allowlist: vec!["a/b".into()],
        };
        (
            Gate::new(
                registry,
                BusPublisher::new(BusPublisherConfig {
                    relay_url: "memory://identity".into(),
                    keys: operator,
                }),
            ),
            caller,
            iac,
        )
    }
    fn proof(gate: &Gate, caller: &Keys, iac: &IntentAuthorizationCredential) -> CallerProof {
        CallerProof::sign(
            caller,
            &gate.caller_audience(),
            "/v1/tools/invoke",
            &serde_json::json!({"tool":"github.comment"}),
            iac,
            100,
            Keys::generate().public_key().to_hex(),
        )
        .unwrap()
    }
    fn auth(
        g: &Gate,
        i: &IntentAuthorizationCredential,
        p: &CallerProof,
        now: i64,
    ) -> Result<String, GateError> {
        g.authenticate_caller(
            i,
            p,
            "/v1/tools/invoke",
            &serde_json::json!({"tool":"github.comment"}),
            now,
        )
    }
    #[test]
    fn valid_distinct_caller_and_operator_then_replay_denied() {
        let (g, c, i) = fixture();
        let p = proof(&g, &c, &i);
        let id = auth(&g, &i, &p, 100).unwrap();
        assert_ne!(id, g.publisher_pubkey_hex());
        g.authorize_identity(&id, &i, "m", Some(("github.comment", "a/b")))
            .unwrap();
        assert!(auth(&g, &i, &p, 100).is_err());
    }
    #[test]
    fn wrong_key_tampered_iac_body_path_and_audience_denied() {
        let (g, c, i) = fixture();
        let body = serde_json::json!({"tool":"github.comment"});
        assert!(auth(&g, &i, &proof(&g, &Keys::generate(), &i), 100).is_err());
        let mut forged = i.clone();
        forged.caller_pubkey = Some(Keys::generate().public_key().to_hex());
        assert!(auth(&g, &forged, &proof(&g, &c, &i), 100).is_err());
        let p = proof(&g, &c, &i);
        assert!(g
            .authenticate_caller(&i, &p, "/v1/chat/completions", &body, 100)
            .is_err());
        assert!(g
            .authenticate_caller(
                &i,
                &p,
                "/v1/tools/invoke",
                &serde_json::json!({"tool":"github.merge"}),
                100
            )
            .is_err());
        let foreign = CallerProof::sign(
            &c,
            "wrong-gate",
            "/v1/tools/invoke",
            &body,
            &i,
            100,
            "a".repeat(64),
        )
        .unwrap();
        assert!(auth(&g, &i, &foreign, 100).is_err());
    }
    #[test]
    fn stale_future_unbound_and_cache_failure_denied() {
        let (g, c, mut i) = fixture();
        let p = proof(&g, &c, &i);
        assert!(auth(&g, &i, &p, 161).is_err());
        assert!(auth(&g, &i, &p, 99).is_err());
        i.caller_pubkey = None;
        i.sign(g.publisher().keys()).unwrap();
        assert!(auth(&g, &i, &p, 100).is_err());
    }
    #[test]
    fn unknown_revoked_and_intersection_denied() {
        let (mut g, c, i) = fixture();
        let id = c.public_key().to_hex();
        assert!(g
            .authorize_identity(&Keys::generate().public_key().to_hex(), &i, "m", None)
            .is_err());
        assert!(g.authorize_identity(&id, &i, "other", None).is_err());
        assert!(g
            .authorize_identity(&id, &i, "m", Some(("github.merge", "a/b")))
            .is_err());
        assert!(g
            .authorize_identity(&id, &i, "m", Some(("github.comment", "a/c")))
            .is_err());
        let mut other_scope = i.clone();
        other_scope.scope_id = "other".into();
        assert!(g.authorize_identity(&id, &other_scope, "m", None).is_err());
        g.registry.identities.get_mut(&id).unwrap().revoked = true;
        assert!(g.authorize_identity(&id, &i, "m", None).is_err());
    }
    #[test]
    fn live_registry_revocation_and_broken_registry_fail_closed() {
        let (g, c, i) = fixture();
        let id = c.public_key().to_hex();
        let path = std::env::temp_dir().join(format!("membrane-{}.yaml", id));
        std::fs::write(&path, serde_yaml::to_string(g.registry()).unwrap()).unwrap();
        let g = g.with_identity_registry_path(path.clone());
        g.authorize_identity(&id, &i, "m", None).unwrap();
        let mut r = g.registry().clone();
        r.identities.get_mut(&id).unwrap().revoked = true;
        std::fs::write(&path, serde_yaml::to_string(&r).unwrap()).unwrap();
        assert!(g.authorize_identity(&id, &i, "m", None).is_err());
        std::fs::write(&path, "not: [valid").unwrap();
        assert!(g.authorize_identity(&id, &i, "m", None).is_err());
        std::fs::remove_file(path).unwrap();
        assert!(g.authorize_identity(&id, &i, "m", None).is_err());
    }
    #[test]
    fn old_gate_challenge_cannot_replay_after_restart() {
        let (g, c, i) = fixture();
        let p = proof(&g, &c, &i);
        let restarted = Gate::new(
            g.registry().clone(),
            BusPublisher::new(BusPublisherConfig {
                relay_url: "memory://restart".into(),
                keys: g.publisher().keys().clone(),
            }),
        );
        assert_ne!(g.caller_audience(), restarted.caller_audience());
        assert!(auth(&restarted, &i, &p, 100).is_err());
    }
    #[test]
    fn replay_capacity_and_poisoned_lock_fail_closed() {
        let (g, c, i) = fixture();
        g.replay
            .lock()
            .unwrap()
            .extend((0..10_000).map(|n| (format!("occupied-{n}"), 100)));
        assert!(auth(&g, &i, &proof(&g, &c, &i), 100).is_err());
        let g = std::sync::Arc::new(g);
        let other = g.clone();
        let _ = std::thread::spawn(move || {
            let _lock = other.replay.lock().unwrap();
            panic!("poison replay cache");
        })
        .join();
        assert!(auth(&g, &i, &proof(&g, &c, &i), 100).is_err());
    }
    #[test]
    fn simultaneous_replay_has_exactly_one_winner() {
        let (g, c, i) = fixture();
        let p = proof(&g, &c, &i);
        let g = std::sync::Arc::new(g);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                let g = g.clone();
                let i = i.clone();
                let p = p.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    auth(&g, &i, &p, 100).is_ok()
                })
            })
            .collect();
        assert_eq!(
            jobs.into_iter()
                .filter(|j| j.thread().id() != std::thread::current().id())
                .map(|j| j.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
    }
}
