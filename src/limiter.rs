use std::sync::Arc;

use async_trait::async_trait;

/// The outcome of one rate-limit check.
#[derive(Debug, Clone)]
pub struct Decision {
    /// `true` → the caller is allowed to proceed.
    pub allowed: bool,
    /// Configured maximum (tokens, requests, …) for this limiter.
    pub limit: i64,
    /// Units of capacity still available after this request.
    pub remaining: i64,
    /// How long (ms) the caller should wait before retrying.  0 when allowed.
    pub retry_after_ms: i64,
    /// Human-readable explanation surfaced in responses.
    pub reason: String,
}

/// Common interface every rate-limiting algorithm must implement.
///
/// All implementations must be cheaply clonable via `Arc` and callable from
/// multiple Tokio tasks concurrently (`Send + Sync`).
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Decide whether a request from `client_id` accessing `resource` and
    /// consuming `cost` units should be allowed.
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision;

    /// Short, display-friendly name of the algorithm.
    fn name(&self) -> &'static str;
}

/// A type-erased, reference-counted rate limiter shared across Tokio tasks.
pub type SharedLimiter = Arc<dyn RateLimiter>;
