//! End to end: registry file from the repo, snapshot JSON in the gate's /audit
//! shape, compiled binary, and `git apply --check` on the emitted patch.
use std::io::Write;
use std::process::{Command, Stdio};

const SNAPSHOT: &str = r#"{"schema_version":1,"observed_at":1800000000,"status":"live","last_cp_age_secs":3,"router_stale":false,"degraded":false,
"policy":{"permitted_channels":["local-llm"],"forbidden_exports":[],"model_allowlist":["sha256:demo-model"],"github_repo_allowlist":[],"delta_t_secs":300},
"decisions":[
{"sequence":3,"timestamp":1800000000,"outcome":"deny","rule":"repository_allowlist","agent":"gate","scope":null,"action":"tool","subject":"acme/pilot"},
{"sequence":2,"timestamp":1800000000,"outcome":"deny","rule":"tool_allowlist","agent":"gate","scope":null,"action":"tool","subject":"github.merge"},
{"sequence":1,"timestamp":1800000000,"outcome":"deny","rule":"repository_allowlist","agent":"gate","scope":null,"action":"tool","subject":"acme/pilot"}],
"total_observed":3,"retained":3,"allowed":0,"denied":3,"deny_rate":1.0,"audit_available":true}"#;

fn registry_path() -> String {
    format!(
        "{}/../tools/channel-registry.example.yaml",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn run(format: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_membrane-advisor"))
        .args(["--registry", &registry_path(), "--format", format])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SNAPSHOT.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn text_report_groups_and_proposes() {
    let t = run("text");
    assert!(t.contains("rule=repository_allowlist subject=acme/pilot denials=2"));
    assert!(t.contains("rule=tool_allowlist subject=github.merge"));
    assert!(t.contains("+  - acme/pilot"));
}

#[test]
fn patch_applies_cleanly_and_is_the_only_output() {
    let patch = run("patch");
    assert!(patch.starts_with("--- a/"));
    let dir = std::env::temp_dir().join(format!("advisor-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Apply against a copy; the advisor itself never writes the registry.
    let name = "registry.yaml";
    std::fs::copy(registry_path(), dir.join(name)).unwrap();
    let patch = patch.replace(&registry_path(), name);
    let patch_file = dir.join("p.patch");
    std::fs::write(&patch_file, patch).unwrap();
    let status = Command::new("git")
        .args(["apply", "--check", "-p1", "p.patch"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(status.success(), "git apply --check failed");
    let _ = std::fs::remove_dir_all(&dir);
}
