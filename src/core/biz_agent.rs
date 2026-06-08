use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::core::agent_instance::{AgentInstance, AgentRole, AgentStatus};
use crate::core::agent_runner::{AgentRunner, TaskContext, TaskResult};
use crate::memory::l0_store::L0Store;
use crate::memory::l1_session::L1Session;
use crate::memory::memory_manager::MemoryManager;
use crate::CoreError;

/// Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// 最大子 Agent 数量
    pub max_sub_agents: usize,
    /// 最大 LLM 调用迭代次数
    pub max_iterations: u32,
    /// 是否使用编排模式（分解 + 子 Agent）
    pub orchestrator_mode: bool,
    /// 子 Agent 是否并行执行
    pub parallel_sub_agents: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_sub_agents: 5,
            max_iterations: 10,
            orchestrator_mode: true,
            parallel_sub_agents: true,
        }
    }
}

/// 统一 BizAgent — PA/DA/CA/AA 共用同一 Agent 类
///
/// 架构:
/// - SA 创建 agent.md (提示词) 并启动一个 BizAgent
/// - BizAgent.execute() 以两种模式之一运行:
///   - MONO: 直接 LLM 调用 + 工具
///   - ORCHESTRATOR: 分解 → 生成子 BizAgent (同角色) → 聚合
/// - 每个角色有不同的分解/聚合逻辑
/// - 子 Agent 数量受 AgentConfig.max_sub_agents 限制
pub struct BizAgent {
    pub instance: AgentInstance,
    pub agent_md: String,
    pub config: AgentConfig,
    runner: Arc<AgentRunner>,
    session: Option<L1Session>,
    sub_agents: Vec<BizAgent>,
    sub_results: Vec<TaskResult>,
}

impl BizAgent {
    pub fn new(
        agent_id: String,
        role: AgentRole,
        agent_md: &str,
        runner: Arc<AgentRunner>,
        config: AgentConfig,
    ) -> Self {
        Self {
            instance: AgentInstance::new(agent_id, role),
            agent_md: agent_md.to_string(),
            config,
            runner,
            session: None,
            sub_agents: Vec::new(),
            sub_results: Vec::new(),
        }
    }

    pub fn agent_id(&self) -> &str { &self.instance.agent_id }
    pub fn role(&self) -> AgentRole { self.instance.role }
    pub fn status(&self) -> &AgentStatus { &self.instance.status }

    /// 主入口：执行任务。
    /// 编排模式下委托给 分解→子 Agent→聚合。
    pub async fn execute(&mut self, context: TaskContext) -> TaskResult {
        self.instance.status = AgentStatus::Running;
        info!(agent = %self.agent_id(), role = %self.role(), "BizAgent start");

        let task_iri = context.task_iri.clone();
        let mut session = {
            let mut mm = self.runner.memory_manager.lock().await;
            mm.create_session(self.agent_id(), &self.role().to_string(), &context.task_iri)
        };

        let result = if self.config.orchestrator_mode && self.should_decompose(&context) {
            self.execute_orchestrator(context, &mut session).await
        } else {
            self.execute_mono(context).await
        };

        {
            let mut mm = self.runner.memory_manager.lock().await;
            let _ = mm.finalize_session(session, &task_iri);
        }

        self.instance.status = AgentStatus::Completed;
        result
    }

    // ========== MONO 模式 ==========

    /// 单 Agent 直接执行，委托给 AgentRunner.execute()。
    /// AgentRunner.execute() 内部会创建并管理 L1 session。
    async fn execute_mono(&self, context: TaskContext) -> TaskResult {
        let result: Result<TaskResult, CoreError> = self.runner.execute(
            &mut self.instance.clone(),
            context,
        ).await;

        match result {
            Ok(r) => r,
            Err(e) => TaskResult {
                task_iri: String::new(),
                status: "failed".to_string(),
                summary: e.to_string(),
                output: None,
                jsonld_output: None,
                artifacts: Vec::new(),
                errors: vec![e.to_string()],
                turn_count: 0,
                tool_call_count: 0,
                five_w2h_updates: None,
                tracked_actions: Vec::new(),
                archive_iri: None,
            },
        }
    }

    // ========== ORCHESTRATOR 模式 ==========

    fn should_decompose(&self, _context: &TaskContext) -> bool {
        if self.config.max_sub_agents == 0 {
            return false;
        }
        if !self.config.orchestrator_mode {
            return false;
        }
        true
    }

    async fn execute_orchestrator(
        &mut self,
        context: TaskContext,
        session: &mut L1Session,
    ) -> TaskResult {
        let sub_tasks = self.decompose(&context).await;
        let sub_count = sub_tasks.len().min(self.config.max_sub_agents);

        if sub_count == 0 {
            return self.execute_mono(context).await;
        }

        info!(agent = %self.agent_id(), sub_count = sub_count, "分解任务");

        let sub_contexts: Vec<TaskContext> = sub_tasks
            .into_iter()
            .take(sub_count)
            .enumerate()
            .map(|(i, ctx)| TaskContext {
                objective: format!("[Sub-{}#{}] {}", self.role(), i, ctx.objective),
                ..ctx
            })
            .collect();

        self.sub_results.clear();

        if self.config.parallel_sub_agents {
            let mut handles = Vec::new();

            for (i, sub_ctx) in sub_contexts.into_iter().enumerate() {
                let sub_id = format!("{}_sub_{}", self.agent_id(), i);
                let runner = self.runner.clone();
                let agent_md = self.agent_md.clone();
                let config = AgentConfig { orchestrator_mode: false, ..self.config.clone() };
                let role = self.role();

                let handle = tokio::spawn(async move {
                    let sub = BizAgent::new(
                        sub_id,
                        role,
                        &agent_md,
                        runner,
                        config,
                    );
                    sub.execute_mono(sub_ctx).await
                });

                handles.push(handle);
            }

            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        session.add_summary("assistant", &format!("[子任务] {}", result.summary), None);
                        self.sub_results.push(result);
                    }
                    Err(e) => {
                        warn!("子 Agent 执行失败: {}", e);
                        self.sub_results.push(TaskResult {
                            task_iri: String::new(),
                            status: "failed".to_string(),
                            summary: format!("子 Agent 执行失败: {}", e),
                            output: None,
                            jsonld_output: None,
                            artifacts: Vec::new(),
                            errors: vec![e.to_string()],
                            turn_count: 0,
                            tool_call_count: 0,
                            five_w2h_updates: None,
                            tracked_actions: Vec::new(),
                            archive_iri: None,
                        });
                    }
                }
            }
        } else {
            for (i, sub_ctx) in sub_contexts.into_iter().enumerate() {
                let sub_id = format!("{}_sub_{}", self.agent_id(), i);
                let sub = BizAgent::new(
                    sub_id,
                    self.role(),
                    &self.agent_md,
                    self.runner.clone(),
                    AgentConfig { orchestrator_mode: false, ..self.config.clone() },
                );
                let result = sub.execute_mono(sub_ctx).await;
                session.add_summary("assistant", &format!("[子任务{}] {}", i, result.summary), None);
                self.sub_results.push(result);
            }
        }

        let final_result = self.aggregate(&context).await;
        session.add_summary("assistant", &final_result.summary, None);
        final_result
    }

    // ========== 角色分解 ==========

    async fn decompose(&self, context: &TaskContext) -> Vec<TaskContext> {
        match self.role() {
            AgentRole::Plan => self.decompose_plan_with_llm(context).await,
            AgentRole::Do => self.decompose_do_with_llm(context).await,
            AgentRole::Check => self.decompose_check_with_llm(context).await,
            AgentRole::Act => self.decompose_act_with_llm(context).await,
        }
    }

    async fn decompose_plan_with_llm(&self, context: &TaskContext) -> Vec<TaskContext> {
        self.decompose_with_llm(context, "planning", 
            "将任务分解为多个独立的计划子任务，每个子任务应该有明确的目标和边界").await
    }

    async fn decompose_do_with_llm(&self, context: &TaskContext) -> Vec<TaskContext> {
        self.decompose_with_llm(context, "execution",
            "将执行任务分解为多个独立的实现单元，每个单元应该可以独立完成").await
    }

    async fn decompose_check_with_llm(&self, context: &TaskContext) -> Vec<TaskContext> {
        self.decompose_with_llm(context, "verification",
            "将检查任务分解为多个独立的验证维度，如功能验证、性能验证、安全验证等").await
    }

    async fn decompose_act_with_llm(&self, context: &TaskContext) -> Vec<TaskContext> {
        self.decompose_with_llm(context, "decision",
            "将决策任务分解为多个独立的决策点，每个决策点应该有明确的选项和评估标准").await
    }

    /// 通用的 LLM 分解方法
    async fn decompose_with_llm(&self, context: &TaskContext, phase: &str, instruction: &str) -> Vec<TaskContext> {
        let prompt = format!(
            r#"你是一个任务分解专家。请将以下{}任务分解为多个独立的子任务。

## 原始任务
{}

## 分解指导
{}

## 输出要求
请以 JSON 数组格式输出分解后的子任务列表，每个子任务包含：
- "description": 子任务描述（简洁明确）
- "priority": 优先级（high/medium/low）
- "dependencies": 依赖的其他子任务编号（数组，从0开始）

示例格式：
[
  {{"description": "子任务1描述", "priority": "high", "dependencies": []}},
  {{"description": "子任务2描述", "priority": "medium", "dependencies": [0]}}
]

如果任务不需要分解（已经是原子任务），返回：
[{{"description": "原始任务", "priority": "high", "dependencies": []}}]

请直接输出 JSON 数组，不要有其他内容。"#,
            phase, context.objective, instruction
        );

        let messages = vec![
            crate::gateway::unified_gateway::ChatMessage {
                role: "user".to_string(),
                content: prompt,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            }
        ];

        let model = self.runner.gateway.get_model(&self.role().to_string().to_lowercase());
        
        match self.runner.gateway.chat_with_params(&model, messages, None, None, None, None).await {
            Ok(response) => {
                if let Some(choice) = response.choices.first() {
                    if let Some(content) = &choice.message.content {
                        return self.parse_decomposition_result(content, context);
                    }
                }
            }
            Err(e) => {
                warn!("LLM 分解失败: {}, 使用原始任务", e);
            }
        }

        vec![context.clone()]
    }

    fn parse_decomposition_result(&self, content: &str, context: &TaskContext) -> Vec<TaskContext> {
        let json_str = if content.starts_with('[') {
            content.to_string()
        } else {
            if let Some(start) = content.find('[') {
                if let Some(end) = content.rfind(']') {
                    content[start..=end].to_string()
                } else {
                    content.to_string()
                }
            } else {
                content.to_string()
            }
        };

        match serde_json::from_str::<Value>(&json_str) {
            Ok(Value::Array(tasks)) => {
                let sub_tasks: Vec<TaskContext> = tasks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, task)| {
                        let desc = task.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or(&context.objective);
                        
                        Some(TaskContext {
                            task_iri: context.task_iri.clone(),
                            objective: format!("[Sub-{}#{}] {}", self.role(), i, desc),
                            ..context.clone()
                        })
                    })
                    .collect();
                
                if sub_tasks.is_empty() {
                    vec![context.clone()]
                } else {
                    info!("LLM 分解成功: {} 个子任务", sub_tasks.len());
                    sub_tasks
                }
            }
            _ => {
                warn!("无法解析 LLM 分解结果，使用原始任务");
                vec![context.clone()]
            }
        }
    }

    // ========== 角色聚合 ==========

    async fn aggregate(&self, context: &TaskContext) -> TaskResult {
        match self.role() {
            AgentRole::Plan => self.aggregate_with_llm(context, "planning").await,
            AgentRole::Do => self.aggregate_with_llm(context, "execution").await,
            AgentRole::Check => self.aggregate_with_llm(context, "verification").await,
            AgentRole::Act => self.aggregate_with_llm(context, "decision").await,
        }
    }

    async fn aggregate_with_llm(&self, context: &TaskContext, phase: &str) -> TaskResult {
        let simple_result = self.aggregate_results(phase);
        
        if self.sub_results.len() <= 1 {
            return simple_result;
        }
        
        let sub_summaries: Vec<String> = self.sub_results
            .iter()
            .enumerate()
            .map(|(i, r)| format!("子任务{} [{}]: {}", i + 1, r.status, r.summary))
            .collect();
        
        let prompt = format!(
            r#"你是一个结果聚合专家。请总结以下{}阶段的多个子任务结果。

## 原始任务
{}

## 子任务结果
{}

## 输出要求
请以 JSON 格式输出聚合结果：
{{
  "summary": "整体结果摘要（不超过300字）",
  "key_findings": ["关键发现1", "关键发现2"],
  "recommendations": ["建议1", "建议2"],
  "overall_status": "success/partial/failed"
}}

请直接输出 JSON，不要有其他内容。"#,
            phase, context.objective, sub_summaries.join("\n")
        );

        let messages = vec![
            crate::gateway::unified_gateway::ChatMessage {
                role: "user".to_string(),
                content: prompt,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            }
        ];

        let model = self.runner.gateway.get_model(&self.role().to_string().to_lowercase());
        
        match self.runner.gateway.chat_with_params(&model, messages, None, None, None, None).await {
            Ok(response) => {
                if let Some(choice) = response.choices.first() {
                    if let Some(content) = &choice.message.content {
                        return self.parse_aggregation_result(content, &simple_result);
                    }
                }
            }
            Err(e) => {
                warn!("LLM 聚合失败: {}, 使用简单聚合", e);
            }
        }

        simple_result
    }

    fn parse_aggregation_result(&self, content: &str, fallback: &TaskResult) -> TaskResult {
        let json_str = if content.starts_with('{') {
            content.to_string()
        } else {
            if let Some(start) = content.find('{') {
                if let Some(end) = content.rfind('}') {
                    content[start..=end].to_string()
                } else {
                    content.to_string()
                }
            } else {
                content.to_string()
            }
        };

        match serde_json::from_str::<Value>(&json_str) {
            Ok(parsed) => {
                let summary = parsed.get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or(&fallback.summary)
                    .to_string();
                
                let status = parsed.get("overall_status")
                    .and_then(|s| s.as_str())
                    .unwrap_or(&fallback.status)
                    .to_string();

                let mut artifacts = Vec::new();
                if let Some(findings) = parsed.get("key_findings").and_then(|f| f.as_array()) {
                    artifacts.push(json!({"type": "key_findings", "items": findings}));
                }
                if let Some(recommendations) = parsed.get("recommendations").and_then(|r| r.as_array()) {
                    artifacts.push(json!({"type": "recommendations", "items": recommendations}));
                }

                info!("LLM 聚合成功");
                TaskResult {
                    task_iri: fallback.task_iri.clone(),
                    status,
                    summary,
                    output: Some(json!({"aggregated": true})),
                    jsonld_output: None,
                    artifacts,
                    errors: fallback.errors.clone(),
                    turn_count: fallback.turn_count,
                    tool_call_count: fallback.tool_call_count,
                    five_w2h_updates: None,
                    tracked_actions: Vec::new(),
                    archive_iri: None,
                }
            }
            _ => {
                warn!("无法解析 LLM 聚合结果，使用简单聚合");
                fallback.clone()
            }
        }
    }

    fn aggregate_results(&self, _phase: &str) -> TaskResult {
        let total = self.sub_results.len();
        let successes = self.sub_results.iter().filter(|r| r.status == "success").count();
        let mut all_errors = Vec::new();

        let mut summary_parts = Vec::new();
        for (i, r) in self.sub_results.iter().enumerate() {
            summary_parts.push(format!("  [{}] {}: {}", i, r.status, r.summary));
            all_errors.extend(r.errors.clone());
        }

        let summary = format!(
            "聚合 {} 个子任务: {}/{} 成功\n{}",
            total,
            successes,
            total,
            summary_parts.join("\n"),
        );

        TaskResult {
            task_iri: String::new(),
            status: if successes == total { "success".to_string() } else { "partial".to_string() },
            summary,
            output: None,
            jsonld_output: None,
            artifacts: Vec::new(),
            errors: all_errors,
            turn_count: 0,
            tool_call_count: 0,
            five_w2h_updates: None,
            tracked_actions: Vec::new(),
            archive_iri: None,
        }
    }
}
