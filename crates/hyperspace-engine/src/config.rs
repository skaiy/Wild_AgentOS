//! HyperspaceEngine configuration with serde deserialize support.
//!
//! Can be loaded from TOML / JSON / YAML via the `config` crate.

use serde::{Deserialize, Serialize};

use crate::hnsw::HnswConfig;
use crate::hyper_vector::MetricKind;
use crate::wal::WalSyncMode;

/// Top-level engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperspaceEngineConfig {
    /// Data directory for WAL, snapshots, and vector storage.
    pub data_path: String,

    /// Runtime metric kind: cosine | poincare | lorentz | euclidean.
    #[serde(default = "default_metric")]
    pub metric: MetricKind,

    /// WAL sync mode: strict | batch | async.
    #[serde(default = "default_wal_sync")]
    pub wal_sync: WalSyncMode,

    /// Memory cache capacity in number of entries.
    #[serde(default = "default_cache_capacity")]
    pub cache_capacity: usize,

    /// HNSW index parameters.
    #[serde(default)]
    pub hnsw: HnswConfig,

    /// Whether to enable fast-routing (Klein chord distance for Poincaré).
    #[serde(default)]
    pub fast_routing: bool,

    /// Checkpoint interval in milliseconds (auto-checkpoint tick).
    #[serde(default = "default_checkpoint_interval")]
    pub checkpoint_interval_ms: u64,
}

impl Default for HyperspaceEngineConfig {
    fn default() -> Self {
        Self {
            data_path: "./data/hyperspace".into(),
            metric: default_metric(),
            wal_sync: default_wal_sync(),
            cache_capacity: default_cache_capacity(),
            hnsw: HnswConfig::default(),
            fast_routing: false,
            checkpoint_interval_ms: default_checkpoint_interval(),
        }
    }
}

fn default_metric() -> MetricKind {
    MetricKind::Cosine
}

fn default_wal_sync() -> WalSyncMode {
    WalSyncMode::Batch { interval_ms: 100 }
}

fn default_cache_capacity() -> usize {
    100_000
}

fn default_checkpoint_interval() -> u64 {
    30_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let cfg = HyperspaceEngineConfig::default();
        assert_eq!(cfg.metric, MetricKind::Cosine);
        assert_eq!(cfg.hnsw.ef_construction, 200);
        assert_eq!(cfg.checkpoint_interval_ms, 30_000);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let cfg = HyperspaceEngineConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: HyperspaceEngineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.metric, cfg.metric);
        assert_eq!(restored.hnsw.m, cfg.hnsw.m);
        assert_eq!(restored.wal_sync, cfg.wal_sync);
    }

    #[test]
    fn test_config_deserialize_partial() {
        let json = r#"{"data_path": "/tmp/test"}"#;
        let cfg: HyperspaceEngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.data_path, "/tmp/test");
        // Other fields should use defaults
        assert_eq!(cfg.metric, MetricKind::Cosine);
        assert_eq!(cfg.cache_capacity, 100_000);
    }
}
