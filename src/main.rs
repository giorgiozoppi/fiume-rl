//! Rate-limiting server entry point.
//!
//! Usage:
//! ```bash
//! cargo run --bin server [config.yaml]
//! RUST_LOG=debug cargo run --bin server
//! ```

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use rate_limiting::{
    algorithms::{
        fixed_window::FixedWindowLimiter, leaky_bucket::LeakyBucketLimiter,
        sliding_window_counter::SlidingWindowCounterLimiter,
        sliding_window_log::SlidingWindowLogLimiter, token_bucket::TokenBucketLimiter,
    },
    config::{Algorithm, Config},
    http_server,
    limiter::SharedLimiter,
    server::Server,
    store::{local::LocalStore, etcd::EtcdStore, SharedStore},
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("rate_limiting=info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.yaml".to_string());

    let config_str = std::fs::read_to_string(&config_path).context("reading config file")?;
    let config: Config = serde_yaml::from_str(&config_str).context("parsing config YAML")?;

    info!(path = %config_path, algorithm = ?config.rate_limiter.algorithm, "config loaded");

    // ── build state store ─────────────────────────────────────────────────────
    let store: SharedStore = match &config.etcd {
        Some(etcd_cfg) => {
            info!(endpoints = ?etcd_cfg.endpoints, prefix = %etcd_cfg.key_prefix, "connecting to etcd");
            let s = EtcdStore::connect(&etcd_cfg.endpoints, &etcd_cfg.key_prefix)
                .await
                .context("connecting to etcd")?;
            info!("etcd connected — using distributed store");
            Arc::new(s)
        }
        None => {
            info!("no etcd config — using local in-memory store");
            Arc::new(LocalStore::new())
        }
    };

    // ── build rate limiter ────────────────────────────────────────────────────
    let rl = &config.rate_limiter;
    let limiter: SharedLimiter = match rl.algorithm {
        Algorithm::FixedWindow => Arc::new(FixedWindowLimiter::new(
            rl.max_requests,
            rl.window_secs,
            Arc::clone(&store),
        )),
        Algorithm::SlidingWindowLog => Arc::new(SlidingWindowLogLimiter::new(
            rl.max_requests,
            rl.window_secs,
            Arc::clone(&store),
        )),
        Algorithm::SlidingWindowCounter => Arc::new(SlidingWindowCounterLimiter::new(
            rl.max_requests,
            rl.window_secs,
            Arc::clone(&store),
        )),
        Algorithm::TokenBucket => Arc::new(TokenBucketLimiter::new(
            rl.bucket_capacity,
            rl.refill_rate,
            Arc::clone(&store),
        )),
        Algorithm::LeakyBucket => Arc::new(LeakyBucketLimiter::new(
            rl.leak_rate,
            rl.queue_capacity,
            Arc::clone(&store),
        )),
    };

    info!(name = limiter.name(), "rate limiter ready");

    // ── start servers ─────────────────────────────────────────────────────────
    let tcp_addr = format!("{}:{}", config.server.host, config.server.port);
    let http_addr = format!("{}:{}", config.server.host, config.server.http_port);

    let tcp_server = Server::bind(&tcp_addr, Arc::clone(&limiter)).await?;

    // ── resolve HMAC secret from environment ──────────────────────────────────
    // The secret is never stored in config.yaml (which lands in a k8s ConfigMap).
    // K8s injects it via a Secret mounted as RATE_LIMIT_HMAC_SECRET.
    let hmac_secret = if config.server.strict_security {
        match std::env::var("RATE_LIMIT_HMAC_SECRET") {
            Ok(s) if !s.is_empty() => {
                info!("strict_security enabled — X-RateLimit-Mac signing active");
                Some(s)
            }
            _ => {
                tracing::warn!(
                    "strict_security=true but RATE_LIMIT_HMAC_SECRET is not set; \
                     X-RateLimit-Mac header will be omitted"
                );
                None
            }
        }
    } else {
        None
    };

    tokio::select! {
        res = tcp_server.run() => res.context("TCP server error"),
        res = http_server::start(&http_addr, limiter, Arc::clone(&store), hmac_secret) => res.context("HTTP server error"),
    }
}
