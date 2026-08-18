use std::sync::{Arc, RwLock};

use chrono::Utc;

use crate::graph::local_store::TripleData;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IRIDelta {
    pub added: Vec<TripleData>,
    pub removed: Vec<TripleData>,
    pub timestamp: String,
    pub version: u64,
}

struct Inner {
    added: Vec<TripleData>,
    removed: Vec<TripleData>,
    version: u64,
}

pub struct DeltaTracker {
    inner: Arc<RwLock<Inner>>,
}

impl DeltaTracker {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                added: Vec::new(),
                removed: Vec::new(),
                version: 0,
            })),
        }
    }

    pub fn track_add(&self, triple: TripleData) {
        if let Ok(mut guard) = self.inner.write() {
            guard.added.push(triple);
        }
    }

    pub fn track_remove(&self, triple: TripleData) {
        if let Ok(mut guard) = self.inner.write() {
            guard.removed.push(triple);
        }
    }

    pub fn snapshot(&self) -> IRIDelta {
        if let Ok(mut guard) = self.inner.write() {
            guard.version += 1;
            IRIDelta {
                added: guard.added.clone(),
                removed: guard.removed.clone(),
                timestamp: Utc::now().to_rfc3339(),
                version: guard.version,
            }
        } else {
            IRIDelta {
                added: Vec::new(),
                removed: Vec::new(),
                timestamp: Utc::now().to_rfc3339(),
                version: 0,
            }
        }
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.added.clear();
            guard.removed.clear();
        }
    }
}

impl Default for DeltaTracker {
    fn default() -> Self {
        Self::new()
    }
}