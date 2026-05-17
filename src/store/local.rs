use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex,
    },
};

use async_trait::async_trait;

use super::{CasResult, StateStore};

/// In-process store backed by a `Mutex<HashMap>` — used for single-node deployments.
pub struct LocalStore {
    state: Mutex<HashMap<String, (Vec<u8>, i64)>>,
    counter: AtomicI64,
}

impl LocalStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            counter: AtomicI64::new(1),
        }
    }

    fn next_version(&self) -> i64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl StateStore for LocalStore {
    async fn get(&self, key: &str) -> Option<(Vec<u8>, i64)> {
        self.state.lock().unwrap().get(key).cloned()
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> i64 {
        let v = self.next_version();
        self.state.lock().unwrap().insert(key.to_string(), (value, v));
        v
    }

    async fn cas(&self, key: &str, expected: Option<i64>, value: Vec<u8>) -> CasResult {
        let mut map = self.state.lock().unwrap();
        let current = map.get(key);

        let matches = match (expected, current) {
            (None, None) => true,
            (Some(ev), Some((_, cv))) => ev == *cv,
            _ => false,
        };

        if matches {
            let v = self.counter.fetch_add(1, Ordering::SeqCst);
            map.insert(key.to_string(), (value, v));
            CasResult { success: true, version: v }
        } else {
            let current_version = current.map(|(_, v)| *v).unwrap_or(0);
            CasResult { success: false, version: current_version }
        }
    }
}
