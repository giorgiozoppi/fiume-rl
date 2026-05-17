use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    /// Optional etcd cluster; omit to use an in-process local store.
    #[serde(default)]
    pub etcd: Option<EtcdConfig>,
    pub rate_limiter: RateLimiterConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Port for the HTTP API (rate-limit headers).  Defaults to 8080.
    #[serde(default = "defaults::http_port")]
    pub http_port: u16,
    /// When true every HTTP response carries an X-RateLimit-Mac header
    /// (HMAC-SHA256 over the decision fields) so clients can verify
    /// the response was not tampered with (Fu et al., USENIX Sec 2001).
    /// The signing key is read from the RATE_LIMIT_HMAC_SECRET env var
    /// (injected via a Kubernetes Secret — never put secrets in config.yaml).
    #[serde(default)]
    pub strict_security: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EtcdConfig {
    /// etcd endpoints, e.g. ["http://etcd-0.etcd:2379", ...].
    pub endpoints: Vec<String>,
    /// Key namespace inside etcd.  Defaults to "rate-limiting".
    #[serde(default = "defaults::key_prefix")]
    pub key_prefix: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimiterConfig {
    pub algorithm: Algorithm,

    // ── window-based algorithms ───────────────────────────────────────────────
    #[serde(default = "defaults::max_requests")]
    pub max_requests: u64,
    #[serde(default = "defaults::window_secs")]
    pub window_secs: u64,

    // ── token bucket ─────────────────────────────────────────────────────────
    #[serde(default = "defaults::bucket_capacity")]
    pub bucket_capacity: f64,
    /// Tokens added to every client bucket per second.
    #[serde(default = "defaults::refill_rate")]
    pub refill_rate: f64,

    // ── leaky bucket ─────────────────────────────────────────────────────────
    /// Requests drained from the queue per second.
    #[serde(default = "defaults::leak_rate")]
    pub leak_rate: f64,
    #[serde(default = "defaults::queue_capacity")]
    pub queue_capacity: u64,
}

/// Which rate-limiting algorithm the server should use.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    FixedWindow,
    SlidingWindowLog,
    SlidingWindowCounter,
    TokenBucket,
    LeakyBucket,
}

mod defaults {
    pub fn http_port() -> u16 { 8080 }
    pub fn key_prefix() -> String { "rate-limiting".to_string() }
    pub fn max_requests() -> u64 { 100 }
    pub fn window_secs() -> u64 { 1 }
    pub fn bucket_capacity() -> f64 { 100.0 }
    pub fn refill_rate() -> f64 { 10.0 }
    pub fn leak_rate() -> f64 { 10.0 }
    pub fn queue_capacity() -> u64 { 100 }
}
