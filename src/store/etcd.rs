use async_trait::async_trait;
use etcd_client::{Client, Compare, CompareOp, GetOptions, Txn, TxnOp};

use super::{CasResult, StateStore};

/// Distributed state store backed by etcd — enables multi-replica rate limiting.
///
/// All keys are stored under a shared prefix so multiple services can coexist
/// in the same etcd cluster.
pub struct EtcdStore {
    client: tokio::sync::Mutex<Client>,
    prefix: String,
}

impl EtcdStore {
    pub async fn connect(endpoints: &[String], key_prefix: &str) -> anyhow::Result<Self> {
        let client = Client::connect(endpoints, None).await?;
        Ok(Self {
            client: tokio::sync::Mutex::new(client),
            prefix: key_prefix.to_string(),
        })
    }

    fn full_key(&self, key: &str) -> String {
        format!("{}/{}", self.prefix, key)
    }
}

#[async_trait]
impl StateStore for EtcdStore {
    async fn get(&self, key: &str) -> Option<(Vec<u8>, i64)> {
        let fk = self.full_key(key);
        let mut client = self.client.lock().await;
        let resp = client.get(fk, None).await.ok()?;
        let kv = resp.kvs().first()?;
        Some((kv.value().to_vec(), kv.mod_revision()))
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> i64 {
        let fk = self.full_key(key);
        let mut client = self.client.lock().await;
        client
            .put(fk, value, None)
            .await
            .ok()
            .and_then(|r| r.header().map(|h| h.revision()))
            .unwrap_or(0)
    }

    async fn cas(&self, key: &str, expected: Option<i64>, value: Vec<u8>) -> CasResult {
        let fk = self.full_key(key);
        let mut client = self.client.lock().await;

        // None → key must not exist yet (create_revision == 0).
        // Some(v) → key must have ModRevision == v.
        let condition = match expected {
            None => Compare::create_revision(fk.clone(), CompareOp::Equal, 0),
            Some(v) => Compare::mod_revision(fk.clone(), CompareOp::Equal, v),
        };

        let txn = Txn::new()
            .when(vec![condition])
            .and_then(vec![TxnOp::put(fk.clone(), value, None)]);

        match client.txn(txn).await {
            Ok(resp) if resp.succeeded() => CasResult {
                success: true,
                version: resp.header().map(|h| h.revision()).unwrap_or(0),
            },
            _ => {
                // Fetch the current version for the caller's retry loop.
                let current = client
                    .get(fk, None)
                    .await
                    .ok()
                    .and_then(|r| r.kvs().first().map(|kv| kv.mod_revision()))
                    .unwrap_or(0);
                CasResult { success: false, version: current }
            }
        }
    }

    /// Probe etcd reachability: fetch a single key with a limit of 1.
    /// Returns false if the connection is broken or the cluster is unavailable.
    async fn ping(&self) -> bool {
        let mut client = self.client.lock().await;
        client
            .get("__ping__", Some(GetOptions::new().with_limit(1)))
            .await
            .is_ok()
    }
}
