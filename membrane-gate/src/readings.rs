//! Readings computed from the bounded audit log.
//!
//! A reading summarizes what the gate decided so an operator can spot drift,
//! especially in the allow log: volume by identity, new action types and the
//! clauses that stopped requests. Readings are observational. They never feed an
//! authorization decision, and they describe only the retained decisions, so
//! the coverage window is always reported alongside the numbers.
use crate::audit::Decision;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The recent period compared with everything retained before it.
pub const RECENT_SECS: i64 = 3600;
const MAX_IDENTITIES: usize = 10;
const MAX_ACTIONS: usize = 20;
const IDENTITY_DISPLAY_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Coverage {
    pub retained: usize,
    /// Decisions seen since the gate started that are no longer retained.
    pub dropped: u64,
    pub oldest_at: Option<i64>,
    pub newest_at: Option<i64>,
    pub recent_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleCount {
    pub rule: String,
    pub denies: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdentityReading {
    /// Caller key verified by proof of possession, shortened for display.
    pub identity: String,
    pub allows: usize,
    pub denies: usize,
    pub recent_allows: usize,
    pub baseline_allows: usize,
    /// Recent allows per hour divided by baseline allows per hour. Absent until
    /// the retained baseline covers at least one hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_ratio: Option<f64>,
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionCount {
    pub action: String,
    pub allows: usize,
    pub denies: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Readings {
    pub coverage: Coverage,
    pub allowed: usize,
    pub denied: usize,
    pub denied_by_rule: Vec<RuleCount>,
    pub identities: Vec<IdentityReading>,
    /// Decisions with no verified caller identity.
    pub unauthenticated: usize,
    pub actions: Vec<ActionCount>,
    /// Action types present in the recent period and absent from the baseline.
    /// Empty when there is no baseline to compare with.
    pub new_action_types: Vec<String>,
}

fn display_identity(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(IDENTITY_DISPLAY_LEN)
        .collect()
}

#[derive(Default)]
struct IdentityAcc {
    allows: usize,
    denies: usize,
    recent_allows: usize,
    baseline_allows: usize,
    last_seen_at: i64,
}

/// `decisions` is the audit snapshot, newest first. `total_observed` is the
/// gate's running count, used only to report how many decisions were dropped.
pub fn compute(now: i64, total_observed: u64, decisions: &[Decision]) -> Readings {
    let recent_from = now - RECENT_SECS;
    let oldest_at = decisions.iter().map(|d| d.timestamp).min();
    let newest_at = decisions.iter().map(|d| d.timestamp).max();

    let (mut allowed, mut denied, mut unauthenticated) = (0usize, 0usize, 0usize);
    let mut by_rule: BTreeMap<&str, usize> = BTreeMap::new();
    let mut identities: BTreeMap<String, IdentityAcc> = BTreeMap::new();
    let mut actions: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut recent_actions: BTreeSet<&str> = BTreeSet::new();
    let mut baseline_actions: BTreeSet<&str> = BTreeSet::new();

    for d in decisions {
        let is_allow = d.outcome == "allow";
        let is_deny = d.outcome == "deny";
        let recent = d.timestamp >= recent_from;
        if is_allow {
            allowed += 1;
        } else if is_deny {
            denied += 1;
            *by_rule.entry(d.rule.as_str()).or_default() += 1;
        }
        let entry = actions.entry(d.action.as_str()).or_default();
        if is_allow {
            entry.0 += 1;
        } else if is_deny {
            entry.1 += 1;
        }
        if recent {
            recent_actions.insert(d.action.as_str());
        } else {
            baseline_actions.insert(d.action.as_str());
        }
        match d.authenticated_identity.as_deref() {
            None => unauthenticated += 1,
            Some(id) => {
                let acc = identities.entry(display_identity(id)).or_default();
                acc.last_seen_at = acc.last_seen_at.max(d.timestamp);
                if is_allow {
                    acc.allows += 1;
                    if recent {
                        acc.recent_allows += 1;
                    } else {
                        acc.baseline_allows += 1;
                    }
                } else if is_deny {
                    acc.denies += 1;
                }
            }
        }
    }

    // The baseline is the retained time before the recent period.
    let baseline_hours = oldest_at
        .filter(|o| *o < recent_from)
        .map(|o| (recent_from - o) as f64 / 3600.0)
        .filter(|h| *h >= 1.0);

    let mut denied_by_rule: Vec<RuleCount> = by_rule
        .into_iter()
        .map(|(rule, denies)| RuleCount {
            rule: rule.into(),
            denies,
        })
        .collect();
    denied_by_rule.sort_by(|a, b| b.denies.cmp(&a.denies).then(a.rule.cmp(&b.rule)));

    let mut identity_rows: Vec<IdentityReading> = identities
        .into_iter()
        .map(|(identity, a)| IdentityReading {
            volume_ratio: baseline_hours.filter(|_| a.baseline_allows > 0).map(|h| {
                let recent_per_hour = a.recent_allows as f64 * 3600.0 / RECENT_SECS as f64;
                recent_per_hour / (a.baseline_allows as f64 / h)
            }),
            identity,
            allows: a.allows,
            denies: a.denies,
            recent_allows: a.recent_allows,
            baseline_allows: a.baseline_allows,
            last_seen_at: a.last_seen_at,
        })
        .collect();
    identity_rows.sort_by(|a, b| {
        (b.allows + b.denies)
            .cmp(&(a.allows + a.denies))
            .then(a.identity.cmp(&b.identity))
    });
    identity_rows.truncate(MAX_IDENTITIES);

    let mut action_rows: Vec<ActionCount> = actions
        .into_iter()
        .map(|(action, (allows, denies))| ActionCount {
            action: action
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect(),
            allows,
            denies,
        })
        .collect();
    action_rows.sort_by(|a, b| {
        (b.allows + b.denies)
            .cmp(&(a.allows + a.denies))
            .then(a.action.cmp(&b.action))
    });
    action_rows.truncate(MAX_ACTIONS);

    let new_action_types = if baseline_actions.is_empty() {
        Vec::new()
    } else {
        recent_actions
            .difference(&baseline_actions)
            .map(|a| a.chars().filter(|c| !c.is_control()).take(64).collect())
            .collect()
    };

    Readings {
        coverage: Coverage {
            retained: decisions.len(),
            dropped: total_observed.saturating_sub(decisions.len() as u64),
            oldest_at,
            newest_at,
            recent_secs: RECENT_SECS,
        },
        allowed,
        denied,
        denied_by_rule,
        identities: identity_rows,
        unauthenticated,
        actions: action_rows,
        new_action_types,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(seq: u64, ts: i64, outcome: &str, rule: &str, action: &str, id: Option<&str>) -> Decision {
        Decision {
            sequence: seq,
            timestamp: ts,
            outcome: outcome.into(),
            rule: rule.into(),
            agent: "gate".into(),
            authenticated_identity: id.map(Into::into),
            scope: None,
            action: action.into(),
            subject: None,
        }
    }

    #[test]
    fn empty_log_reports_no_coverage_and_no_ratios() {
        let r = compute(10_000, 0, &[]);
        assert_eq!((r.allowed, r.denied, r.unauthenticated), (0, 0, 0));
        assert!(r.coverage.oldest_at.is_none() && r.new_action_types.is_empty());
    }

    #[test]
    fn counts_denies_by_rule_and_splits_authenticated_callers() {
        let rows = vec![
            d(4, 100, "deny", "tool_allowlist", "tool", Some("AA")),
            d(3, 99, "deny", "tool_allowlist", "tool", None),
            d(2, 98, "deny", "iac_signature", "chat", None),
            d(1, 97, "allow", "ok", "chat", Some("aa")),
        ];
        let r = compute(200, 9, &rows);
        assert_eq!((r.allowed, r.denied, r.unauthenticated), (1, 3, 2));
        assert_eq!(
            r.denied_by_rule[0],
            RuleCount {
                rule: "tool_allowlist".into(),
                denies: 2
            }
        );
        assert_eq!(r.coverage.dropped, 5);
        let aa: Vec<_> = r.identities.iter().filter(|i| i.identity == "AA").collect();
        assert_eq!((aa[0].allows, aa[0].denies), (0, 1));
    }

    #[test]
    fn new_action_type_needs_a_baseline_and_absence_from_it() {
        let now = 100_000;
        let old = now - RECENT_SECS - 7200;
        let rows = vec![
            d(3, now - 10, "allow", "ok", "tool", Some("aa")),
            d(2, now - 20, "allow", "ok", "chat", Some("aa")),
            d(1, old, "allow", "ok", "chat", Some("aa")),
        ];
        assert_eq!(
            compute(now, 3, &rows).new_action_types,
            vec!["tool".to_string()]
        );
        let only_recent = vec![d(1, now - 10, "allow", "ok", "tool", Some("aa"))];
        assert!(compute(now, 1, &only_recent).new_action_types.is_empty());
    }

    #[test]
    fn volume_ratio_compares_recent_rate_with_baseline_rate() {
        let now = 100_000;
        let mut rows = Vec::new();
        // Baseline: 2 hours before the recent period, 4 allows -> 2 per hour.
        for i in 0..4 {
            rows.push(d(
                i + 1,
                now - RECENT_SECS - 7200 + (i as i64) * 10,
                "allow",
                "ok",
                "chat",
                Some("aa"),
            ));
        }
        // Recent hour: 6 allows -> 6 per hour, ratio 3.
        for i in 0..6 {
            rows.push(d(
                i + 10,
                now - 100 - (i as i64),
                "allow",
                "ok",
                "chat",
                Some("aa"),
            ));
        }
        rows.reverse();
        let r = compute(now, 10, &rows);
        let ratio = r.identities[0].volume_ratio.unwrap();
        assert!((ratio - 3.0).abs() < 0.05, "ratio {ratio}");
    }

    #[test]
    fn ratio_is_absent_when_baseline_is_under_an_hour_or_empty() {
        let now = 100_000;
        let rows = vec![
            d(2, now - 10, "allow", "ok", "chat", Some("aa")),
            d(
                1,
                now - RECENT_SECS - 600,
                "allow",
                "ok",
                "chat",
                Some("aa"),
            ),
        ];
        assert!(compute(now, 2, &rows).identities[0].volume_ratio.is_none());
    }

    #[test]
    fn identity_text_is_sanitized_and_bounded() {
        let long = format!("ab\n{}", "x".repeat(200));
        let rows = vec![d(
            1,
            5,
            "allow",
            "ok",
            &format!("a\u{7}{}", "y".repeat(200)),
            Some(&long),
        )];
        let r = compute(10, 1, &rows);
        assert!(r.identities[0].identity.len() <= IDENTITY_DISPLAY_LEN);
        assert!(!r.identities[0].identity.contains('\n'));
        assert!(r.actions[0].action.len() <= 64 && !r.actions[0].action.contains('\u{7}'));
    }
          }
