#![allow(dead_code)]

pub mod core;
pub mod gateway;
pub mod memory;
pub mod perception;
pub mod tools;
pub mod llm;
pub mod templates;
pub mod api;
pub mod utils;
pub mod config;
pub mod permissions;
pub mod jsonld;
pub mod skill_graph;
pub mod worker;
pub mod batch;
pub mod methodology;
pub mod knowledge_graph;
pub mod root_cause;

pub mod causal;
pub mod temporal;
pub mod gnn;
pub mod graph_backend;

#[cfg(feature = "ontology")]
pub mod ontology;

/// Bridge types for ontology embedding storage (OntologyEmbedStore, HyperspaceEmbedStore,
/// OntologySearchBridge).  This module was moved out of crates/hyperspace-engine/src/open_ontologies.rs
/// because it bridges two independent subsystems; it belongs at the application level.
#[cfg(feature = "ontology")]
pub mod ontology_bridge;

pub use core::{
    AgentRunner, AgentInstance, SupervisorAgent,
    agent_runner::{TaskContext, TaskResult},
    agent_instance::{AgentRole, AgentStatus},
    sa::{ExecutionPlan, TaskComplexity, CyclePhase, CycleState, PlanStep},
    CoreError, CoreConfig,
};
pub use gateway::UnifiedGateway;
pub use memory::{L0Store, L1Session, Blackboard, ProjectionEngine};
pub use tools::SkillRegistry;
pub use config::Settings;
pub use jsonld::JsonLdContext;
