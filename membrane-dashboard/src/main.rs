//! Separate, unprivileged read-only audit consumer. No keys, token, or connector.
use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json,
};
use clap::Parser;
use membrane_gate::audit::{local_only, loopback_address, Snapshot};
use serde_json::json;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Local, read-only Membrane audit dashboard")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8790")]
    listen: String,
    #[arg(long, default_value = "http://127.0.0.1:8788")]
    audit_url: String,
}
#[derive(Clone)]
struct App {
    client: reqwest::Client,
    url: String,
}
fn audit_url(input: &str) -> anyhow::Result<String> {
    let url = reqwest::Url::parse(input)?;
    anyhow::ensure!(
        url.scheme() == "http"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "use a plain loopback HTTP base URL"
    );
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let ip: std::net::IpAddr = host.parse()?;
    anyhow::ensure!(
        ip.is_loopback(),
        "audit upstream must be a literal loopback IP"
    );
    Ok(url.join("audit")?.to_string())
}
async fn data(State(app): State<App>) -> Response {
    // Redirects disabled; timeout and size bound; never forwards browser cookies/headers.
    let result = async {
        let response = app.client.get(&app.url).send().await?.error_for_status()?;
        if response.content_length().is_some_and(|n| n > 2_000_000) {
            anyhow::bail!("snapshot too large");
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(bytes.len() + chunk.len() <= 2_000_000, "snapshot too large");
            bytes.extend_from_slice(&chunk);
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            snapshot.schema_version == 1 && snapshot.audit_available,
            "unsupported or unavailable audit snapshot"
        );
        Ok::<_, anyhow::Error>(snapshot)
    }
    .await;
    match result {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":"unknown", "error":"Audit source unavailable. Gate status is not verified."}))).into_response(),
    }
}
fn router(app: App) -> axum::Router {
    axum::Router::new()
        .route("/", get(|| async { Html(include_str!("index.html")) }))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("style.css"),
                )
            }),
        )
        .route("/api/snapshot", get(data))
        .with_state(app)
        .layer(middleware::from_fn(local_only))
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let address = loopback_address(&args.listen)?;
    let url = audit_url(&args.audit_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Read-only operator dashboard: http://{address}");
    axum::serve(listener, router(App { client, url }))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    fn app() -> axum::Router {
        router(App {
            client: reqwest::Client::new(),
            url: "http://127.0.0.1:1/audit".into(),
        })
    }
    #[test]
    fn only_literal_local_upstream() {
        for url in [
            "https://127.0.0.1",
            "http://example.com",
            "http://localhost",
            "http://127.0.0.1@evil.com",
            "http://127.0.0.1/path",
            "http://127.0.0.1?secret=x",
        ] {
            assert!(audit_url(url).is_err(), "{url}");
        }
        assert!(audit_url("http://[::1]:8788").is_ok());
    }
    #[tokio::test]
    async fn rejects_writes_and_rebinding() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/snapshot")
                    .method("POST")
                    .header("host", "127.0.0.1:8790")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "attacker.example:8790")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "127.0.0.1:8790")
                    .header("origin", "http://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    #[tokio::test]
    async fn offline_is_unknown_and_uncached() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/snapshot")
                    .header("host", "127.0.0.1:8790")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
