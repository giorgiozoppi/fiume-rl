//! # Sliding Window Counter
//!
//! Blends the previous window's counter with the current window's counter,
//! weighted by how far into the current window we are:
//!
//! ```text
//! weighted = prev_count × (1 − elapsed/window) + curr_count
//! ```
//!
//! **Pro**: O(1) state per client, no boundary burst, distributable.
//! **Con**: Slightly approximate (assumes previous window was uniformly distributed).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    limiter::{Decision, RateLimiter},
    store::SharedStore,
};

#[derive(Serialize, Deserialize)]
struct State {
    prev_count: u64,
    curr_count: u64,
    curr_window_id: u64,
}

pub struct SlidingWindowCounterLimiter {
    max_requests: u64,
    window_secs: u64,
    store: SharedStore,
}

impl SlidingWindowCounterLimiter {
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
impl RateLimiter for SlidingWindowCounterLimiter {
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision {
        let key = format!("sliding_window_counter/{}/{}", client_id, resource);
        let cost = cost as u64;

        for attempt in 0u32..5 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(2u64.pow(attempt))).await;
            }

            let now = Self::now_secs();
            let window_id = now / self.window_secs;
            let elapsed_fraction =
                (now % self.window_secs) as f64 / self.window_secs as f64;

            let (state, version) = match self.store.get(&key).await {
                Some((bytes, v)) => {
                    let s = serde_json::from_slice::<State>(&bytes).unwrap_or(State {
                        prev_count: 0,
                        curr_count: 0,
                        curr_window_id: window_id,
                    });
                    (s, Some(v))
                }
                None => (State { prev_count: 0, curr_count: 0, curr_window_id: window_id }, None),
            };

            let diff = window_id.saturating_sub(state.curr_window_id);
            let (prev_count, curr_count) = match diff {
                0 => (state.prev_count, state.curr_count),
                1 => (state.curr_count, 0),
                _ => (0, 0),
            };

            let weighted = prev_count as f64 * (1.0 - elapsed_fraction) + curr_count as f64;

            let (new_curr, allowed, retry_after_ms) =
                if weighted + cost as f64 <= self.max_requests as f64 {
                    (curr_count + cost, true, 0i64)
                } else {
                    let wait_ms =
                        ((1.0 - elapsed_fraction) * self.window_secs as f64 * 1_000.0) as i64;
                    (curr_count, false, wait_ms)
                };

            let remaining =
                (self.max_requests as f64 - weighted - cost as f64).max(0.0) as i64;

            let new_state = State { prev_count, curr_count: new_curr, curr_window_id: window_id };
            let bytes = serde_json::to_vec(&new_state).unwrap();

            if self.store.cas(&key, version, bytes).await.success {
                return Decision {
                    allowed,
                    limit: self.max_requests as i64,
                    remaining: if allowed { remaining } else { 0 },
                    retry_after_ms,
                    reason: if allowed {
                        format!(
                            "ok — weighted {:.1}/{} in window",
                            weighted + cost as f64,
                            self.max_requests
                        )
                    } else {
                        format!(
                            "sliding-counter limit exceeded (weighted {:.1}/{}), retry in {}ms",
                            weighted, self.max_requests, retry_after_ms
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
        "sliding_window_counter"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::local::LocalStore;

    fn limiter(max: u64, secs: u64) -> SlidingWindowCounterLimiter {
        SlidingWindowCounterLimiter::new(max, secs, Arc::new(LocalStore::new()))
    }

    #[tokio::test]
    async fn name_is_correct() {
        assert_eq!(limiter(5, 60).name(), "sliding_window_counter");
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
        assert!(d.retry_after_ms > 0);
    }

    #[tokio::test]
    async fn clients_are_isolated() {
        let l = limiter(2, 60);
        l.check("a", "r", 2).await;
        assert!(l.check("b", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn cost_greater_than_one_is_counted_correctly() {
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
