//! Temporal — Skill Graph Timeline Versioning & Temporal Hypergraph.
//!
//! Provides:
//! - `TimelineStore`: point-in-time snapshots, rollback, and diff for the skill graph
//! - `TemporalHypergraphStore`: time-aware N-ary hyperedges with versioning + time-range queries
//!
//! # Architecture
//!
//! ```text
//! SkillGraphStore (via mutation hooks)
//!   ├── TimelineStore
//!   │     ├── Full snapshots (periodic, configurable)
//!   │     ├── Incremental mutations (between snapshots)
//!   │     └── Snapshot index (metadata for list/query)
//!   └── TemporalHypergraphStore
//!         ├── TemporalHyperedge (versioned hyperedges)
//!         └── TemporalIndex (binary-search over sorted time entries)
//! ```

pub mod timeline;
pub mod types;

pub use timeline::TimelineStore;
pub use types::{
    GraphDiff, GraphMutation, GraphSnapshot, SnapshotMeta, TemporalHyperedge,
    TemporalHypergraphStore, TemporalIndex, TemporalIndexEntry, TemporalVersion, TimeInterval,
};
