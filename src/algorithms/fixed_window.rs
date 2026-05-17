//! # Fixed Window Counter
//!
//! Divides the timeline into fixed-size buckets.  Each bucket has its own
//! counter; once the counter hits the limit, every subsequent request in that
//! bucket is rejected.
//!
//! **Pro**: O(1) state per client, trivial to distribute.
//! **Con**: Allows a burst of 2× the limit at a window boundary.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    limiter::{Decision, RateLimiter},
    store::SharedStore,
};

#[derive(Serialize, Deserialize)]
struct State {
    count: u64,
    window_id: u64,
}

pub struct FixedWindowLimiter {
    max_requests: u64,
    window_secs: u64,
    store: SharedStore,
}

impl FixedWindowLimiter {
    pub fn new(max_requests: u64, window_secs: u64, store: SharedStore) -> Self {
        Self { max_requests, window_secs, store }
    }

    fn now_secs() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_secs()
    }
}

#[async_trait]
impl RateLimiter for FixedWindowLimiter {
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision {
        let key = format!("fixed_window/{}/{}", client_id, resource);
        let cost = cost as u64;

        for attempt in 0u32..5 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(2u64.pow(attempt))).await;
            }

            let now = Self::now_secs();
            let window_id = now / self.window_secs;

            let (state, version) = match self.store.get(&key).await {
                Some((bytes, v)) => {
                    let s = serde_json::from_slice::<State>(&bytes)
                        .unwrap_or(State { count: 0, window_id });
                    (s, Some(v))
                }
                None => (State { count: 0, window_id }, None),
            };

            let count = if state.window_id == window_id { state.count } else { 0 };

            let (new_count, allowed, retry_after_ms) = if count + cost <= self.max_requests {
                (count + cost, true, 0i64)
            } else {
                let window_end = (window_id + 1) * self.window_secs;
                let wait_ms = window_end.saturating_sub(now) * 1_000;
                (count, false, wait_ms as i64)
            };

            let new_state = State { count: new_count, window_id };
            let bytes = serde_json::to_vec(&new_state).unwrap();

            if self.store.cas(&key, version, bytes).await.success {
                return Decision {
                    allowed,
                    limit: self.max_requests as i64,
                    remaining: (self.max_requests.saturating_sub(new_count)) as i64,
                    retry_after_ms,
                    reason: if allowed {
                        format!("ok — {}/{} used", new_count, self.max_requests)
                    } else {
                        format!(
                            "fixed-window limit {}/{} exceeded, resets in {}ms",
                            count, self.max_requests, retry_after_ms
                        )
                    },
                };
            }
        }

        Decision {
            allowed: false,
            limit: self.max_requests as i64,
            remaining: 0,
            retry_after_ms: 100,
            reason: "rate-limit check failed (concurrent update), retry in 100ms".to_string(),
        }
    }

    fn name(&self) -> &'static str {
        "fixed_window"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::local::LocalStore;

    fn limiter(max: u64, secs: u64) -> FixedWindowLimiter {
        FixedWindowLimiter::new(max, secs, Arc::new(LocalStore::new()))
    }

    #[tokio::test]
    async fn name_is_correct() {
        assert_eq!(limiter(5, 60).name(), "fixed_window");
    }

    #[tokio::test]
    async fn allows_up_to_max_requests() {
        let l = limiter(5, 60);
        for i in 0..5 {
            assert!(l.check("c1", "r", 1).await.allowed, "request {i}");
        }
    }

    #[tokio::test]
    async fn denies_when_limit_exceeded() {
        let l = limiter(3, 60);
        l.check("c1", "r", 3).await;
        let d = l.check("c1", "r", 1).await;
        assert!(!d.allowed);
        assert_eq!(d.remaining, 0);
        assert!(d.retry_after_ms > 0);
    }

    #[tokio::test]
    async fn clients_are_isolated() {
        let l = limiter(2, 60);
        l.check("a", "r", 2).await;
        assert!(l.check("b", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn cost_greater_than_one_consumed_correctly() {
        let l = limiter(10, 60);
        assert!(l.check("c1", "r", 7).await.allowed);
        assert!(!l.check("c1", "r", 4).await.allowed);
    }

    #[tokio::test]
    async fn remaining_decreases_with_each_request() {
        let l = limiter(10, 60);
        let d1 = l.check("c1", "r", 1).await;
        let d2 = l.check("c1", "r", 1).await;
        assert!(d1.remaining > d2.remaining);
    }
}
