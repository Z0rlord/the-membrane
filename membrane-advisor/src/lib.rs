//! Deterministic recommendation generator for Membrane gate denials.
//!
//! Input: an audit [`Snapshot`] (the gate's `/audit` JSON) and the operator's channel
//! registry YAML. Output: grouped recommendations and, where the registry can express
//! the fix, a unified diff a human reviews and merges through normal code review.
//!
//! Design rules (enforced here and covered by tests):
//! - No LLM, no network, no randomness. Same input, same output.
//! - Read-only. This crate never writes the registry; it returns text.
//! - The log is attacker-influenced. A denial subject becomes part of a patch only if it
//!   passes a strict allowlist-shaped check. Wildcards and odd characters never do.
//! - Narrowest change: one exact entry per patch, never a wildcard, never a loosened
//!   threshold. A denial is the gate working, so every recommendation says that doing nothing
//!   is a valid answer.
//! - Only fixes the registry can express become diffs. Tool allowlists, export lists and
//!   channels live in the signed IAC, which a text patch cannot change; those get
//!   advice, not a diff.

pub mod seam;

use anyhow::{anyhow, bail, Result};
use membrane_gate::audit::{Decision, Snapshot};
use membrane_gate::ChannelRegistry;
use serde::Serialize;
use similar::TextDiff;
use std::collections::BTreeMap;

/// Days after the snapshot an added entry should be re-reviewed. Written as a comment;
/// the registry format has no expiry, so this is not enforced by the gate.
pub const REVIEW_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A registry change that would allow the denied call.
    RegistryPatch,
    /// Needs a re-issued signed IAC; a text patch cannot express it.
    IacReissue,
    /// Operational action, no policy change.
    Operate,
    /// Investigate before changing anything.
    Investigate,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub rule: String,
    pub authenticated_identity: Option<String>,
    pub subject: Option<String>,
    pub kind: Kind,
    pub denials: usize,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub summary: String,
    /// Registry key and entry the patch adds, when `kind` is `registry_patch`.
    pub patch_entry: Option<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub observed_at: i64,
    pub retained: usize,
    pub denied: usize,
    pub recommendations: Vec<Recommendation>,
    /// Unified diff against the registry file, empty when no registry change applies.
    pub patch: String,
    /// Optional model triage notes (see [`seam`]). Advisory; empty unless a backend ran.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<seam::Note>,
}

/// Ask `backend` about each rule's denial group and attach validated notes. Notes never
/// change `recommendations` or `patch`; a failed or invalid answer is simply absent.
pub fn annotate<B: seam::Backend>(r: &mut Report, backend: &B) {
    let mut by_rule: BTreeMap<
        &str,
        (
            usize,
            std::collections::BTreeSet<&str>,
            std::collections::BTreeSet<&str>,
        ),
    > = BTreeMap::new();
    for rec in &r.recommendations {
        let e = by_rule.entry(rec.rule.as_str()).or_default();
        e.0 += rec.denials;
        if let Some(s) = rec.subject.as_deref() {
            e.1.insert(s);
        }
        if let Some(i) = rec.authenticated_identity.as_deref() {
            e.2.insert(i);
        }
    }
    let notes: Vec<seam::Note> = by_rule
        .into_iter()
        .filter_map(|(rule, (n, subj, ids))| {
            seam::advise(
                backend,
                &seam::GroupView {
                    rule: rule.to_string(),
                    denials: n,
                    distinct_subjects: subj.len(),
                    identified_callers: ids.len(),
                },
            )
        })
        .collect();
    r.notes = notes;
}

/// A subject may enter a patch only if it is exactly one plain allowlist entry.
pub fn safe_model_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/' | '@' | '+')
        })
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

pub fn safe_repo(s: &str) -> bool {
    let mut parts = s.split('/');
    let (Some(o), Some(r), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ok = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p != "."
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    ok(o) && ok(r)
}

#[derive(Default)]
struct Group {
    count: usize,
    first: u64,
    last: u64,
}

fn group_denials(
    decisions: &[Decision],
) -> BTreeMap<(String, Option<String>, Option<String>), Group> {
    let mut map: BTreeMap<(String, Option<String>, Option<String>), Group> = BTreeMap::new();
    for d in decisions.iter().filter(|d| d.outcome == "deny") {
        let g = map
            .entry((
                d.rule.clone(),
                d.subject.clone(),
                d.authenticated_identity.clone(),
            ))
            .or_default();
        if g.count == 0 {
            g.first = d.sequence;
            g.last = d.sequence;
        }
        g.count += 1;
        g.first = g.first.min(d.sequence);
        g.last = g.last.max(d.sequence);
    }
    map
}

/// Insert `item` into the top-level YAML list `key`, preserving comments and layout.
/// Errors (so the caller falls back to advice) on shapes it will not guess at.
pub fn add_list_item(text: &str, key: &str, item: &str, comment: &str) -> Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let head = format!("{key}:");
    let idx = lines
        .iter()
        .position(|l| l.starts_with(&head))
        .ok_or_else(|| anyhow!("key `{key}` not found at top level"))?;
    let rest = lines[idx][head.len()..]
        .split('#')
        .next()
        .unwrap_or("")
        .trim();
    let mut out: Vec<String> = Vec::new();
    let new_item = |indent: &str| vec![format!("{indent}# {comment}"), format!("{indent}- {item}")];
    match rest {
        "[]" => {
            out.extend(lines[..idx].iter().map(|s| s.to_string()));
            out.push(head.clone());
            out.extend(new_item("  "));
            out.extend(lines[idx + 1..].iter().map(|s| s.to_string()));
        }
        "" => {
            let mut last = None;
            let mut indent = "  ".to_string();
            for (j, l) in lines.iter().enumerate().skip(idx + 1) {
                let t = l.trim_start();
                if t.starts_with("- ") || t == "-" {
                    if last.is_none() {
                        indent = l[..l.len() - t.len()].to_string();
                    }
                    last = Some(j);
                } else if t.is_empty() || t.starts_with('#') {
                    continue;
                } else {
                    break;
                }
            }
            let last = last.ok_or_else(|| anyhow!("`{key}` has no list items to extend"))?;
            out.extend(lines[..=last].iter().map(|s| s.to_string()));
            out.extend(new_item(&indent));
            out.extend(lines[last + 1..].iter().map(|s| s.to_string()));
        }
        _ => bail!("`{key}` uses an inline list; refusing to rewrite it"),
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    Ok(s)
}

fn review_by(observed_at: i64) -> String {
    chrono::DateTime::from_timestamp(observed_at + REVIEW_DAYS * 86_400, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Try to add `item` to `key`, then prove the result still parses as a registry that
/// contains it and changed nothing else's membership. Plain scalar first, quoted second.
fn checked_add(text: &str, key: &str, item: &str, comment: &str) -> Result<String> {
    // Baseline is the text so far, so earlier accepted edits do not read as drift.
    let registry: ChannelRegistry = serde_yaml::from_str(text)?;
    let contains = |r: &ChannelRegistry| match key {
        "model_allowlist" => r.model_allowlist.iter().any(|m| m == item),
        "github_repo_allowlist" => r.github_repo_allowlist.iter().any(|m| m == item),
        _ => false,
    };
    for candidate in [item.to_string(), format!("\"{item}\"")] {
        let Ok(edited) = add_list_item(text, key, &candidate, comment) else {
            continue;
        };
        let Ok(parsed) = serde_yaml::from_str::<ChannelRegistry>(&edited) else {
            continue;
        };
        let unchanged = parsed.identities == registry.identities
            && parsed.permitted_channels == registry.permitted_channels
            && parsed.forbidden_exports == registry.forbidden_exports
            && parsed.delta_t_secs == registry.delta_t_secs
            && (key == "model_allowlist" || parsed.model_allowlist == registry.model_allowlist)
            && (key == "github_repo_allowlist"
                || parsed.github_repo_allowlist == registry.github_repo_allowlist);
        if contains(&parsed) && unchanged {
            return Ok(edited);
        }
    }
    bail!("could not produce a verified edit for `{key}`")
}

/// Build the report. `registry_path` is only used to label the diff.
pub fn analyze(
    snapshot: &Snapshot,
    registry_text: &str,
    registry_path: &str,
    min_denials: usize,
) -> Result<Report> {
    let registry: ChannelRegistry = serde_yaml::from_str(registry_text)?;
    let comment = format!(
        "membrane-advisor: added after denials; review by {} (not enforced)",
        review_by(snapshot.observed_at)
    );
    let mut recs = Vec::new();
    let mut edited = registry_text.to_string();
    let mut applied: Vec<(String, String)> = Vec::new();

    for ((rule, subject, identity), g) in group_denials(&snapshot.decisions) {
        if g.count < min_denials {
            continue;
        }
        let mut rec = Recommendation {
            rule: rule.clone(),
            authenticated_identity: identity.clone(),
            subject: subject.clone(),
            kind: Kind::Investigate,
            denials: g.count,
            first_sequence: g.first,
            last_sequence: g.last,
            summary: String::new(),
            patch_entry: None,
        };
        let shown = subject.as_deref().unwrap_or("-");
        match (rule.as_str(), subject.as_deref()) {
            ("model_allowlist", Some(m)) => {
                if !safe_model_id(m) {
                    rec.summary = "Denied model name is not a plain identifier, so no patch is generated. The name is caller-chosen text; treat it as hostile until reviewed.".into();
                } else if registry.model_allowlist.iter().any(|x| x == m) {
                    rec.kind = Kind::IacReissue;
                    rec.summary = format!("`{m}` is already in the registry, so the signed IAC is what denies it. Re-issue the IAC (`membrane iac reissue --add-model`) only if the use is intended. No registry change.");
                } else {
                    rec.kind = Kind::RegistryPatch;
                    rec.summary = format!("Add `{m}` to model_allowlist only if this model is meant to run. The signed IAC must also list it, or the gate still denies. If the call was not intended, change nothing: the denial is correct.");
                    rec.patch_entry = Some(("model_allowlist".into(), m.to_string()));
                }
            }
            ("repository_allowlist", Some(r)) => {
                if !safe_repo(r) {
                    rec.summary = "Denied repository is not a plain owner/name, so no patch is generated. Wildcards are never recommended.".into();
                } else if registry.github_repo_allowlist.iter().any(|x| x == r) {
                    rec.summary = "Repository is already allowlisted; the denial came from elsewhere. Check the matching tool and IAC denials.".into();
                } else {
                    rec.kind = Kind::RegistryPatch;
                    rec.summary = format!("Add `{r}` to github_repo_allowlist only if this exact repository is meant to be writable. Otherwise change nothing.");
                    rec.patch_entry = Some(("github_repo_allowlist".into(), r.to_string()));
                }
            }
            ("tool_allowlist", s) => {
                rec.kind = Kind::IacReissue;
                rec.summary = format!("Tool `{shown}` is not in the signed IAC. Tool allowlists live in the IAC, not the registry, so there is no text patch. Re-issue the IAC (`membrane iac reissue --add-tool`) with this one tool only if intended.{}", if s.is_none() { " The log did not record the tool name (gate predates subject logging)." } else { "" });
            }
            ("model_allowlist" | "repository_allowlist", None) => {
                rec.summary = "Denied, but the log has no subject (gate predates subject logging). Upgrade the gate to get a specific recommendation.".into();
            }
            ("iac_validity", _) => {
                rec.kind = Kind::IacReissue;
                rec.summary = "IAC missing or expired. Re-issue a signed IAC with a new valid_until (`membrane iac reissue`) if the session should continue.".into();
            }
            ("identity_authentication", _) => {
                rec.summary = "Caller proof missing, forged, stale or replayed. Do not grant a claimed identity or loosen policy.".into();
            }
            ("identity_grant", _) => {
                rec.summary = "Authenticated caller has no active matching identity grant. Inspect the exact scope and revoked flag. Do not automatically grant or un-revoke from a denial.".into();
            }
            ("grant_window", _) => {
                rec.summary = "Identity grant is expired, not yet valid or has a contradictory window. Renewing is an operator decision: edit not_before and expires_at in the registry if access should continue. Do not extend a grant automatically from a denial.".into();
            }
            ("iac_signature", _) => {
                rec.summary = "IAC signature failed. Do not change policy. This is a tampered or foreign credential until shown otherwise.".into();
            }
            ("channel_allowlist" | "export_restriction" | "context_bound", _) => {
                rec.kind = Kind::IacReissue;
                rec.summary = "The signed IAC does not match the registry's channel, export or context constraints. Re-issue the IAC to match the registry. Do not loosen the registry to fit a stale IAC.".into();
            }
            ("session_degraded" | "session_stale", _) => {
                rec.kind = Kind::Operate;
                rec.summary = "Router checkpoint is stale or the session was severed. Restore the router and its checkpoints. Do not raise delta_t_secs to hide it.".into();
            }
            ("connector_unavailable" | "receipt_or_upstream_unavailable", _) => {
                rec.kind = Kind::Operate;
                rec.summary =
                    "Connector or upstream unavailable. Operational fault, not a policy gap."
                        .into();
            }
            ("request_or_registry_invalid", _) => {
                rec.summary = "Malformed request or registry error. Inspect the caller; no policy change is implied.".into();
            }
            _ => {
                rec.summary =
                    format!("Unrecognized rule `{rule}`. No recommendation; inspect manually.");
            }
        }
        if let Some(ref key) = identity {
            // The log is not authority: caller text must still be a canonical key.
            if key.len() == 64
                && key
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                rec.summary.push_str(&format!(" Authenticated caller: {key}. If this activity is unintended, set identities.{key}.revoked to true; review before applying."));
            }
            if rec.patch_entry.is_some() {
                rec.kind = Kind::Investigate;
                rec.patch_entry = None;
                rec.summary.push_str(" Identity grants are an additional intersection; no global widening patch is generated for this caller.");
            }
        }
        if let Some((key, item)) = rec.patch_entry.clone() {
            match checked_add(&edited, &key, &item, &comment) {
                Ok(next) => {
                    edited = next;
                    applied.push((key, item));
                }
                Err(e) => {
                    rec.kind = Kind::Investigate;
                    rec.patch_entry = None;
                    rec.summary.push_str(&format!(" No patch generated: {e}."));
                }
            }
        }
        recs.push(rec);
    }

    // Patch-bearing recommendations first, then by volume, then by name: stable order.
    recs.sort_by(|a, b| {
        (b.kind == Kind::RegistryPatch)
            .cmp(&(a.kind == Kind::RegistryPatch))
            .then(b.denials.cmp(&a.denials))
            .then(a.rule.cmp(&b.rule))
            .then(a.subject.cmp(&b.subject))
    });

    let patch = if edited == registry_text {
        String::new()
    } else {
        TextDiff::from_lines(registry_text, &edited)
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{registry_path}"), &format!("b/{registry_path}"))
            .to_string()
    };
    Ok(Report {
        observed_at: snapshot.observed_at,
        retained: snapshot.retained,
        denied: snapshot.denied,
        recommendations: recs,
        patch,
        notes: Vec::new(),
    })
}

pub fn render_text(r: &Report) -> String {
    let mut s = format!(
        "membrane-advisor report (advisory only; nothing was changed)\nretained decisions: {}, denied: {}\n\n",
        r.retained, r.denied
    );
    if r.recommendations.is_empty() {
        s.push_str("No denials at or above the threshold. Nothing to recommend.\n");
    }
    for (i, rec) in r.recommendations.iter().enumerate() {
        s.push_str(&format!(
            "{}. [{:?}] rule={} subject={} denials={} (seq {}..{})\n   {}\n",
            i + 1,
            rec.kind,
            rec.rule,
            rec.subject.as_deref().unwrap_or("-"),
            rec.denials,
            rec.first_sequence,
            rec.last_sequence,
            rec.summary
        ));
    }
    if !r.notes.is_empty() {
        s.push_str("\nModel triage (advisory; a suggestion to read, never a decision):\n");
        for n in &r.notes {
            s.push_str(&format!(
                "  rule={} triage={:?} p_legitimate={:.2} via {}{}\n",
                n.rule,
                n.triage,
                n.p_legitimate,
                n.backend,
                n.backend_model
                    .as_deref()
                    .map(|m| format!(" ({m})"))
                    .unwrap_or_default()
            ));
        }
    }
    if !r.patch.is_empty() {
        s.push_str("\nProposed registry patch (review, then apply and merge through your normal code channel):\n\n");
        s.push_str(&r.patch);
    }
    s
}

#[cfg(test)]
mod tests;
