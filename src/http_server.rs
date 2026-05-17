//! HTTP API server — headers conform to draft-ietf-httpapi-ratelimit-headers-08.
//!
//! ## Routes
//!
//! ### Rate-limit check
//!   POST /check
//!     Body:    {"client_id":"alice","resource":"/api/orders","cost":1}
//!     200 OK   → allowed
//!     429      → denied
//!
//! ### Kubernetes health probes
//!   GET /health/startup  → 200 once the process has fully initialised.
//!                          Maps to k8s startupProbe; gives the server time to
//!                          connect to etcd before liveness checks begin.
//!   GET /health/live     → 200 if the event loop is responsive (not deadlocked).
//!                          Maps to k8s livenessProbe; triggers pod restart on hang.
//!   GET /health/ready    → 200 when the backing store is reachable; 503 otherwise.
//!                          Maps to k8s readinessProbe; removes pod from Service
//!                          endpoints while etcd is partitioned or restarting.
//!
//! ### Standard rate-limit response headers (draft-ietf-httpapi-ratelimit-headers-08)
//!   RateLimit-Limit     — quota ceiling for this client/resource pair
//!   RateLimit-Remaining — units of quota left after this request
//!   RateLimit-Reset     — seconds until the quota resets (0 when allowed)
//!   RateLimit-Policy    — informational: algorithm name and limit
//!   Retry-After         — seconds to wait before retrying (429 only, RFC 9110)
//!
//! ### Extension header (strict_security mode)
//!   X-RateLimit-Mac: t=<unix_ms>,v=<hmac_sha256_hex>
//!   Signed payload: "{client_id}|{resource}|{allowed}|{remaining}|{retry_after_ms}|{t}"
//!   Authenticates the decision against tampering in transit
//!   (Fu, Sit, Smith, Feamster — USENIX Security 2001).

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::info;

use crate::{limiter::SharedLimiter, store::SharedStore};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct AppState {
    limiter: SharedLimiter,
    store: SharedStore,
    /// Present when strict_security = true in config.
    hmac_secret: Option<String>,
}

#[derive(Deserialize)]
struct CheckRequest {
    client_id: String,
    #[serde(default = "default_resource")]
    resource: String,
    #[serde(default = "default_cost")]
    cost: u32,
}

fn default_resource() -> String { "/".to_string() }
fn default_cost() -> u32 { 1 }

#[derive(Serialize)]
struct CheckResponse {
    allowed: bool,
    remaining: i64,
    retry_after_ms: i64,
    reason: String,
}

/// HMAC-SHA256 of `message` using `secret`, returned as a lowercase hex string.
fn hmac_sha256_hex(secret: &str, message: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Returns the current Unix timestamp in milliseconds.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Kubernetes health probe handlers ──────────────────────────────────────────

/// Startup probe — returns 200 as soon as the HTTP server is ready to accept
/// connections.  k8s uses this to give the pod time to connect to etcd before
/// liveness checks start ticking.
async fn health_startup() -> StatusCode {
    StatusCode::OK
}

/// Liveness probe — returns 200 if the async runtime can dispatch a handler.
/// A hanging response here causes k8s to restart the pod.
async fn health_live() -> StatusCode {
    StatusCode::OK
}

/// Readiness probe — returns 200 only when the backing store is reachable.
/// Returns 503 during etcd leader elections or network partitions so that k8s
/// removes this pod from the Service endpoints until the store recovers.
async fn health_ready(State(state): State<AppState>) -> StatusCode {
    if state.store.ping().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn check_handler(
    State(state): State<AppState>,
    Json(req): Json<CheckRequest>,
) -> impl IntoResponse {
    let d = state.limiter.check(&req.client_id, &req.resource, req.cost).await;

    let status = if d.allowed { StatusCode::OK } else { StatusCode::TOO_MANY_REQUESTS };

    let mut headers = HeaderMap::new();
    let hv = |s: String| HeaderValue::from_str(&s).unwrap_or(HeaderValue::from_static("0"));

    // ── draft-ietf-httpapi-ratelimit-headers-08 ────────────────────────────────
    headers.insert(
        HeaderName::from_static("ratelimit-limit"),
        hv(d.limit.to_string()),
    );
    headers.insert(
        HeaderName::from_static("ratelimit-remaining"),
        hv(d.remaining.to_string()),
    );
    // RateLimit-Reset: seconds until quota resets.
    // Denied → ceil(retry_after_ms / 1000), minimum 1.
    // Allowed → 0 (quota has not yet reset; client need not wait).
    let reset_secs = if d.allowed {
        0i64
    } else {
        (d.retry_after_ms / 1_000).max(1)
    };
    headers.insert(
        HeaderName::from_static("ratelimit-reset"),
        hv(reset_secs.to_string()),
    );
    // RateLimit-Policy: informational — algorithm name and quota ceiling.
    // Format follows the draft's structured-header convention: token;l=<limit>
    headers.insert(
        HeaderName::from_static("ratelimit-policy"),
        hv(format!("{};l={}", state.limiter.name(), d.limit)),
    );
    // Retry-After: standard 429 header (RFC 9110 §10.2.4)
    if !d.allowed && d.retry_after_ms > 0 {
        headers.insert(
            HeaderName::from_static("retry-after"),
            hv(reset_secs.to_string()),
        );
    }

    // ── X-RateLimit-Mac (strict_security mode) ─────────────────────────────────
    if let Some(secret) = &state.hmac_secret {
        let ts = now_ms();
        let msg = format!(
            "{}|{}|{}|{}|{}|{}",
            req.client_id, req.resource, d.allowed, d.remaining, d.retry_after_ms, ts
        );
        let sig = hmac_sha256_hex(secret, &msg);
        headers.insert(
            HeaderName::from_static("x-ratelimit-mac"),
            hv(format!("t={ts},v={sig}")),
        );
    }

    let body = Json(CheckResponse {
        allowed: d.allowed,
        remaining: d.remaining,
        retry_after_ms: d.retry_after_ms,
        reason: d.reason,
    });

    (status, headers, body)
}

pub async fn start(
    addr: &str,
    limiter: SharedLimiter,
    store: SharedStore,
    hmac_secret: Option<String>,
) -> anyhow::Result<()> {
    let state = AppState { limiter, store, hmac_secret };
    let app = Router::new()
        .route("/check", post(check_handler))
        .route("/health/startup", get(health_startup))
        .route("/health/live",    get(health_live))
        .route("/health/ready",   get(health_ready))
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    info!(addr, "HTTP server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
