//! # Token Bucket
//!
//! Each client owns a "bucket" that fills with tokens at `refill_rate` tokens/s
//! up to `capacity`.  A request consumes `cost` tokens; if the bucket doesn't
//! have enough, the request is rejected.
//!
//! State is stored in the shared `StateStore` (local or etcd), serialised as
//! JSON, and updated via compare-and-swap so concurrent replicas never collide.
//!
//! **Pro**: Naturally handles bursts up to `capacity`; smooth long-term rate.
//! **Con**: Clients can accumulate tokens over idle periods and burst all at once.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    limiter::{Decision, RateLimiter},
    store::SharedStore,
};

#[derive(Serialize, Deserialize)]
struct State {
    tokens: f64,
    last_refill_ms: i64,
}

pub struct TokenBucketLimiter {
    capacity: f64,
    refill_rate: f64,
    store: SharedStore,
}

impl TokenBucketLimiter {
    pub fn new(capacity: f64, refill_rate: f64, store: SharedStore) -> Self {
        Self { capacity, refill_rate, store }
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards")
        .as_millis() as i64
}

#[async_trait]
impl RateLimiter for TokenBucketLimiter {
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision {
        let key = format!("token_bucket/{}/{}", client_id, resource);
        let cost_f = cost as f64;

        for attempt in 0u32..5 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(2u64.pow(attempt))).await;
            }

            let now = now_ms();
            let (state, version) = match self.store.get(&key).await {
                Some((bytes, v)) => {
                    let s = serde_json::from_slice::<State>(&bytes)
                        .unwrap_or(State { tokens: self.capacity, last_refill_ms: now });
                    (s, Some(v))
                }
                None => (State { tokens: self.capacity, last_refill_ms: now }, None),
            };

            let elapsed_secs = (now - state.last_refill_ms).max(0) as f64 / 1_000.0;
            let tokens = (state.tokens + elapsed_secs * self.refill_rate).min(self.capacity);

            let (new_tokens, allowed, retry_after_ms) = if tokens >= cost_f {
                (tokens - cost_f, true, 0i64)
            } else {
                let deficit = cost_f - tokens;
                let wait_ms = (deficit / self.refill_rate * 1_000.0) as i64;
                (tokens, false, wait_ms)
            };

            let new_state = State { tokens: new_tokens, last_refill_ms: now };
            let bytes = serde_json::to_vec(&new_state).unwrap();

            if self.store.cas(&key, version, bytes).await.success {
                return Decision {
                    allowed,
                    limit: self.capacity as i64,
                    remaining: new_tokens as i64,
                    retry_after_ms,
                    reason: if allowed {
                        format!("ok — {:.1} tokens remaining", new_tokens)
                    } else {
                        format!(
                            "token-bucket empty ({:.1}/{:.0} available), retry in {}ms",
                            new_tokens, cost_f, retry_after_ms
                        )
                    },
                };
            }
        }

        Decision {
            allowed: false,
            limit: self.capacity as i64,
            remaining: 0,
            retry_after_ms: 100,
            reason: "rate-limit check failed (concurrent update), retry in 100ms".to_string(),
        }
    }

    fn name(&self) -> &'static str {
        "token_bucket"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::local::LocalStore;

    fn limiter(cap: f64, rate: f64) -> TokenBucketLimiter {
        TokenBucketLimiter::new(cap, rate, Arc::new(LocalStore::new()))
    }

    #[tokio::test]
    async fn name_is_correct() {
        assert_eq!(limiter(10.0, 2.0).name(), "token_bucket");
    }

    #[tokio::test]
    async fn new_client_starts_full_and_allows_up_to_capacity() {
        let l = limiter(10.0, 1.0);
        for i in 0..10 {
            assert!(l.check("c1", "r", 1).await.allowed, "request {i} should be allowed");
        }
    }

    #[tokio::test]
    async fn deny_when_tokens_exhausted() {
        let l = limiter(3.0, 1.0);
        l.check("c1", "r", 3).await;
        let d = l.check("c1", "r", 1).await;
        assert!(!d.allowed);
        assert!(d.retry_after_ms > 0);
    }

    #[tokio::test]
    async fn tokens_refill_over_time() {
        let l = limiter(5.0, 10.0);
        l.check("c1", "r", 5).await;
        assert!(!l.check("c1", "r", 1).await.allowed);
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        assert!(l.check("c1", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn clients_are_isolated() {
        let l = limiter(2.0, 1.0);
        l.check("a", "r", 2).await;
        assert!(l.check("b", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn cost_larger_than_capacity_always_denied() {
        let l = limiter(5.0, 1.0);
        assert!(!l.check("c1", "r", 6).await.allowed);
    }

    #[tokio::test]
    async fn remaining_decreases_with_each_request() {
        let l = limiter(10.0, 0.0);
        let d1 = l.check("c1", "r", 3).await;
        let d2 = l.check("c1", "r", 3).await;
        assert!(d1.remaining > d2.remaining);
    }
}
