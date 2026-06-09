//! JSON-LD DAG 工作流引擎
//!
//! 提供可选的 JSON-LD 定义的工作流执行能力，与现有 SA ExecutionPlan 并存。
//!
//! ## 架构
//!
//! - `definition.rs` — JSON-LD 序列化类型（Web UI 编辑/保存的格式）
//! - `loader.rs` — 解析 JSON-LD → petgraph DiGraph，含环检测 + 拓扑排序
//! - `engine.rs` — DAG 执行引擎，条件分支 + 重试
//! - `adapter.rs` — ExecutionPlan → DAG 转换器，统一执行路径

pub mod adapter;
pub mod definition;
pub mod engine;
pub mod loader;

pub use definition::*;
pub use engine::DagEngine;
pub use loader::{build_dag, has_cycle, load_workflow_jsonld, topological_order, WorkflowDag};
