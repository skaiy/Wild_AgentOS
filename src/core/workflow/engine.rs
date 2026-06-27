//! DAG execution engine
//!
//! Executes workflow nodes using petgraph topological sort, supports conditional branching, retry, and parallel fan-in.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::definition::*;
use super::loader::*;
use crate::core::agent_instance::{AgentInstance, AgentRole};
use crate::core::agent_runner::{AgentRunner, TaskContext};

/// DAG execution engine
pub struct DagEngine {
    runner: Arc<AgentRunner>,
    max_iterations: u32,
}

impl DagEngine {
    pub fn new(runner: Arc<AgentRunner>, max_iterations: u32) -> Self {
        Self {
            runner,
            max_iterations,
        }
    }

    /// Execute the entire workflow DAG
    pub async fn execute(
        &self,
        dag: &WorkflowDag,
        task_iri: &str,
        user_input: &str,
    ) -> Result<Vec<NodeResult>, String> {
        if has_cycle(dag) {
            return Err("Workflow contains a cycle, cannot execute".to_string());
        }

        let order = topological_order(dag)?;
        let mut completed: HashMap<String, NodeResult> = HashMap::new();
        let mut results = Vec::new();

        // execute in topological order
        for node_idx in &order {
            let node_def = &dag.graph[*node_idx].def;

            // Skip entry condition check: topological sort guarantees dependencies are ready
            if !all_dependencies_met(dag, *node_idx, &completed) {
                warn!(node = %node_def.id, "Dependencies not ready, skipping");
                continue;
            }

            let objective = Self::build_objective(node_def, &completed, user_input);
            let role = Self::parse_role(&node_def.agent_role);

            let context = TaskContext::new(task_iri, &objective, self.max_iterations)
                .with_original_task(user_input);

            info!(
                node = %node_def.id,
                role = %node_def.agent_role,
                "Executing DAG node"
            );

            // Execute node (with retry support)
            let mut last_error: Option<String> = None;
            let mut node_result = None;
            let max_attempts = std::cmp::max(1, node_def.retry_count + 1);

            for attempt in 1..=max_attempts {
                if attempt > 1 {
                    info!(
                        node = %node_def.id,
                        attempt,
                        "Retrying node"
                    );
                    if node_def.retry_delay_secs > 0 {
                        sleep(Duration::from_secs(node_def.retry_delay_secs)).await;
                    }
                }

                match self.execute_node(role, &context, node_def).await {
                    Ok(result) => {
                        node_result = Some(result);
                        last_error = None;
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e.clone());
                        warn!(node = %node_def.id, attempt, error = %e, "Node execution failed");
                    }
                }
            }

            let result = match node_result {
                Some(r) => {
                    completed.insert(node_def.id.clone(), r.clone());
                    r
                }
                None => {
                    let err_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
                    let nr = NodeResult {
                        node_id: node_def.id.clone(),
                        status: "failed".to_string(),
                        summary: format!("Node {} execution failed: {}", node_def.id, err_msg),
                        archive_iri: None,
                        turn_count: 0,
                        tool_call_count: 0,
                        error: Some(err_msg),
                        output: None,
                        artifacts: vec![],
                    };
                    // Check conditional branch on failure
                    if let Some(branch_target) = should_branch(node_def, &nr) {
                        info!(
                            node = %node_def.id,
                            target = %branch_target,
                            "Triggered conditional branch jump"
                        );
                        completed.insert(node_def.id.clone(), nr.clone());
                        // Find if branch target node is in order
                        for later_idx in order.iter().skip_while(|i| *i != node_idx) {
                            let later_def = &dag.graph[*later_idx].def;
                            if later_def.id == branch_target {
                                // Mark branch target as ready
                                break;
                            }
                        }
                    }
                    nr
                }
            };

            results.push(result);
        }

        info!(
            total = results.len(),
            "DAG execution completed"
        );
        Ok(results)
    }

    /// Execute a single Agent node
    async fn execute_node(
        &self,
        role: AgentRole,
        context: &TaskContext,
        node_def: &WorkflowNodeDef,
    ) -> Result<NodeResult, String> {
        let agent_id = format!("wf_{}_{}", node_def.id, uuid::Uuid::new_v4().hyphenated());
        let mut agent = AgentInstance::new(agent_id, role);

        let task_result = self.runner.execute(&mut agent, context.clone()).await
            .map_err(|e| format!("Agent execution failed: {}", e))?;

        Ok(NodeResult {
            node_id: node_def.id.clone(),
            status: task_result.status.clone(),
            summary: task_result.summary.clone(),
            archive_iri: task_result.archive_iri.clone(),
            turn_count: task_result.turn_count,
            tool_call_count: task_result.tool_call_count,
            error: task_result.errors.first().cloned(),
            output: task_result.output,
            artifacts: task_result.artifacts,
        })
    }

    /// Build node objective (with upstream summary)
    fn build_objective(
        node_def: &WorkflowNodeDef,
        completed: &HashMap<String, NodeResult>,
        user_input: &str,
    ) -> String {
        let mut parts = vec![node_def.objective.clone()];

        // Add upstream node summaries as context
        let mut summaries = Vec::new();
        for (id, result) in completed {
            if let Some(iri) = &result.archive_iri {
                summaries.push(format!(
                    "[{}] {}\nFor full report, use read_agent_output: {}",
                    id, result.summary, iri
                ));
            } else {
                summaries.push(format!("[{}] {}", id, result.summary));
            }
        }

        if !summaries.is_empty() {
            parts.push("\n\n## Upstream Node Output\n".to_string());
            parts.push(summaries.join("\n\n"));
        }

        parts.push(format!("\n\n## Original User Task\n{}", user_input));
        parts.join("")
    }

    fn parse_role(role_str: &str) -> AgentRole {
        match role_str.to_lowercase().as_str() {
            "plan" | "pa" => AgentRole::Plan,
            "do" | "da" | "executor" => AgentRole::Do,
            "check" | "ca" | "reviewer" => AgentRole::Check,
            "act" | "aa" | "decision" => AgentRole::Act,
            _ => AgentRole::Do,
        }
    }
}
