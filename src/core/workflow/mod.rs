//! JSON-LD DAG workflow types and loader
//!
//! Provides JSON-LD-defined workflow definition types and DAG construction utilities.
//! Execution happens in `supervisor_agent::execute_plan()` via the SA execution engine.
//!
//! ## Architecture
//!
//! - `definition.rs` — JSON-LD serialization types (Web UI edit/save format)
//! - `loader.rs` — Parse JSON-LD → petgraph DiGraph, cycle detection + topological sort
//! - `adapter.rs` — ExecutionPlan → DAG adapter, unified execution path

pub mod adapter;
pub mod definition;
pub mod loader;

pub use definition::*;
pub use loader::{build_dag, has_cycle, load_workflow_jsonld, topological_order, WorkflowDag};
