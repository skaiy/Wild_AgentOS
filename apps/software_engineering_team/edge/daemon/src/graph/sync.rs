use std::sync::Arc;
use std::sync::RwLock;

use crate::graph::delta::{DeltaTracker, IRIDelta};
use crate::graph::local_store::{LocalGraphStore, TripleData};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PushResult {
    pub accepted: bool,
    pub version: u64,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PushRequestBody {
    delta: IRIDelta,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ContextResponseBody {
    triples: Vec<TripleData>,
}

pub struct GraphSyncer {
    store: Arc<LocalGraphStore>,
    delta: Arc<RwLock<DeltaTracker>>,
    center_url: String,
    auth_token: String,
    client: reqwest::Client,
}

impl GraphSyncer {
    pub fn new(
        store: Arc<LocalGraphStore>,
        delta: Arc<RwLock<DeltaTracker>>,
        center_url: String,
        auth_token: String,
    ) -> Self {
        let client = reqwest::Client::new();
        Self {
            store,
            delta,
            center_url,
            auth_token,
            client,
        }
    }

    pub async fn push_delta(&self) -> anyhow::Result<PushResult> {
        let delta = {
            let guard = self.delta.read().map_err(|e| anyhow::anyhow!("lock error: {}", e))?;
            guard.snapshot()
        };
        let body = PushRequestBody { delta: delta.clone() };
        let url = format!("{}/api/v1/graph/sync", self.center_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await?;
        let result: PushResult = resp.json().await?;
        if result.accepted {
            if let Ok(guard) = self.delta.read() {
                guard.clear();
            }
        }
        Ok(result)
    }

    pub async fn pull_context(&self, iris: Vec<String>) -> anyhow::Result<()> {
        let mut url = format!("{}/api/v1/graph/context?", self.center_url);
        for iri in &iris {
            url.push_str("iri=");
            url.push_str(&urlencoding(&iri));
            url.push('&');
        }
        url.pop();
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await?;
        let body: ContextResponseBody = resp.json().await?;
        self.store.insert_triples(body.triples).await?;
        Ok(())
    }
}

fn urlencoding(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}