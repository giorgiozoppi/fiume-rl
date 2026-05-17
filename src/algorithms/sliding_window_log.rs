//! # Sliding Window Log
//!
//! Keeps an exact timestamp log for every request in the past window.
//! On each new request the log is pruned and the length checked.
//!
//! **Pro**: Perfectly accurate — no boundary burst.
//! **Con**: O(max_requests) state per client; heavy under sustained traffic.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    limiter::{Decision, RateLimiter},
    store::SharedStore,
};

#[derive(Serialize, Deserialize)]
struct State {
    /// Unix timestamps in milliseconds, oldest-first.
    timestamps_ms: Vec<i64>,
}

pub struct SlidingWindowLogLimiter {
    max_requests: u64,
    window_ms: i64,
    store: SharedStore,
}

impl SlidingWindowLogLimiter {
    pub fn new(max_requests: u64, window_secs: u64, store: SharedStore) -> Self {
        Self {
            max_requests,
            window_ms: (window_secs * 1_000) as i64,
            store,
        }
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
impl RateLimiter for SlidingWindowLogLimiter {
    async fn check(&self, client_id: &str, resource: &str, cost: u32) -> Decision {
        let key = format!("sliding_window_log/{}/{}", client_id, resource);
        let cost = cost as u64;

        for attempt in 0u32..5 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(2u64.pow(attempt))).await;
            }

            let now = now_ms();
            let cutoff = now - self.window_ms;

            let (mut state, version) = match self.store.get(&key).await {
                Some((bytes, v)) => {
                    let s = serde_json::from_slice::<State>(&bytes)
                        .unwrap_or(State { timestamps_ms: Vec::new() });
                    (s, Some(v))
                }
                None => (State { timestamps_ms: Vec::new() }, None),
            };

            // Evict timestamps outside the window.
            state.timestamps_ms.retain(|&t| t > cutoff);
            let current = state.timestamps_ms.len() as u64;

            let (allowed, retry_after_ms) = if current + cost <= self.max_requests {
                for _ in 0..cost {
                    state.timestamps_ms.push(now);
                }
                (true, 0i64)
            } else {
                let oldest = state.timestamps_ms.first().copied().unwrap_or(now);
                let wait_ms = (oldest + self.window_ms - now).max(0);
                (false, wait_ms)
            };

            let bytes = serde_json::to_vec(&state).unwrap();

            if self.store.cas(&key, version, bytes).await.success {
                return Decision {
                    allowed,
                    limit: self.max_requests as i64,
                    remaining: (self.max_requests.saturating_sub(current + if allowed { cost } else { 0 })) as i64,
                    retry_after_ms,
                    reason: if allowed {
                        format!("ok — {}/{} in window", current + cost, self.max_requests)
                    } else {
                        format!(
                            "sliding-log limit {}/{} exceeded, retry in {}ms",
                            current, self.max_requests, retry_after_ms
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
        "sliding_window_log"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::local::LocalStore;

    fn limiter(max: u64, secs: u64) -> SlidingWindowLogLimiter {
        SlidingWindowLogLimiter::new(max, secs, Arc::new(LocalStore::new()))
    }

    #[tokio::test]
    async fn name_is_correct() {
        assert_eq!(limiter(5, 1).name(), "sliding_window_log");
    }

    #[tokio::test]
    async fn allows_up_to_max_requests() {
        let l = limiter(4, 60);
        for i in 0..4 {
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
    async fn old_entries_expire_and_allow_new_requests() {
        let l = limiter(2, 1);
        l.check("c1", "r", 2).await;
        assert!(!l.check("c1", "r", 1).await.allowed);
        tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;
        assert!(l.check("c1", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn clients_are_isolated() {
        let l = limiter(2, 60);
        l.check("a", "r", 2).await;
        assert!(l.check("b", "r", 1).await.allowed);
    }

    #[tokio::test]
    async fn cost_greater_than_one_is_logged_correctly() {
        let l = limiter(10, 60);
        let d = l.check("c1", "r", 6).await;
        assert!(d.allowed);
        assert_eq!(d.remaining, 4);
    }

    #[tokio::test]
    async fn remaining_decreases_correctly() {
        let l = limiter(10, 60);
        let d1 = l.check("c1", "r", 1).await;
        let d2 = l.check("c1", "r", 1).await;
        assert_eq!(d1.remaining, 9);
        assert_eq!(d2.remaining, 8);
    }
}
