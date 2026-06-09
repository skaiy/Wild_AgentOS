//! ExecutionPlan → DAG 适配器
//!
//! 将 SA 现有的 ExecutionPlan 线性序列自动转换为 DAG 内部表示，
//! 使得新旧两种流程定义方式共享同一个 DAG 执行引擎。

use super::definition::*;
use super::loader::*;
use crate::core::agent_instance::AgentRole;
use crate::core::sa::ExecutionPlan;

/// 将 ExecutionPlan 转换为 WorkflowDefinition（可被 DAG 引擎执行）
pub fn plan_to_workflow(plan: &ExecutionPlan, task_iri: &str) -> WorkflowDefinition {
    let plan_id = &plan.plan_id;

    let mut nodes = Vec::new();
    let mut prev_id: Option<String> = None;

    for step in &plan.steps {
        let node_id = format!("wf:{}/{}", plan_id, step.step_id);

        let mut node = WorkflowNodeDef {
            id: node_id.clone(),
            node_type: "AgentNode".to_string(),
            agent_role: format!("{:?}", step.role),
            objective: step.objective.clone(),
            next: None,
            next_nodes: vec![],
            dependencies: step.dependencies.clone(),
            tools: step.tools_allowed.clone(),
            success_criteria: step.success_criteria.clone(),
            input_mapping: None,
            branch_on_failure: None,
            retry_count: 0,
            retry_delay_secs: 0,
            timeout_secs: 0,
            final_node: false,
            extra: Default::default(),
        };

        // 串联线性步骤为 next 链
        if let Some(ref pid) = prev_id {
            // 找到前驱节点设置 next
            if let Some(prev_node) = nodes.iter_mut().find(|n: &&mut WorkflowNodeDef| n.id == *pid) {
                prev_node.next = Some(node_id.clone());
            }
            // 当前节点依赖前驱
            if !node.dependencies.contains(pid) {
                node.dependencies.push(pid.clone());
            }
        }
        prev_id = Some(node_id);
        nodes.push(node);
    }

    // 标记最后一个为 final
    if let Some(last) = nodes.last_mut() {
        last.final_node = true;
    }

    let entry_node = nodes.first()
        .map(|n| n.id.clone())
        .unwrap_or_default();

    // 处理并行组
    let parallel_updates: Vec<(String, Vec<String>)> = plan.parallel_groups.iter()
        .filter(|g| g.len() > 1)
        .filter_map(|group| {
            let role_strs: Vec<String> = group.iter()
                .map(|r| format!("{:?}", r))
                .collect();
            let first_idx = nodes.iter().position(|n| role_strs.contains(&n.agent_role))?;
            if first_idx == 0 { return None; }
            let prev_id = nodes[first_idx - 1].id.clone();
            let parallel_ids: Vec<String> = nodes[first_idx..].iter()
                .filter(|n| role_strs.contains(&n.agent_role))
                .map(|n| n.id.clone())
                .collect();
            if parallel_ids.is_empty() { None }
            else { Some((prev_id, parallel_ids)) }
        })
        .collect();

    for (prev_id, parallel_ids) in parallel_updates {
        if let Some(prev_node) = nodes.iter_mut().find(|n| n.id == prev_id) {
            prev_node.next_nodes = parallel_ids;
            prev_node.next = None;
        }
    }

    WorkflowDefinition {
        id: format!("iri://workflow/{}", plan_id),
        name: plan.description.clone(),
        description: format!("从 ExecutionPlan '{}' 自动转换", plan.description),
        version: "1.0".to_string(),
        entry_node,
        nodes,
    }
}

/// 快速判断：ExecutionPlan 是否可被 DAG 引擎执行（总是可以，因 adapter）
pub fn is_plan_compatible(_plan: &ExecutionPlan) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sa::{ExecutionPlan, PlanStep};

    #[test]
    fn test_plan_to_workflow_linear() {
        let plan = ExecutionPlan {
            plan_id: "test_001".to_string(),
            agent_sequence: vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act],
            parallel_groups: vec![],
            task_complexity: crate::core::sa::TaskComplexity::Standard,
            description: "测试计划".to_string(),
            steps: vec![
                PlanStep {
                    step_id: "step_1".to_string(),
                    role: AgentRole::Plan,
                    objective: "制定计划".to_string(),
                    expected_output: "plan".to_string(),
                    dependencies: vec![],
                    tools_allowed: vec!["file_read".to_string()],
                    success_criteria: "计划完整".to_string(),
                },
                PlanStep {
                    step_id: "step_2".to_string(),
                    role: AgentRole::Do,
                    objective: "执行任务".to_string(),
                    expected_output: "output".to_string(),
                    dependencies: vec!["step_1".to_string()],
                    tools_allowed: vec!["file_write".to_string(), "bash".to_string()],
                    success_criteria: "产物完整".to_string(),
                },
            ],
            context_requirements: Default::default(),
            success_metrics: vec![],
            max_recursion_depth: 0,
            sub_tasks: vec![],
        };

        let wf = plan_to_workflow(&plan, "iri://task/test_task");
        assert_eq!(wf.nodes.len(), 2);
        assert_eq!(wf.entry_node, "wf:test_001/step_1");
        assert_eq!(wf.nodes[0].next.as_deref(), Some("wf:test_001/step_2"));
        assert!(wf.nodes[1].final_node);
    }

    #[test]
    fn test_plan_to_workflow_parallel() {
        let plan = ExecutionPlan {
            plan_id: "test_002".to_string(),
            agent_sequence: vec![AgentRole::Do, AgentRole::Check],
            parallel_groups: vec![vec![AgentRole::Do, AgentRole::Do]],
            task_complexity: crate::core::sa::TaskComplexity::Standard,
            description: "并行测试".to_string(),
            steps: vec![
                PlanStep {
                    step_id: "step_1".to_string(),
                    role: AgentRole::Do,
                    objective: "模块A".to_string(),
                    expected_output: "a".to_string(),
                    dependencies: vec![],
                    tools_allowed: vec![],
                    success_criteria: "".to_string(),
                },
                PlanStep {
                    step_id: "step_2".to_string(),
                    role: AgentRole::Do,
                    objective: "模块B".to_string(),
                    expected_output: "b".to_string(),
                    dependencies: vec![],
                    tools_allowed: vec![],
                    success_criteria: "".to_string(),
                },
            ],
            context_requirements: Default::default(),
            success_metrics: vec![],
            max_recursion_depth: 0,
            sub_tasks: vec![],
        };

        let wf = plan_to_workflow(&plan, "iri://task/test2");
        // 两个 Do 节点并行：无 entry 的 next，应为 next_nodes
        assert_eq!(wf.nodes.len(), 2);
    }
}
