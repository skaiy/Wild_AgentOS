//! DAG 执行引擎
//!
//! 使用 petgraph 拓扑排序执行工作流节点，支持条件分支、重试和并行 fan-in。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::definition::*;
use super::loader::*;
use crate::core::agent_instance::{AgentInstance, AgentRole};
use crate::core::agent_runner::{AgentRunner, TaskContext};

/// DAG 执行引擎
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

    /// 执行整个工作流 DAG
    pub async fn execute(
        &self,
        dag: &WorkflowDag,
        task_iri: &str,
        user_input: &str,
    ) -> Result<Vec<NodeResult>, String> {
        if has_cycle(dag) {
            return Err("工作流包含环，无法执行".to_string());
        }

        let order = topological_order(dag)?;
        let mut completed: HashMap<String, NodeResult> = HashMap::new();
        let mut results = Vec::new();

        // 按拓扑序依次执行
        for node_idx in &order {
            let node_def = &dag.graph[*node_idx].def;

            // 跳过入口条件检查：拓扑排序保证依赖已就绪
            if !all_dependencies_met(dag, *node_idx, &completed) {
                warn!(node = %node_def.id, "依赖未就绪，跳过");
                continue;
            }

            let objective = Self::build_objective(node_def, &completed, user_input);
            let role = Self::parse_role(&node_def.agent_role);

            let context = TaskContext::new(task_iri, &objective, self.max_iterations)
                .with_original_task(user_input);

            info!(
                node = %node_def.id,
                role = %node_def.agent_role,
                "执行 DAG 节点"
            );

            // 执行节点（支持重试）
            let mut last_error: Option<String> = None;
            let mut node_result = None;
            let max_attempts = std::cmp::max(1, node_def.retry_count + 1);

            for attempt in 1..=max_attempts {
                if attempt > 1 {
                    info!(
                        node = %node_def.id,
                        attempt,
                        "重试节点"
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
                        warn!(node = %node_def.id, attempt, error = %e, "节点执行失败");
                    }
                }
            }

            let result = match node_result {
                Some(r) => {
                    completed.insert(node_def.id.clone(), r.clone());
                    r
                }
                None => {
                    let err_msg = last_error.unwrap_or_else(|| "未知错误".to_string());
                    let nr = NodeResult {
                        node_id: node_def.id.clone(),
                        status: "failed".to_string(),
                        summary: format!("节点 {} 执行失败: {}", node_def.id, err_msg),
                        archive_iri: None,
                        turn_count: 0,
                        tool_call_count: 0,
                        error: Some(err_msg),
                        output: None,
                        artifacts: vec![],
                    };
                    // 失败时检查条件分支
                    if let Some(branch_target) = should_branch(node_def, &nr) {
                        info!(
                            node = %node_def.id,
                            target = %branch_target,
                            "触发条件分支跳转"
                        );
                        completed.insert(node_def.id.clone(), nr.clone());
                        // 找分支目标节点是否在 order 中
                        for later_idx in order.iter().skip_while(|i| *i != node_idx) {
                            let later_def = &dag.graph[*later_idx].def;
                            if later_def.id == branch_target {
                                // 将分支目标标记为就绪
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
            "DAG 执行完成"
        );
        Ok(results)
    }

    /// 执行单个 Agent 节点
    async fn execute_node(
        &self,
        role: AgentRole,
        context: &TaskContext,
        node_def: &WorkflowNodeDef,
    ) -> Result<NodeResult, String> {
        let agent_id = format!("wf_{}_{}", node_def.id, uuid::Uuid::new_v4().hyphenated());
        let mut agent = AgentInstance::new(agent_id, role);

        let task_result = self.runner.execute(&mut agent, context.clone()).await
            .map_err(|e| format!("Agent 执行失败: {}", e))?;

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

    /// 构建节点 objective（含上游摘要）
    fn build_objective(
        node_def: &WorkflowNodeDef,
        completed: &HashMap<String, NodeResult>,
        user_input: &str,
    ) -> String {
        let mut parts = vec![node_def.objective.clone()];

        // 添加上游节点的摘要作为上下文
        let mut summaries = Vec::new();
        for (id, result) in completed {
            if let Some(iri) = &result.archive_iri {
                summaries.push(format!(
                    "[{}] {}\n如需查看完整报告，可使用 read_agent_output 查询: {}",
                    id, result.summary, iri
                ));
            } else {
                summaries.push(format!("[{}] {}", id, result.summary));
            }
        }

        if !summaries.is_empty() {
            parts.push("\n\n## 上游节点输出\n".to_string());
            parts.push(summaries.join("\n\n"));
        }

        parts.push(format!("\n\n## 用户原始任务\n{}", user_input));
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
