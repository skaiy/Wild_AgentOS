//! JSON-LD DAG workflow engine
//!
//! Provides optional JSON-LD-defined workflow execution, co-existing with SA ExecutionPlan.
//!
//! ## Architecture
//!
//! - `definition.rs` — JSON-LD serialization types (Web UI edit/save format)
//! - `loader.rs` — Parse JSON-LD → petgraph DiGraph, cycle detection + topological sort
//! - `engine.rs` — DAG execution engine, conditional branches + retry
//! - `adapter.rs` — ExecutionPlan → DAG adapter, unified execution path

pub mod adapter;
pub mod definition;
pub mod engine;
pub mod loader;

pub use definition::*;
pub use engine::DagEngine;
pub use loader::{build_dag, has_cycle, load_workflow_jsonld, topological_order, WorkflowDag};
