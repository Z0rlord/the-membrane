//! Real GitHub tool connector for the production gate.
//!
//! Enforced only after IAC `tool_allowlist` + repo allowlist checks.
//! Tokens come from env (`MEMBRANE_GITHUB_TOKEN` or `GITHUB_TOKEN`) — never logged.
//! Public demo / `membrane demo` keeps simulated tools and does not mount this path.

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::info;

/// Post a comment on an issue or pull request (same GitHub Issues Comments API).
pub const TOOL_GITHUB_COMMENT: &str = "github.comment";
/// Merge a pull request — typically denied by omitting from the IAC allowlist.
pub const TOOL_GITHUB_MERGE: &str = "github.merge";
/// Optional read-only tool id (no network side effect beyond GET).
pub const TOOL_GITHUB_ISSUE_READ: &str = "github.issue.read";

pub const ENV_TOKEN_PRIMARY: &str = "MEMBRANE_GITHUB_TOKEN";
pub const ENV_TOKEN_FALLBACK: &str = "GITHUB_TOKEN";
/// When set to `1`/`true`, ignored integration tests may hit live GitHub.
pub const ENV_INTEGRATION: &str = "MEMBRANE_GITHUB_INTEGRATION";

#[derive(Debug, Error)]
pub enum GitHubConnectorError {
    #[error("GitHub token missing; set {ENV_TOKEN_PRIMARY} or {ENV_TOKEN_FALLBACK}")]
    TokenMissing,
    #[error("repo not in gate allowlist: {0}")]
    RepoDenied(String),
    #[error("unsupported GitHub tool: {0}")]
    UnsupportedTool(String),
    #[error("invalid tool arguments: {0}")]
    InvalidArgs(String),
    #[error("GitHub API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("HTTP client error: {0}")]
    Http(String),
}

#[derive(Debug, Clone, Default)]
pub struct GitHubConnectorConfig {
    /// Explicit `owner/name` allowlist. Empty = deny all repos (fail closed).
    pub repo_allowlist: Vec<String>,
    /// Override API base (tests). Default: https://api.github.com
    pub api_base: String,
    /// Token from env; never logged.
    pub token: Option<String>,
}

impl GitHubConnectorConfig {
    pub fn from_env(repo_allowlist: Vec<String>) -> Self {
        let token = std::env::var(ENV_TOKEN_PRIMARY)
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var(ENV_TOKEN_FALLBACK).ok().filter(|s| !s.is_empty()));
        Self {
            repo_allowlist,
            api_base: "https://api.github.com".into(),
            token,
        }
    }

    pub fn repo_allowed(&self, owner: &str, repo: &str) -> bool {
        let key = format!("{owner}/{repo}");
        self.repo_allowlist.iter().any(|r| r == &key)
    }

    pub fn has_token(&self) -> bool {
        self.token.as_ref().is_some_and(|t| !t.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvokeRequest {
    pub tool: String,
    pub model: String,
    pub owner: String,
    pub repo: String,
    #[serde(default)]
    pub issue_number: Option<u64>,
    #[serde(default)]
    pub pull_number: Option<u64>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub commit_title: Option<String>,
}

/// Digest-only context for the CP Merkle tree — never includes comment body or tokens.
#[derive(Debug, Clone, Serialize)]
pub struct ToolReceiptContext {
    pub tool: String,
    pub model: String,
    pub owner: String,
    pub repo: String,
    pub simulation: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

pub fn body_sha256_hex(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

pub fn is_github_tool(tool_id: &str) -> bool {
    matches!(
        tool_id,
        TOOL_GITHUB_COMMENT | TOOL_GITHUB_MERGE | TOOL_GITHUB_ISSUE_READ
    )
}

/// Pure policy checks that must pass before any GitHub HTTP call.
pub fn authorize_repo_and_args(
    config: &GitHubConnectorConfig,
    req: &ToolInvokeRequest,
) -> Result<(), GitHubConnectorError> {
    if !is_github_tool(&req.tool) {
        return Err(GitHubConnectorError::UnsupportedTool(req.tool.clone()));
    }
    if req.owner.trim().is_empty() || req.repo.trim().is_empty() {
        return Err(GitHubConnectorError::InvalidArgs(
            "owner and repo are required".into(),
        ));
    }
    if !config.repo_allowed(&req.owner, &req.repo) {
        return Err(GitHubConnectorError::RepoDenied(format!(
            "{}/{}",
            req.owner, req.repo
        )));
    }
    match req.tool.as_str() {
        TOOL_GITHUB_COMMENT => {
            if req.issue_number.is_none() && req.pull_number.is_none() {
                return Err(GitHubConnectorError::InvalidArgs(
                    "issue_number or pull_number required for github.comment".into(),
                ));
            }
            if req.body.as_ref().map(|b| b.trim().is_empty()).unwrap_or(true) {
                return Err(GitHubConnectorError::InvalidArgs(
                    "body required for github.comment".into(),
                ));
            }
        }
        TOOL_GITHUB_MERGE => {
            if req.pull_number.is_none() {
                return Err(GitHubConnectorError::InvalidArgs(
                    "pull_number required for github.merge".into(),
                ));
            }
        }
        TOOL_GITHUB_ISSUE_READ => {
            if req.issue_number.is_none() && req.pull_number.is_none() {
                return Err(GitHubConnectorError::InvalidArgs(
                    "issue_number or pull_number required for github.issue.read".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

pub struct GitHubConnector {
    config: GitHubConnectorConfig,
    client: reqwest::Client,
}

impl GitHubConnector {
    pub fn new(config: GitHubConnectorConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("membrane-gate-github-connector/0.1")
            .build()
            .expect("reqwest client");
        Self { config, client }
    }

    pub fn config(&self) -> &GitHubConnectorConfig {
        &self.config
    }

    /// Validate repo/args and (for mutating tools) token presence — still no HTTP.
    pub fn preflight(&self, req: &ToolInvokeRequest) -> Result<(), GitHubConnectorError> {
        authorize_repo_and_args(&self.config, req)?;
        if matches!(
            req.tool.as_str(),
            TOOL_GITHUB_COMMENT | TOOL_GITHUB_MERGE | TOOL_GITHUB_ISSUE_READ
        ) && !self.config.has_token()
        {
            return Err(GitHubConnectorError::TokenMissing);
        }
        Ok(())
    }

    /// Execute a GitHub tool. Caller must already have passed `Gate::authorize_tool`.
    pub async fn execute(
        &self,
        req: &ToolInvokeRequest,
    ) -> Result<ToolReceiptContext, GitHubConnectorError> {
        self.preflight(req)?;
        let token = self
            .config
            .token
            .as_deref()
            .ok_or(GitHubConnectorError::TokenMissing)?;

        info!(
            tool = %req.tool,
            owner = %req.owner,
            repo = %req.repo,
            "github connector invoking (token redacted)"
        );

        match req.tool.as_str() {
            TOOL_GITHUB_COMMENT => self.post_comment(token, req).await,
            TOOL_GITHUB_MERGE => self.merge_pull(token, req).await,
            TOOL_GITHUB_ISSUE_READ => self.read_issue(token, req).await,
            other => Err(GitHubConnectorError::UnsupportedTool(other.into())),
        }
    }

    async fn post_comment(
        &self,
        token: &str,
        req: &ToolInvokeRequest,
    ) -> Result<ToolReceiptContext, GitHubConnectorError> {
        let number = req.issue_number.or(req.pull_number).unwrap();
        let body = req.body.as_deref().unwrap_or("");
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.config.api_base.trim_end_matches('/'),
            req.owner,
            req.repo,
            number
        );
        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&json!({ "body": body }))
            .send()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(GitHubConnectorError::Api {
                status,
                message: redact_tokenish(&text),
            });
        }
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        Ok(ToolReceiptContext {
            tool: req.tool.clone(),
            model: req.model.clone(),
            owner: req.owner.clone(),
            repo: req.repo.clone(),
            simulation: false,
            issue_number: Some(number),
            pull_number: req.pull_number,
            body_sha256: Some(body_sha256_hex(body)),
            result: Some(json!({
                "comment_id": parsed.get("id"),
                "html_url": parsed.get("html_url"),
            })),
        })
    }

    async fn merge_pull(
        &self,
        token: &str,
        req: &ToolInvokeRequest,
    ) -> Result<ToolReceiptContext, GitHubConnectorError> {
        let pull_number = req.pull_number.unwrap();
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/merge",
            self.config.api_base.trim_end_matches('/'),
            req.owner,
            req.repo,
            pull_number
        );
        let mut payload = json!({});
        if let Some(title) = &req.commit_title {
            payload["commit_title"] = json!(title);
        }
        let resp = self
            .client
            .put(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&payload)
            .send()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(GitHubConnectorError::Api {
                status,
                message: redact_tokenish(&text),
            });
        }
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        Ok(ToolReceiptContext {
            tool: req.tool.clone(),
            model: req.model.clone(),
            owner: req.owner.clone(),
            repo: req.repo.clone(),
            simulation: false,
            issue_number: None,
            pull_number: Some(pull_number),
            body_sha256: None,
            result: Some(json!({
                "merged": parsed.get("merged"),
                "sha": parsed.get("sha"),
            })),
        })
    }

    async fn read_issue(
        &self,
        token: &str,
        req: &ToolInvokeRequest,
    ) -> Result<ToolReceiptContext, GitHubConnectorError> {
        let number = req.issue_number.or(req.pull_number).unwrap();
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.config.api_base.trim_end_matches('/'),
            req.owner,
            req.repo,
            number
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| GitHubConnectorError::Http(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(GitHubConnectorError::Api {
                status,
                message: redact_tokenish(&text),
            });
        }
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(ToolReceiptContext {
            tool: req.tool.clone(),
            model: req.model.clone(),
            owner: req.owner.clone(),
            repo: req.repo.clone(),
            simulation: false,
            issue_number: Some(number),
            pull_number: req.pull_number,
            body_sha256: None,
            result: Some(json!({
                "number": parsed.get("number"),
                "state": parsed.get("state"),
                "title_sha256": body_sha256_hex(title),
                "html_url": parsed.get("html_url"),
            })),
        })
    }
}

fn redact_tokenish(s: &str) -> String {
    // Keep error text short; never echo likely secrets.
    let trimmed = if s.len() > 240 { &s[..240] } else { s };
    trimmed
        .replace("ghp_", "[redacted]")
        .replace("github_pat_", "[redacted]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(repos: &[&str]) -> GitHubConnectorConfig {
        GitHubConnectorConfig {
            repo_allowlist: repos.iter().map(|s| (*s).to_string()).collect(),
            api_base: "http://127.0.0.1:9".into(),
            token: Some("test-token".into()),
        }
    }

    #[test]
    fn deny_repo_not_on_allowlist_before_http() {
        let config = cfg(&["acme/allowed"]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "other".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("hi".into()),
            commit_title: None,
        };
        let err = authorize_repo_and_args(&config, &req).unwrap_err();
        assert!(matches!(err, GitHubConnectorError::RepoDenied(_)));
    }

    #[test]
    fn empty_allowlist_denies_all_repos() {
        let config = cfg(&[]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "x".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("hi".into()),
            commit_title: None,
        };
        assert!(matches!(
            authorize_repo_and_args(&config, &req),
            Err(GitHubConnectorError::RepoDenied(_))
        ));
    }

    #[test]
    fn allow_comment_args_on_listed_repo() {
        let config = cfg(&["acme/allowed"]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "allowed".into(),
            issue_number: Some(7),
            pull_number: None,
            body: Some("membrane pilot comment".into()),
            commit_title: None,
        };
        authorize_repo_and_args(&config, &req).unwrap();
    }

    #[test]
    fn preflight_requires_token_without_calling_github() {
        let config = GitHubConnectorConfig {
            repo_allowlist: vec!["acme/allowed".into()],
            api_base: "http://127.0.0.1:9".into(),
            token: None,
        };
        let connector = GitHubConnector::new(config);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "allowed".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("x".into()),
            commit_title: None,
        };
        let err = connector.preflight(&req).unwrap_err();
        assert!(matches!(err, GitHubConnectorError::TokenMissing));
    }

    #[test]
    fn body_digest_is_stable_and_not_plaintext() {
        let dig = body_sha256_hex("secret-in-comment");
        assert_eq!(dig.len(), 64);
        assert!(!dig.contains("secret"));
    }

    #[tokio::test]
    async fn mock_http_posts_comment_after_preflight() {
        use axum::{routing::post, Json, Router};
        use std::net::SocketAddr;
        use tokio::sync::oneshot;

        let (tx, rx) = oneshot::channel::<()>();
        let app = Router::new().route(
            "/repos/acme/allowed/issues/3/comments",
            post(|| async {
                Json(json!({
                    "id": 99,
                    "html_url": "https://github.com/acme/allowed/issues/3#issuecomment-99"
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .ok();
        });

        let config = GitHubConnectorConfig {
            repo_allowlist: vec!["acme/allowed".into()],
            api_base: format!("http://{addr}"),
            token: Some("test-token".into()),
        };
        let connector = GitHubConnector::new(config);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "allowed".into(),
            issue_number: Some(3),
            pull_number: None,
            body: Some("hello from membrane".into()),
            commit_title: None,
        };
        let ctx = connector.execute(&req).await.unwrap();
        assert!(!ctx.simulation);
        assert_eq!(
            ctx.body_sha256.as_deref(),
            Some(body_sha256_hex("hello from membrane").as_str())
        );
        assert_eq!(ctx.result.as_ref().unwrap()["comment_id"], 99);
        let _ = tx.send(());
        let _addr: SocketAddr = addr;
    }

    #[tokio::test]
    #[ignore = "set MEMBRANE_GITHUB_INTEGRATION=1 with a real token + disposable repo"]
    async fn live_github_integration() {
        let enabled = std::env::var(ENV_INTEGRATION)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !enabled {
            return;
        }
        let owner = std::env::var("MEMBRANE_GITHUB_OWNER").expect("MEMBRANE_GITHUB_OWNER");
        let repo = std::env::var("MEMBRANE_GITHUB_REPO").expect("MEMBRANE_GITHUB_REPO");
        let issue: u64 = std::env::var("MEMBRANE_GITHUB_ISSUE")
            .unwrap_or_else(|_| "1".into())
            .parse()
            .unwrap();
        let config = GitHubConnectorConfig::from_env(vec![format!("{owner}/{repo}")]);
        let connector = GitHubConnector::new(config);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "integration".into(),
            owner,
            repo,
            issue_number: Some(issue),
            pull_number: None,
            body: Some("Membrane integration check (safe to delete)".into()),
            commit_title: None,
        };
        let ctx = connector.execute(&req).await.unwrap();
        assert!(ctx.result.is_some());
    }
}
