//! Causal Engine — Root Cause Analysis for the Skill Graph.
//!
//! Given observed error events, traces backwards through the skill dependency
//! graph via Bayesian inference to identify the most probable root cause skill(s).
//!
//! # Architecture
//!
//! ```text
//! CausalEngine
//!   ├── CausalModelStore — persistence + query of causal data
//!   └── Inference pipeline
//!         ├── 1. Subgraph extraction (reverse traversal from observed skills)
//!         ├── 2. Bayesian posterior computation
//!         ├── 3. Path reconstruction (most-likely propagation)
//!         └── 4. Ranking by confidence
//! ```

pub mod engine;
pub mod fused;
pub mod store;
pub mod types;

pub use engine::CausalEngine;
pub use fused::FusedRootCauseEngine;
pub use store::CausalModelStore;
pub use types::{CausalInference, CausalObservation};
