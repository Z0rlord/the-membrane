use super::*;
use membrane_gate::audit::PolicyView;

const REGISTRY: &str = "# registry\npermitted_channels:\n  - local-llm\n\nforbidden_exports:\n  - cloud-telemetry\n\nmodel_allowlist:\n  - sha256:demo-model # keep\n\ndelta_t_secs: 300\n\n# repos\ngithub_repo_allowlist: []\n";

fn deny(seq: u64, rule: &str, subject: Option<&str>) -> Decision {
    Decision {
        sequence: seq,
        timestamp: 1_800_000_000,
        outcome: "deny".into(),
        rule: rule.into(),
        agent: "gate".into(),
        authenticated_identity: None,
        scope: None,
        action: "tool".into(),
        subject: subject.map(Into::into),
    }
}

fn snap(decisions: Vec<Decision>) -> Snapshot {
    let denied = decisions.len();
    Snapshot {
        schema_version: 1,
        observed_at: 1_800_000_000,
        status: "live".into(),
        last_cp_age_secs: None,
        router_stale: false,
        degraded: false,
        policy: PolicyView {
            permitted_channels: vec![],
            forbidden_exports: vec![],
            model_allowlist: vec![],
            github_repo_allowlist: vec![],
            delta_t_secs: 300,
        },
        retained: denied,
        decisions,
        total_observed: denied as u64,
        allowed: 0,
        denied,
        deny_rate: Some(1.0),
        audit_available: true,
    }
}

fn run(decisions: Vec<Decision>) -> Report {
    analyze(&snap(decisions), REGISTRY, "tools/channel-registry.yaml", 1).unwrap()
}

#[test]
fn repo_denial_becomes_exact_entry_patch() {
    let r = run(vec![
        deny(1, "repository_allowlist", Some("acme/widgets"));
        3
    ]);
    assert_eq!(r.recommendations.len(), 1);
    assert_eq!(r.recommendations[0].kind, Kind::RegistryPatch);
    assert_eq!(r.recommendations[0].denials, 3);
    assert!(r
        .patch
        .contains("+github_repo_allowlist:\n+  # membrane-advisor"));
    assert!(r.patch.contains("+  - acme/widgets"));
    assert!(r.patch.contains("-github_repo_allowlist: []"));
    assert!(r.patch.contains("review by 2027-01-22"));
}

#[test]
fn model_denial_extends_existing_block_and_keeps_comments() {
    let r = run(vec![deny(
        5,
        "model_allowlist",
        Some("qwen2.5-0.5b-instruct"),
    )]);
    assert_eq!(r.recommendations[0].kind, Kind::RegistryPatch);
    assert!(r.patch.contains("+  - qwen2.5-0.5b-instruct"));
    assert!(
        !r.patch.contains("-  - sha256:demo-model"),
        "existing entry untouched"
    );
}

#[test]
fn patch_applies_and_registry_stays_valid() {
    let r = run(vec![
        deny(1, "model_allowlist", Some("m-2")),
        deny(2, "repository_allowlist", Some("acme/widgets")),
    ]);
    // Re-derive the edited text by applying both entries and confirm membership.
    let mut text = REGISTRY.to_string();
    for (k, v) in r
        .recommendations
        .iter()
        .filter_map(|x| x.patch_entry.clone())
    {
        text = add_list_item(&text, &k, &v, "c").unwrap();
    }
    let parsed: ChannelRegistry = serde_yaml::from_str(&text).unwrap();
    assert!(parsed.model_allowlist.contains(&"m-2".to_string()));
    assert!(parsed
        .model_allowlist
        .contains(&"sha256:demo-model".to_string()));
    assert_eq!(parsed.github_repo_allowlist, vec!["acme/widgets"]);
    assert_eq!(parsed.delta_t_secs, 300);
    assert!(r.recommendations.iter().all(|x| x.patch_entry.is_some()));
    assert!(r.patch.contains("+  - m-2") && r.patch.contains("+  - acme/widgets"));
    assert!(r
        .patch
        .starts_with("--- a/tools/channel-registry.yaml\n+++ b/tools/channel-registry.yaml\n"));
}

#[test]
fn hostile_subjects_never_reach_a_patch() {
    for bad in [
        "*",
        "acme/*",
        "acme/widgets\n  - evil/repo",
        "../x/y",
        "a/b/c",
        "acme",
        "",
        "x y",
        "--- a/etc",
    ] {
        let r = run(vec![deny(1, "repository_allowlist", Some(bad))]);
        assert!(r.patch.is_empty(), "patched {bad:?}");
        assert_ne!(r.recommendations[0].kind, Kind::RegistryPatch);
    }
    for bad in [
        "*",
        "m\n  - evil",
        "model with space",
        "-leading",
        "m;rm",
        "",
    ] {
        let r = run(vec![deny(1, "model_allowlist", Some(bad))]);
        assert!(r.patch.is_empty(), "patched {bad:?}");
    }
}

#[test]
fn already_allowed_model_points_at_iac_not_registry() {
    let r = run(vec![deny(1, "model_allowlist", Some("sha256:demo-model"))]);
    assert_eq!(r.recommendations[0].kind, Kind::IacReissue);
    assert!(r.patch.is_empty());
}

#[test]
fn tool_denial_is_advice_only() {
    let r = run(vec![deny(1, "tool_allowlist", Some("github.merge"))]);
    assert_eq!(r.recommendations[0].kind, Kind::IacReissue);
    assert!(r.patch.is_empty());
}

#[test]
fn signature_failure_never_recommends_loosening() {
    let r = run(vec![deny(1, "iac_signature", None)]);
    assert_eq!(r.recommendations[0].kind, Kind::Investigate);
    assert!(r.patch.is_empty());
}

#[test]
fn stale_session_does_not_raise_delta_t() {
    let r = run(vec![deny(1, "session_stale", None)]);
    assert_eq!(r.recommendations[0].kind, Kind::Operate);
    assert!(r.patch.is_empty());
    assert!(r.recommendations[0]
        .summary
        .contains("Do not raise delta_t_secs"));
}

#[test]
fn missing_subject_yields_no_patch() {
    let r = run(vec![deny(1, "repository_allowlist", None)]);
    assert!(r.patch.is_empty());
    assert!(r.recommendations[0].summary.contains("no subject"));
}

#[test]
fn allows_are_ignored_and_threshold_applies() {
    let mut d = deny(1, "repository_allowlist", Some("acme/widgets"));
    d.outcome = "allow".into();
    assert!(run(vec![d]).recommendations.is_empty());
    let r = analyze(
        &snap(vec![deny(1, "repository_allowlist", Some("acme/widgets"))]),
        REGISTRY,
        "r.yaml",
        2,
    )
    .unwrap();
    assert!(r.recommendations.is_empty() && r.patch.is_empty());
}

#[test]
fn inline_list_is_refused_not_rewritten() {
    let text = REGISTRY.replace("github_repo_allowlist: []", "github_repo_allowlist: [a/b]");
    let r = analyze(
        &snap(vec![deny(1, "repository_allowlist", Some("acme/widgets"))]),
        &text,
        "r.yaml",
        1,
    )
    .unwrap();
    assert!(r.patch.is_empty());
    assert_eq!(r.recommendations[0].kind, Kind::Investigate);
}

#[test]
fn deterministic_and_order_independent() {
    let a = vec![
        deny(1, "model_allowlist", Some("m-2")),
        deny(2, "repository_allowlist", Some("acme/widgets")),
        deny(3, "session_stale", None),
    ];
    let mut b = a.clone();
    b.reverse();
    let (ra, rb) = (run(a), run(b));
    assert_eq!(render_text(&ra), render_text(&rb));
}

#[test]
fn old_snapshots_without_subject_still_parse() {
    let json = r#"{"sequence":1,"timestamp":1,"outcome":"deny","rule":"tool_allowlist","agent":"g","scope":null,"action":"tool"}"#;
    let d: Decision = serde_json::from_str(json).unwrap();
    assert!(d.subject.is_none());
}

#[test]
fn authenticated_callers_group_separately_and_never_widen_global_policy() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let mut one = deny(1, "model_allowlist", Some("new-model"));
    one.authenticated_identity = Some(a.clone());
    let mut two = deny(2, "model_allowlist", Some("new-model"));
    two.authenticated_identity = Some(b.clone());
    let r = analyze(&snap(vec![one, two]), REGISTRY, "registry.yaml", 1).unwrap();
    assert_eq!(r.recommendations.len(), 2);
    assert!(r.patch.is_empty());
    assert!(r
        .recommendations
        .iter()
        .all(|x| x.summary.contains(".revoked to true") && x.authenticated_identity.is_some()));
}
