pub mod etcd;
pub mod local;

use std::sync::Arc;

use async_trait::async_trait;

#[derive(Debug)]
pub struct CasResult {
    pub success: bool,
    /// Version after a successful put, or the current version on conflict.
    pub version: i64,
}

/// Versioned key-value store shared across algorithm instances.
///
/// Every value is paired with a monotone `version`.  CAS prevents lost-update
/// races when multiple server replicas check the same client concurrently.
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Return `(value_bytes, version)`, or `None` if the key is absent.
    async fn get(&self, key: &str) -> Option<(Vec<u8>, i64)>;

    /// Unconditional write; returns the new version.
    async fn put(&self, key: &str, value: Vec<u8>) -> i64;

    /// Atomic compare-and-swap.
    ///
    /// - `expected = None`    → succeed only if the key does not yet exist
    /// - `expected = Some(v)` → succeed only if the current version equals `v`
    async fn cas(&self, key: &str, expected: Option<i64>, value: Vec<u8>) -> CasResult;

    /// Lightweight connectivity check used by the readiness probe.
    ///
    /// Returns `true` if the store can service requests.  The default
    /// implementation always returns `true` (correct for `LocalStore`);
    /// `EtcdStore` overrides this to verify the etcd connection is alive.
    async fn ping(&self) -> bool {
        true
    }
}

pub type SharedStore = Arc<dyn StateStore>;
