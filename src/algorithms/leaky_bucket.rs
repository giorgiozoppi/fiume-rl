//! # Leaky Bucket
//!
//! Requests enter a queue ("bucket") and are processed at a constant
//! `leak_rate` per second — excess requests are dropped when the queue is full.
//! This enforces a perfectly steady output rate regardless of input bursts.
//!
//! **Pro**: Perfectly smooth output; protects downstream services from bursts.
//! **Con**: Extra latency for queued requests; legitimate burst traffic is lost.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    limiter::{Decision, RateLimiter},
    store::SharedStore,
};

#[derive(Serialize, Deserialize)]
struct State {
    queue: f64,
    last_leak_ms: i64,
}

pub struct LeakyBucketLimiter {
    leak_rate: f64,
    capacity: u64,
    store: SharedStore,
}

impl LeakyBucketLimiter {
    pub fn new(leak_rate: f64, capacity: u64, store: SharedStore) -> Self {
        Self { leak_rate, capacity, store }
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
impl RateLimiter for LeakyBucketLimiter {
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision {
        let key = format!("leaky_bucket/{}/{}", client_id, resource);
        let cost_f = cost as f64;
        let cap = self.capacity as f64;

        for attempt in 0u32..5 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(2u64.pow(attempt))).await;
            }

            let now = now_ms();

            let (state, version) = match self.store.get(&key).await {
                Some((bytes, v)) => {
                    let s = serde_json::from_slice::<State>(&bytes)
                        .unwrap_or(State { queue: 0.0, last_leak_ms: now });
                    (s, Some(v))
                }
                None => (State { queue: 0.0, last_leak_ms: now }, None),
            };

            let elapsed_secs = (now - state.last_leak_ms).max(0) as f64 / 1_000.0;
            let queue = (state.queue - elapsed_secs * self.leak_rate).max(0.0);
            let new_queue = queue + cost_f;

            let (final_queue, allowed, retry_after_ms) = if new_queue <= cap {
                (new_queue, true, 0i64)
            } else {
                let overflow = new_queue - cap;
                let wait_ms = (overflow / self.leak_rate * 1_000.0) as i64;
                (queue, false, wait_ms)
            };

            let new_state = State { queue: final_queue, last_leak_ms: now };
            let bytes = serde_json::to_vec(&new_state).unwrap();

            if self.store.cas(&key, version, bytes).await.success {
                return Decision {
                    allowed,
                    limit: self.capacity as i64,
                    remaining: (cap - final_queue) as i64,
                    retry_after_ms,
                    reason: if allowed {
                        format!("queued — depth {:.1}/{:.0}", final_queue, cap)
                    } else {
                        format!(
                            "leaky-bucket full ({:.1}/{:.0}), retry in {}ms",
                            queue, cap, retry_after_ms
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
        "leaky_bucket"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::local::LocalStore;

    fn limiter(rate: f64, cap: u64) -> LeakyBucketLimiter {
        LeakyBucketLimiter::new(rate, cap, Arc::new(LocalStore::new()))
    }

    #[tokio::test]
    async fn name_is_correct() {
        assert_eq!(limiter(2.0, 5).name(), "leaky_bucket");
    }

    #[tokio::test]
    async fn allows_requests_up_to_capacity() {
        let l = limiter(1.0, 5);
        for i in 0..5 {
            assert!(l.check("c1", "r", 1).await.allowed, "request {i}");
        }
    }

    #[tokio::test]
    async fn denies_when_bucket_full() {
        let l = limiter(1.0, 3);
        l.check("c1", "r", 3).await;
        let d = l.check("c1", "r", 1).await;
        assert!(!d.allowed);
        assert!(d.retry_after_ms > 0);
    }

    #[tokio::test]
    async fn capacity_leaks_over_time() {
        let l = limiter(10.0, 3);
        l.check("c1", "r", 3).await;
        assert!(!l.check("c1", "r", 1).await.allowed);
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        assert!(l.check("c1", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn clients_are_isolated() {
        let l = limiter(1.0, 2);
        l.check("a", "r", 2).await;
        assert!(l.check("b", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn remaining_reflects_free_capacity() {
        let l = limiter(1.0, 5);
        let d = l.check("c1", "r", 2).await;
        assert!(d.allowed);
        assert_eq!(d.remaining, 3);
    }
}
