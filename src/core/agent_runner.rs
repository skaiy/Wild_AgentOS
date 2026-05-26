use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tracing::{debug, info, instrument, warn};

use crate::config::settings::AgentSettings;
use crate::core::agent_instance::{AgentInstance, AgentRole, AgentStatus};
use crate::core::system_prompt::{SystemPromptBuilder, SystemPromptRegion};
use crate::gateway::unified_gateway::{ChatMessage, UnifiedGateway};
use crate::jsonld::{generate_iri, validate_jsonld_node, JsonLdContext, JsonLdNode, GLOBAL_CONTEXT};
use crate::memory::l0_store::L0Store;
use crate::memory::l1_session::L1Session;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_manager::MemoryManager;
use crate::templates::template_engine::TemplateEngine;
use crate::tools::hooks::{HookContext, HookManager, HookPoint, HookResult};
use crate::tools::sharing::{ContextInjector, Permission, ShareType, SharingProtocol};
use crate::tools::skill_registry::SkillRegistry;
use crate::tools::tool_executor::ToolExecutor;
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReActPhase {
    Thought,
    Action,
    Observation,
}

const LLM_RESPONSE_FORMAT_WITH_THOUGHT: &str = r#"
你必须严格按照以下 JSON 格式回复，不要添加任何其他内容：

{
  "thought": "你的思考过程（分析当前状态、决定下一步行动）",
  "content": "你的正式回复内容",
  "summary": "本轮回复的摘要（不超过100字）",
  "action": "tool_call|finish|continue",
  "emphasis": []
}

重要约束：
1. 必须是有效的 JSON 格式
2. 不要在 JSON 前后添加任何文字或标记
3. action 只能是 tool_call、finish 或 continue 之一
4. emphasis 是数组，如果没有强调内容则为空数组 []
5. 如果要调用工具，action 设为 "tool_call"，然后使用 function_call
6. 如果任务完成，action 设为 "finish"，不要再调用工具

示例：
{"thought": "我需要创建文件", "content": "创建 calculator.py", "summary": "创建主文件", "action": "tool_call", "emphasis": []}
{"thought": "任务已完成", "content": "所有文件已创建并测试通过", "summary": "任务完成", "action": "finish", "emphasis": []}
"#;

const LLM_RESPONSE_FORMAT_NO_THOUGHT: &str = r#"
你必须以 JSON 格式回复，包含以下字段：
- "content": 你的正式回复内容
- "summary": 本轮回复的摘要（不超过100字）
- "action": 你的下一步行动决策，必须是以下之一：
  - "tool_call": 需要调用工具（通过 function_call 实现）
  - "finish": 任务已完成，返回最终结果
  - "continue": 继续思考（不调用工具，继续下一轮）
- "emphasis": 你从对话中识别出的重要约束或强调内容（数组，如无则为空数组）

注意：你使用的是支持原生思考的模型，请直接在回复前进行思考，无需在 JSON 中输出思考过程。

示例：
{"content": "让我先查看文件", "summary": "查看文件内容", "action": "tool_call", "emphasis": []}
{"content": "所有文件已创建并测试通过", "summary": "任务完成", "action": "finish", "emphasis": []}
"#;

#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task_iri: String,
    pub objective: String,
    pub parent_task_iri: Option<String>,
    pub input_data: HashMap<String, Value>,
    pub constraints: HashMap<String, String>,
    pub max_iterations: u32,
    pub prev_agent_summary: Option<String>,
    pub original_task: Option<String>,
    pub completed_steps: Vec<String>,
    pub pending_steps: Vec<String>,
    pub five_w2h_iri: String,
    pub five_w2h_snapshot: Option<crate::core::five_w2h::Task5W2H>,
}

impl TaskContext {
    pub fn new(task_iri: &str, objective: &str, max_iterations: u32) -> Self {
        Self {
            task_iri: task_iri.to_string(),
            objective: objective.to_string(),
            parent_task_iri: None,
            input_data: HashMap::new(),
            constraints: HashMap::new(),
            max_iterations,
            prev_agent_summary: None,
            original_task: None,
            completed_steps: Vec::new(),
            pending_steps: Vec::new(),
            five_w2h_iri: String::new(),
            five_w2h_snapshot: None,
        }
    }

    pub fn with_prev_summary(mut self, summary: &str) -> Self {
        self.prev_agent_summary = Some(summary.to_string());
        self
    }

    pub fn with_original_task(mut self, task: &str) -> Self {
        self.original_task = Some(task.to_string());
        self
    }

    pub fn with_steps(mut self, completed: Vec<String>, pending: Vec<String>) -> Self {
        self.completed_steps = completed;
        self.pending_steps = pending;
        self
    }

    pub fn with_five_w2h(mut self, iri: &str, snapshot: crate::core::five_w2h::Task5W2H) -> Self {
        self.five_w2h_iri = iri.to_string();
        self.five_w2h_snapshot = Some(snapshot);
        if self.objective.is_empty() {
            self.objective = self.five_w2h_snapshot.as_ref().map(|s| s.derive_objective()).unwrap_or_default();
        }
        self
    }

    pub fn add_completed_step(&mut self, step: &str) {
        self.completed_steps.push(step.to_string());
        if let Some(pos) = self.pending_steps.iter().position(|s| s == step) {
            self.pending_steps.remove(pos);
        }
    }
}

impl Default for TaskContext {
    fn default() -> Self {
        Self {
            task_iri: String::new(),
            objective: String::new(),
            parent_task_iri: None,
            input_data: HashMap::new(),
            constraints: HashMap::new(),
            max_iterations: 20,
            prev_agent_summary: None,
            original_task: None,
            completed_steps: Vec::new(),
            pending_steps: Vec::new(),
            five_w2h_iri: String::new(),
            five_w2h_snapshot: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_iri: String,
    pub status: String,
    pub summary: String,
    pub output: Option<Value>,
    pub jsonld_output: Option<Value>,
    pub artifacts: Vec<Value>,
    pub errors: Vec<String>,
    pub turn_count: u32,
    pub tool_call_count: u32,
    pub five_w2h_updates: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct LlmParsedResponse {
    pub thought: Option<String>,
    pub content: String,
    pub summary: Option<String>,
    pub action: Option<String>,
    pub is_valid_json: bool,
    pub has_native_reasoning: bool,
    pub emphasis: Vec<String>,
}

#[derive(Clone)]
pub struct AgentRunner {
    pub gateway: Arc<UnifiedGateway>,
    pub skills: Arc<SkillRegistry>,
    pub blackboard: Arc<Blackboard>,
    pub l0_store: Arc<L0Store>,
    pub memory_manager: Arc<tokio::sync::Mutex<MemoryManager>>,
    pub templates: Arc<TemplateEngine>,
    pub tool_executor: Arc<std::sync::RwLock<ToolExecutor>>,
    pub agent_settings: AgentSettings,
    pub hook_manager: Arc<HookManager>,
    pub projection: Arc<ProjectionEngine>,
    pub sharing: Arc<SharingProtocol>,
    pub emphasis_config: Option<crate::config::settings::EmphasisConfig>,
    pub event_bus: Option<Arc<crate::core::event_bus::EventBus>>,
}

impl AgentRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gateway: Arc<UnifiedGateway>,
        skills: Arc<SkillRegistry>,
        blackboard: Arc<Blackboard>,
        l0_store: Arc<L0Store>,
        memory_manager: Arc<tokio::sync::Mutex<MemoryManager>>,
        templates: Arc<TemplateEngine>,
        agent_settings: AgentSettings,
    ) -> Self {
        let projection = Arc::new(ProjectionEngine::new(blackboard.clone(), 500));
        let sharing = Arc::new(SharingProtocol::new());
        Self {
            gateway,
            skills,
            blackboard,
            l0_store,
            memory_manager,
            templates,
            tool_executor: Arc::new(std::sync::RwLock::new(ToolExecutor::new())),
            agent_settings,
            hook_manager: Arc::new(HookManager::new()),
            projection,
            sharing,
            emphasis_config: None,
            event_bus: None,
        }
    }

    pub fn with_emphasis_config(mut self, config: crate::config::settings::EmphasisConfig) -> Self {
        self.emphasis_config = Some(config);
        self
    }

    pub fn with_hook_manager(mut self, hook_manager: HookManager) -> Self {
        self.hook_manager = Arc::new(hook_manager);
        self
    }

    pub fn set_event_bus(&mut self, event_bus: Arc<crate::core::event_bus::EventBus>) {
        self.event_bus = Some(event_bus);
    }

    #[instrument(skip(self, agent, ctx), fields(agent_id = %agent.agent_id, role = ?agent.role, task_iri = %ctx.task_iri))]
    pub async fn execute(
        &self,
        agent: &mut AgentInstance,
        ctx: TaskContext,
    ) -> Result<TaskResult, CoreError> {
        // AgentInit hook
        {
            let mut hook_ctx = HookContext::new(
                HookPoint::AgentInit,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            self.hook_manager
                .execute(HookPoint::AgentInit, &mut hook_ctx)
                .await;
        }

        agent.status = AgentStatus::Running;

        // TaskStart hook
        {
            let mut hook_ctx = HookContext::new(
                HookPoint::TaskStart,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            let hook_result = self
                .hook_manager
                .execute(HookPoint::TaskStart, &mut hook_ctx)
                .await;
            if hook_result == HookResult::Abort {
                agent.status = AgentStatus::Failed;
                return Ok(TaskResult {
                    task_iri: ctx.task_iri,
                    status: "aborted".to_string(),
                    summary: "Task aborted by hook".to_string(),
                    output: None,
                    jsonld_output: None,
                    artifacts: Vec::new(),
                    errors: vec!["Task aborted by hook".to_string()],
                    turn_count: 0,
                    tool_call_count: 0,
                    five_w2h_updates: None,
                });
            }
        }

        // AgentStart hook
        let mut hook_ctx = HookContext::new(
            HookPoint::AgentStart,
            &agent.agent_id,
            &agent.role.to_string(),
        )
        .with_task(&ctx.task_iri, &ctx.task_iri);
        let hook_result = self
            .hook_manager
            .execute(HookPoint::AgentStart, &mut hook_ctx)
            .await;
        if hook_result == HookResult::Abort {
            agent.status = AgentStatus::Failed;
            return Ok(TaskResult {
                task_iri: ctx.task_iri,
                status: "aborted".to_string(),
                summary: "Agent aborted by hook".to_string(),
                output: None,
                jsonld_output: None,
                artifacts: Vec::new(),
                errors: vec!["Agent aborted by hook".to_string()],
                turn_count: 0,
                tool_call_count: 0,
                five_w2h_updates: None,
            });
        }

        let mut session = self.memory_manager.lock().await.create_session(
            &agent.agent_id,
            &agent.role.to_string(),
            &ctx.task_iri,
        );

        // MemoryWrite hook for session creation
        {
            let mut hook_ctx = HookContext::new(
                HookPoint::MemoryWrite,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            self.hook_manager
                .execute(HookPoint::MemoryWrite, &mut hook_ctx)
                .await;
        }

        let result = self.exec(agent, ctx.clone(), &mut session).await;

        // 归档 Session 到 L2 + L0（替代 exec 内无效的 close_session）
        let summary = session.summarize();
        {
            let m = self.memory_manager.lock().await;
            let _ = m.archive_to_l2(&ctx.task_iri, &summary);
            let _ = m.archive_to_l0(&summary);
        }

        // TaskEnd hook
        {
            let mut hook_ctx =
                HookContext::new(HookPoint::TaskEnd, &agent.agent_id, &agent.role.to_string())
                    .with_task(&ctx.task_iri, &ctx.task_iri);
            self.hook_manager
                .execute(HookPoint::TaskEnd, &mut hook_ctx)
                .await;
        }

        // AgentEnd hook
        let mut hook_ctx = HookContext::new(
            HookPoint::AgentEnd,
            &agent.agent_id,
            &agent.role.to_string(),
        );
        self.hook_manager
            .execute(HookPoint::AgentEnd, &mut hook_ctx)
            .await;

        // Handle errors
        if let Ok(ref r) = result {
            if r.status == "failed" {
                let mut hook_ctx = HookContext::new(
                    HookPoint::AgentError,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri);
                hook_ctx.error = Some(r.summary.clone());
                self.hook_manager
                    .execute(HookPoint::AgentError, &mut hook_ctx)
                    .await;

                let mut hook_ctx = HookContext::new(
                    HookPoint::TaskError,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri);
                hook_ctx.error = Some(r.summary.clone());
                self.hook_manager
                    .execute(HookPoint::TaskError, &mut hook_ctx)
                    .await;
            }
        }

        result
    }

    /// 使用独立的 BizAgent 实例执行任务（Agent 隔离）
    pub async fn execute_with_biz_agent(
        &self,
        agent: &AgentInstance,
        ctx: TaskContext,
        plan_step: Option<crate::core::sa::PlanStep>,
    ) -> Result<TaskResult, CoreError> {
        use crate::core::biz_agent::{AgentConfig, BizAgent};

        // AgentInit hook
        {
            let mut hook_ctx = HookContext::new(
                HookPoint::AgentInit,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            self.hook_manager
                .execute(HookPoint::AgentInit, &mut hook_ctx)
                .await;
        }

        // 构建独立的 agent.md
        let agent_md = if let Some(ref step) = plan_step {
            self.build_agent_md_from_step(agent.role, step)
        } else {
            let context_data = self.gather_context_data_async(agent.role, &ctx).await;
            let model = self
                .gateway
                .get_model(&agent.role.to_string().to_lowercase());
            self.build_agent_md(agent.role, &ctx.objective, &context_data, &model)
        };

        // 创建 BizAgent 配置
        let config = AgentConfig {
            orchestrator_mode: false,
            max_sub_agents: 5,
            max_iterations: ctx.max_iterations,
            parallel_sub_agents: true,
        };

        // 创建独立的 BizAgent 实例
        let mut biz_agent = BizAgent::new(
            agent.agent_id.clone(),
            agent.role,
            &agent_md,
            Arc::new((*self).clone()),
            config,
        );

        // TaskStart hook
        {
            let mut hook_ctx = HookContext::new(
                HookPoint::TaskStart,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            let hook_result = self
                .hook_manager
                .execute(HookPoint::TaskStart, &mut hook_ctx)
                .await;
            if hook_result == HookResult::Abort {
                return Ok(TaskResult {
                    task_iri: ctx.task_iri,
                    status: "aborted".to_string(),
                    summary: "Task aborted by hook".to_string(),
                    output: None,
                    jsonld_output: None,
                    artifacts: Vec::new(),
                    errors: vec!["Task aborted by hook".to_string()],
                    turn_count: 0,
                    tool_call_count: 0,
                    five_w2h_updates: None,
                });
            }
        }

        // AgentStart hook
        let mut hook_ctx = HookContext::new(
            HookPoint::AgentStart,
            &agent.agent_id,
            &agent.role.to_string(),
        )
        .with_task(&ctx.task_iri, &ctx.task_iri);
        let hook_result = self
            .hook_manager
            .execute(HookPoint::AgentStart, &mut hook_ctx)
            .await;
        if hook_result == HookResult::Abort {
            return Ok(TaskResult {
                task_iri: ctx.task_iri,
                status: "aborted".to_string(),
                summary: "Agent aborted by hook".to_string(),
                output: None,
                jsonld_output: None,
                artifacts: Vec::new(),
                errors: vec!["Agent aborted by hook".to_string()],
                turn_count: 0,
                tool_call_count: 0,
                five_w2h_updates: None,
            });
        }

        // 执行 BizAgent（隔离环境）
        let result = biz_agent.execute(ctx.clone()).await;

        // TaskEnd hook
        {
            let mut hook_ctx =
                HookContext::new(HookPoint::TaskEnd, &agent.agent_id, &agent.role.to_string())
                    .with_task(&ctx.task_iri, &ctx.task_iri);
            self.hook_manager
                .execute(HookPoint::TaskEnd, &mut hook_ctx)
                .await;
        }

        // AgentEnd hook
        let mut hook_ctx = HookContext::new(
            HookPoint::AgentEnd,
            &agent.agent_id,
            &agent.role.to_string(),
        );
        self.hook_manager
            .execute(HookPoint::AgentEnd, &mut hook_ctx)
            .await;

        // Handle errors
        if result.status == "failed" {
            let mut hook_ctx = HookContext::new(
                HookPoint::AgentError,
                &agent.agent_id,
                &agent.role.to_string(),
            )
            .with_task(&ctx.task_iri, &ctx.task_iri);
            hook_ctx.error = Some(result.summary.clone());
            self.hook_manager
                .execute(HookPoint::AgentError, &mut hook_ctx)
                .await;
        }

        Ok(result)
    }

    fn build_agent_md_from_step(
        &self,
        role: AgentRole,
        step: &crate::core::sa::PlanStep,
    ) -> String {
        let role_name = match role {
            AgentRole::Plan => "计划",
            AgentRole::Do => "执行",
            AgentRole::Check => "检查",
            AgentRole::Act => "决策",
        };

        let tools_list = if step.tools_allowed.is_empty() {
            self.tool_executor.read().unwrap().list_tools(&role.to_string())
        } else {
            step.tools_allowed.clone()
        };

        let model = self.gateway.get_model(&role.to_string().to_lowercase());
        let supports_reasoning = self.gateway.supports_native_reasoning(&model);
        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT
        };

        format!(
            r#"# {} Agent

## 当前任务目标
{}

## 预期输出
{}

## 成功标准
{}

## 可用工具
{}

## 输出格式要求
{}

## 注意事项
- 这是一个独立的执行环境，请专注于完成当前任务
- 不要假设任务已完成，必须实际执行并验证
- 如果遇到问题，请详细说明原因
- 完成任务后给出简洁的 summary
"#,
            role_name,
            step.objective,
            step.expected_output,
            step.success_criteria,
            tools_list.join(", "),
            format_constraint
        )
    }

    pub async fn create_session(&self, agent: &AgentInstance, ctx: &TaskContext) -> L1Session {
        self.memory_manager.lock().await.create_session(
            &agent.agent_id,
            &agent.role.to_string(),
            &ctx.task_iri,
        )
    }

    fn gather_context_data(&self, role: AgentRole, ctx: &TaskContext) -> HashMap<String, String> {
        let mut context_data = HashMap::new();

        if let Some(ref original_task) = ctx.original_task {
            context_data.insert("original_task".to_string(), original_task.clone());
        }

        if let Some(ref summary) = ctx.prev_agent_summary {
            match role {
                AgentRole::Do => {
                    context_data.insert("plan_content".to_string(), summary.clone());
                }
                AgentRole::Check => {
                    context_data.insert("execution_result".to_string(), summary.clone());
                }
                AgentRole::Act => {
                    context_data.insert("check_result".to_string(), summary.clone());
                }
                _ => {}
            }
        }

        if !ctx.completed_steps.is_empty() {
            context_data.insert("completed_steps".to_string(), ctx.completed_steps.join(", "));
        }
        if !ctx.pending_steps.is_empty() {
            context_data.insert("pending_steps".to_string(), ctx.pending_steps.join(", "));
        }

        for (k, v) in &ctx.constraints {
            context_data.insert(k.clone(), v.clone());
        }

        if let Some(ref snapshot) = ctx.five_w2h_snapshot {
            context_data.insert("five_w2h_what".to_string(), snapshot.what.clone());
            context_data.insert("five_w2h_why".to_string(), snapshot.why.description.clone());
            if !snapshot.why.success_criteria.is_empty() {
                context_data.insert("five_w2h_success_criteria".to_string(), snapshot.why.success_criteria.join(", "));
            }
            if let Some(ref when) = snapshot.when {
                if let Some(ref deadline) = when.deadline {
                    context_data.insert("five_w2h_deadline".to_string(), deadline.to_rfc3339());
                }
            }
            if let Some(ref how) = snapshot.how {
                if let Some(ref steps) = how.required_steps {
                    context_data.insert("five_w2h_required_steps".to_string(), steps.clone());
                }
                if !how.forbidden_tools.is_empty() {
                    context_data.insert("five_w2h_forbidden_tools".to_string(), how.forbidden_tools.join(", "));
                }
            }
            if let Some(ref how_much) = snapshot.how_much {
                if let Some(budget) = how_much.token_budget {
                    context_data.insert("five_w2h_token_budget".to_string(), budget.to_string());
                }
                if let Some(cycles) = how_much.max_pdca_cycles {
                    context_data.insert("five_w2h_max_cycles".to_string(), cycles.to_string());
                }
            }
            if let Some(ref where_) = snapshot.where_ {
                if let Some(ref env) = where_.execution_environment {
                    context_data.insert("five_w2h_execution_env".to_string(), env.clone());
                }
            }
        }

        context_data
    }

    async fn gather_context_data_async(
        &self,
        role: AgentRole,
        ctx: &TaskContext,
    ) -> HashMap<String, String> {
        let mut context_data = self.gather_context_data(role, ctx);

        let frame_name = match role {
            AgentRole::Plan => "pa_init",
            AgentRole::Do => "da_input",
            AgentRole::Check => "ca_review",
            AgentRole::Act => "aa_decision",
        };

        if let Ok(projection_str) = self
            .projection
            .project(&ctx.task_iri, frame_name, HashMap::new())
            .await
        {
            if !projection_str.is_empty() {
                context_data.insert("context_summary".to_string(), projection_str);
            }
        }

        context_data
    }

    fn build_agent_md(
        &self,
        role: AgentRole,
        objective: &str,
        context_data: &HashMap<String, String>,
        model: &str,
    ) -> String {
        let role_name = role.to_string();
        let role_lower = role_name.to_lowercase();
        let tools_list = self.tool_executor.read().unwrap().list_tools(&role_name);

        let supports_reasoning = self.gateway.supports_native_reasoning(model);
        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT
        };

        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        vars.insert(
            "task_description".to_string(),
            serde_json::Value::String(objective.to_string()),
        );
        vars.insert(
            "available_skills".to_string(),
            serde_json::Value::String(tools_list.join(", ")),
        );
        vars.insert(
            "context_summary".to_string(),
            serde_json::Value::String(
                context_data
                    .get("context_summary")
                    .cloned()
                    .unwrap_or_default(),
            ),
        );
        vars.insert(
            "task_specific_constraints".to_string(),
            serde_json::Value::String(context_data.get("constraints").cloned().unwrap_or_default()),
        );
        vars.insert(
            "plan_content".to_string(),
            serde_json::Value::String(
                context_data
                    .get("plan_content")
                    .cloned()
                    .unwrap_or_else(|| "(由 SA 填写)".to_string()),
            ),
        );
        vars.insert(
            "execution_result".to_string(),
            serde_json::Value::String(
                context_data
                    .get("execution_result")
                    .cloned()
                    .unwrap_or_else(|| "(由 DA 生成)".to_string()),
            ),
        );
        vars.insert(
            "check_result".to_string(),
            serde_json::Value::String(
                context_data
                    .get("check_result")
                    .cloned()
                    .unwrap_or_else(|| "(由 CA 生成)".to_string()),
            ),
        );

        if let Ok(rendered) =
            self.templates
                .render_prompt(&role_lower, "skeleton", &vars, false, None)
        {
            let md = format!(
                r#"# {} Agent.md

{}

## 可用工具
{}

{}
"#,
                role_name,
                rendered,
                tools_list.join(", "),
                format_constraint
            );
            debug!(role = %role_name, supports_reasoning = supports_reasoning, "=== agent.md (from template) ===\n{}", md);
            return md;
        }

        let role_prompt = match role {
            AgentRole::Plan => {
                let w2h_what = context_data.get("five_w2h_what").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_why = context_data.get("five_w2h_why").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_success = context_data.get("five_w2h_success_criteria").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_deadline = context_data.get("five_w2h_deadline").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_env = context_data.get("five_w2h_execution_env").cloned().unwrap_or_else(|| "（未指定）".to_string());
                format!("你是计划Agent(PA)。你的职责是分析用户任务并制定执行计划。\n\n🔴 严格禁止：\n1. 禁止调用写操作工具（file_write, Bash等）\n2. 禁止执行具体工作（创建文件、修改代码等）\n\n✅ 允许的操作：\n1. 可以调用只读工具收集信息（file_read, file_list, grep_search等）\n2. 分析用户任务需求\n3. 制定清晰的执行步骤\n4. 输出JSON格式的计划\n\n📋 任务元数据（5W2H — 必须参考）：\n- What: {}\n- Why: {}\n- 成功标准: {}\n- 截止时间: {}\n- 执行环境: {}\n\n请在上述元数据约束下制定计划。如发现需要补充的信息，请在计划中说明。\n\n规划完成后，建议回填 How 和 Where 维度（可选）：\n{{\"five_w2h_updates\": {{\"how\": {{\"planIRI\": \"计划IRI\", \"preferredSkills\": [...], \"requiredSteps\": \"...\"}}, \"where\": {{\"dataSources\": [...], \"executionEnvironment\": \"...\"}}}}}}", w2h_what, w2h_why, w2h_success, w2h_deadline, w2h_env)
            }
            AgentRole::Do => "你是执行Agent(DA)。你的职责是具体执行任务。\n\n🔴 严格禁止：\n1. 禁止在当前目录执行递归搜索（如 grep -r, find / 等），这会导致超时\n2. 禁止使用相对路径，必须使用任务中指定的绝对路径\n3. 禁止执行与任务无关的操作\n\n✅ 执行要求：\n1. 严格按照任务中指定的路径创建/修改文件\n2. 如果任务要求创建目录，先创建目录再创建文件\n3. 每一步操作都要验证结果\n4. 完成任务后立即调用 finish 结束\n5. 如果 WebSearch/WebFetch 等网络工具失败，不要反复重试，应基于自身知识直接回答\n\n示例流程：\n1. 任务要求创建 /tmp/test/file.txt → 先用 Bash 创建目录，再用 file_write 写入\n2. 任务要求修改文件 → 用 file_read 读取，处理后用 file_write 写入\n3. 任务要求验证 → 用 file_read 读取并检查内容\n4. 搜索工具失败 → 直接基于自身知识回答，不要重试超过1次".to_string(),
            AgentRole::Check => {
                let w2h_what = context_data.get("five_w2h_what").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_why = context_data.get("five_w2h_why").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_deadline = context_data.get("five_w2h_deadline").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_env = context_data.get("five_w2h_execution_env").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_steps = context_data.get("five_w2h_required_steps").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_budget = context_data.get("five_w2h_token_budget").cloned().unwrap_or_else(|| "（未指定）".to_string());
                format!("你是检查Agent(CA)。你的职责是审查执行结果，确保任务目标达成。\n\n🔴 关键职责：\n1. 不要相信前一个 Agent 的 summary，必须实际验证\n2. 使用工具检查文件是否真实存在\n3. 根据任务特性选择合适的审计视角\n\n📋 推荐审计参考（5W2H 维度 — 必须关注的重要维度之一）：\n- What: {} — 任务目标是否达成？\n- Why: {} — 是否满足原始意图？\n- When: {} — 截止时间是否满足？\n- Where: {} — 是否在正确环境操作？\n- How: {} — 步骤是否按计划执行？\n- HowMuch: {} — 资源是否超支？\n\n注意：5W2H 是重要的分析维度之一，你可以根据任务特性增加其他审计视角（如安全性、可维护性、性能等）。\n\n📋 输出格式：\n请输出结构化的审计结果，包含：\n1. 各审计视角的检查结论（PASS/FAIL/CONDITIONAL + 证据）\n2. 总体结论（PASS/CONDITIONAL_PASS/FAIL）\n3. 发现的问题及建议", w2h_what, w2h_why, w2h_deadline, w2h_env, w2h_steps, w2h_budget)
            }
            AgentRole::Act => "你是决策Agent(AA)。你的职责是基于 CA 的审计结果做决策，并给出处置建议。\n\n🔴 关键职责：\n1. 综合考虑 CA 审计结果、任务约束和实际情况做决策\n2. 给出具体的处置建议\n\n📋 决策参考：\n- CA 审计结论\n- 任务约束（5W2H 维度：What/Why/When/Where/How/HowMuch）\n- 任务实际情况\n\n📋 常见决策路径（仅供参考）：\n- 审计全部通过 → 归档任务，沉淀经验\n- 目标/意图未达成 → 建议回退重新分析或修正计划\n- 执行方式/环境问题 → 建议修正计划\n- 时间/资源超支 → 评估原因合理性后决定放行或降级\n\n📋 输出格式：\n1. 任务状态：完成 / 部分完成 / 未完成\n2. 处置建议：具体行动建议\n3. 最终结论：简洁总结".to_string(),
        };

        let context_section = if context_data.is_empty() {
            String::new()
        } else {
            let mut sections = Vec::new();
            if let Some(original) = context_data.get("original_task") {
                sections.push(format!("## 原始任务要求\n{}\n\n⚠️ 重要：你必须验证上述所有要求是否都已完成。", original));
            }
            if let Some(plan) = context_data.get("plan_content") {
                sections.push(format!("## 上级计划\n{}", plan));
            }
            if let Some(result) = context_data.get("execution_result") {
                sections.push(format!("## 执行结果\n{}", result));
            }
            if let Some(check) = context_data.get("check_result") {
                sections.push(format!("## 检查结论\n{}", check));
            }
            if let Some(ctx_summary) = context_data.get("context_summary") {
                sections.push(format!("## 相关上下文\n{}", ctx_summary));
            }
            if let Some(completed) = context_data.get("completed_steps") {
                sections.push(format!("## 已完成步骤\n{}", completed));
            }
            if let Some(pending) = context_data.get("pending_steps") {
                sections.push(format!("## 待完成步骤\n{}", pending));
            }
            let has_w2h = context_data.contains_key("five_w2h_what");
            if has_w2h {
                let mut w2h_lines = Vec::new();
                if let Some(v) = context_data.get("five_w2h_what") {
                    w2h_lines.push(format!("- What: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_why") {
                    w2h_lines.push(format!("- Why: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_success_criteria") {
                    w2h_lines.push(format!("- 成功标准: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_deadline") {
                    w2h_lines.push(format!("- 截止时间: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_execution_env") {
                    w2h_lines.push(format!("- 执行环境: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_required_steps") {
                    w2h_lines.push(format!("- 要求步骤: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_forbidden_tools") {
                    w2h_lines.push(format!("- 禁用工具: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_token_budget") {
                    w2h_lines.push(format!("- Token 预算: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_max_cycles") {
                    w2h_lines.push(format!("- 最大循环: {}", v));
                }
                if !w2h_lines.is_empty() {
                    sections.push(format!("## 任务元数据（5W2H）\n{}", w2h_lines.join("\n")));
                }
            }
            sections.join("\n\n")
        };

        let md = format!(
            "# {} Agent.md\n\n角色: {}\n任务: {}\n可用工具: {}\n工作方式: {}\n\n{}\n\n重要：完成你的职责后，直接输出最终结果，不要再调用工具。你的回复应包含完整的结论或结果。{}",
            role_name, role_name, objective, tools_list.join(", "), role_prompt, context_section, format_constraint
        );
        debug!(role = %role_name, "=== agent.md (fallback) ===\n{}", md);
        md
    }

    #[instrument(skip(self, agent, ctx, sess), fields(agent_id = %agent.agent_id, role = ?agent.role))]
    async fn exec(
        &self,
        agent: &AgentInstance,
        ctx: TaskContext,
        sess: &mut L1Session,
    ) -> Result<TaskResult, CoreError> {
        let model = self
            .gateway
            .get_model(&agent.role.to_string().to_lowercase());
        let supports_reasoning = self.gateway.supports_native_reasoning(&model);

        let context_data = self.gather_context_data_async(agent.role, &ctx).await;
        let agent_md = self.build_agent_md(agent.role, &ctx.objective, &context_data, &model);

        // 使用 SystemPromptBuilder 构建系统提示词
        let mut prompt_builder = SystemPromptBuilder::new();

        // Region 1: 角色定义区
        prompt_builder.set_region(SystemPromptRegion::RoleDefinition, agent_md.clone());

        // Region 2: 强调约束区（从 L0 加载）
        let emphasis_items = self.load_emphasis_from_l0(&ctx.task_iri).await;
        if !emphasis_items.is_empty() {
            let emphasis_content = emphasis_items
                .iter()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            prompt_builder.set_region(SystemPromptRegion::EmphasizedConstraints, emphasis_content);
        }

        // Region 3: 输出格式区
        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT.to_string()
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT.to_string()
        };
        prompt_builder.set_region(SystemPromptRegion::OutputFormat, format_constraint);

        // Region 4: 工具区（内置工具 + 动态工具）
        let tool_menu = self.build_readable_tool_menu(&agent.role);
        if !tool_menu.is_empty() {
            prompt_builder.set_region(SystemPromptRegion::Tools, tool_menu);
        }

        // Region 5: 提取提示区（从配置加载）
        if let Some(ref config) = self.emphasis_config {
            if config.enabled {
                prompt_builder.set_region(
                    SystemPromptRegion::ExtractionPrompt,
                    config.extraction_prompt.clone(),
                );
            }
        }

        // 构建系统提示词（相对固定，放在 system role）
        let system_content = prompt_builder.build();

        // 构建上下文消息（动态变化，放在最后的 user role）
        let summary_chain = sess.get_summary_chain();
        let summary_text = summary_chain
            .first()
            .and_then(|v| v.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let context_msg = if summary_text.is_empty() {
            format!(
                "## 当前任务\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective
            )
        } else {
            format!(
                "## 当前任务\n{}\n\n## 历史摘要\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective, summary_text
            )
        };

        let mut messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_content,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: context_msg,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        ];

        let tools = self
            .tool_executor
            .read()
            .unwrap()
            .tool_definitions_for_role(&agent.role.to_string());

        info!(
            "AgentRunner 开始: role={}, model={}, tools={}, supports_reasoning={}",
            agent.role,
            model,
            tools.len(),
            supports_reasoning
        );

        let mut tc = 0u32;
        let mut errs = Vec::new();
        let mut turn = 0u32;

        let effective_max_turns = match agent.role {
            AgentRole::Plan => ctx.max_iterations.min(200),
            AgentRole::Do => ctx.max_iterations,
            AgentRole::Check => ctx.max_iterations.min(200),
            AgentRole::Act => ctx.max_iterations.min(150),
        };

        loop {
            if turn >= effective_max_turns {
                warn!("[turn {}] 达到角色 {} 最大轮次限制 {}, 强制结束", turn, agent.role, effective_max_turns);
                errs.push("max turns reached".to_string());
                if let Some(ref event_bus) = self.event_bus {
                    let _ = event_bus.emit(&ctx.task_iri, "AGENT_BLOCKED", &agent.agent_id, &serde_json::json!({"iterations": turn}).to_string()).await;
                }
                break;
            }
            turn += 1;

            // ===== Thought 阶段 =====
            info!("[ReAct Turn {}] ===== Thought =====", turn);

            // CycleStart hook
            {
                let mut hook_ctx = HookContext::new(
                    HookPoint::CycleStart,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri)
                .with_data("turn", Value::Number(turn.into()));
                self.hook_manager
                    .execute(HookPoint::CycleStart, &mut hook_ctx)
                    .await;
            }

            {
                let mut hook_ctx = HookContext::new(
                    HookPoint::LlmRequest,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri);
                let hook_result = self
                    .hook_manager
                    .execute(HookPoint::LlmRequest, &mut hook_ctx)
                    .await;
                if hook_result == HookResult::Abort {
                    errs.push("LLM request aborted by hook".to_string());
                    break;
                }
            }

            let max_context_messages = 30;
            if messages.len() > max_context_messages {
                let system_msg = messages.first().cloned();
                let kept_recent = messages.len().saturating_sub(max_context_messages / 2);

                let mut recent: Vec<_> = messages.drain(kept_recent..).collect();

                while !recent.is_empty() {
                    let first = &recent[0];
                    if first.role == "tool" {
                        recent.remove(0);
                        continue;
                    }
                    if first.role == "assistant" {
                        if let Some(ref tool_calls) = first.tool_calls {
                            let expected_tool_results = tool_calls.len();
                            let actual_tool_results = recent
                                .iter()
                                .skip(1)
                                .take_while(|m| m.role == "tool")
                                .count();
                            if actual_tool_results < expected_tool_results {
                                recent.remove(0);
                                continue;
                            }
                        }
                    }
                    break;
                }

                messages.clear();
                if let Some(sys) = system_msg {
                    messages.push(sys);
                }
                let summary_note = ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "[历史摘要] 之前已执行 {} 轮操作，包含 {} 次工具调用。以下是最近的对话：",
                        turn - 1, tc
                    ),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                };
                messages.push(summary_note);
                messages.extend(recent);

                info!(
                    "[turn {}] 消息历史截断: 保留 {} 条 (原始 {} 条)",
                    turn,
                    messages.len(),
                    kept_recent + max_context_messages / 2 + 2
                );
            }

            debug!(
                "[turn {}] 调用 LLM (history: {} msgs, tools: {})",
                turn,
                messages.len(),
                tools.len()
            );

            let response = self
                .gateway
                .chat_with_params(
                    &model,
                    messages.clone(),
                    None,
                    None,
                    if tools.is_empty() {
                        None
                    } else {
                        Some(tools.clone())
                    },
                    None,
                )
                .await
                .map_err(|e| CoreError::Internal {
                    message: e.to_string(),
                })?;

            {
                let mut hook_ctx = HookContext::new(
                    HookPoint::LlmResponse,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri);
                self.hook_manager
                    .execute(HookPoint::LlmResponse, &mut hook_ctx)
                    .await;
            }

            let choice = response
                .choices
                .first()
                .ok_or_else(|| CoreError::Internal {
                    message: "No choices in response".to_string(),
                })?;
            let raw_content = choice.message.content.clone().unwrap_or_default();
            let reasoning_content = choice.message.reasoning_content.clone();
            let finish = choice.finish_reason.as_deref().unwrap_or("");

            debug!(
                "[turn {}] LLM 回复: finish={}, content_len={}, has_reasoning={}",
                turn,
                finish,
                raw_content.len(),
                reasoning_content.is_some()
            );

            let parsed = self.parse_llm_response(
                &raw_content,
                reasoning_content.as_deref(),
                supports_reasoning,
            );

            // 只有当 finish 不是 tool_calls 时才打印 WARN
            // 因为工具调用时 content 非 JSON 是正常行为
            if !parsed.is_valid_json && finish != "tool_calls" {
                warn!("[turn {}] LLM 回复不是有效 JSON，使用 fallback 处理", turn);
            }

            let mut action = parsed
                .action
                .clone()
                .unwrap_or_else(|| "continue".to_string());

            if finish == "tool_calls" && choice.message.tool_calls.is_some() {
                action = "tool_call".to_string();
                debug!(
                    "[turn {}] finish=tool_calls 且存在 tool_calls，强制 action=tool_call",
                    turn
                );
            }

            if (finish == "stop" || finish == "end_turn") && action != "tool_call" {
                if action != "finish" {
                    debug!(
                        "[turn {}] finish={} 且无工具调用，将 action 从 {} 修正为 finish",
                        turn, finish, action
                    );
                }
                action = "finish".to_string();
            }

            info!(
                "[ReAct Turn {}] Thought: action={}, summary={}",
                turn,
                action,
                parsed.summary.as_deref().unwrap_or("")
            );

            // 保存强调内容到 L0 永久记忆
            if !parsed.emphasis.is_empty() {
                let dedup_threshold = self
                    .emphasis_config
                    .as_ref()
                    .map(|c| c.dedup_threshold)
                    .unwrap_or(0.85);
                self.save_emphasis_to_l0(
                    &parsed.emphasis,
                    &ctx.task_iri,
                    &agent.agent_id,
                    dedup_threshold,
                )
                .await;
            }

            // 归档到 L0：保存完整回复 + 思考内容
            let l0_iri = sess
                .archive_full_to_l0(
                    &self.l0_store,
                    &agent.role.to_string(),
                    &parsed.thought.clone().unwrap_or_default(),
                    &parsed.content,
                )
                .ok();
            debug!(
                "[L0] 归档: {:?}, has_reasoning={}, is_valid_json={}",
                l0_iri, parsed.has_native_reasoning, parsed.is_valid_json
            );

            // MemoryWrite hook for L0 archive
            {
                let mut hook_ctx = HookContext::new(
                    HookPoint::MemoryWrite,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri)
                .with_data("storage", Value::String("L0".to_string()));
                if let Some(ref iri) = l0_iri {
                    hook_ctx
                        .data
                        .insert("iri".to_string(), Value::String(iri.clone()));
                }
                self.hook_manager
                    .execute(HookPoint::MemoryWrite, &mut hook_ctx)
                    .await;
            }

            let task_id = ctx.task_iri
                .strip_prefix("iri://task/")
                .unwrap_or_else(|| ctx.task_iri.strip_prefix("iri://").unwrap_or(&ctx.task_iri));
            let node_iri = format!(
                "iri://task/{}/turn_{}",
                task_id,
                turn
            );
            let mut node_json = json!({
                "@id": &node_iri,
                "@type": "AgentTurn",
                "role": agent.role.to_string(),
                "content_len": parsed.content.len(),
                "is_valid_json": parsed.is_valid_json,
                "has_native_reasoning": parsed.has_native_reasoning
            });
            if let Some(ref thought) = parsed.thought {
                node_json["has_thought"] = Value::Bool(true);
                node_json["thought_len"] = Value::Number(thought.len().into());
            }
            if let Some(ref act) = parsed.action {
                node_json["action"] = Value::String(act.clone());
            }
            if let Some(ref s) = parsed.summary {
                node_json["summary"] = Value::String(s.clone());
            }
            JsonLdContext::inject(&mut node_json);
            let cfg = crate::CoreConfig::default();
            if self
                .blackboard
                .write_node(&node_iri, &node_json.to_string(), &cfg)
                .is_ok()
            {
                debug!("[L2] 写入节点: {}", node_iri);

                // BlackboardWrite hook
                let mut hook_ctx = HookContext::new(
                    HookPoint::BlackboardWrite,
                    &agent.agent_id,
                    &agent.role.to_string(),
                )
                .with_task(&ctx.task_iri, &ctx.task_iri)
                .with_data("node_iri", Value::String(node_iri.clone()));
                self.hook_manager
                    .execute(HookPoint::BlackboardWrite, &mut hook_ctx)
                    .await;
            }

            // 使用解析后的 summary 或生成 fallback
            let summary_text = parsed
                .summary
                .clone()
                .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
            sess.add_summary(&agent.role.to_string(), &summary_text, l0_iri);

            // ===== Action 阶段 =====
            info!("[ReAct Turn {}] ===== Action =====", turn);

            match action.as_str() {
                "finish" => {
                    info!("[ReAct] Agent 决定完成任务");

                    // CycleEnd hook
                    {
                        let mut hook_ctx = HookContext::new(
                            HookPoint::CycleEnd,
                            &agent.agent_id,
                            &agent.role.to_string(),
                        )
                        .with_task(&ctx.task_iri, &ctx.task_iri)
                        .with_data("turn", Value::Number(turn.into()))
                        .with_data("had_tool_calls", Value::Bool(false));
                        self.hook_manager
                            .execute(HookPoint::CycleEnd, &mut hook_ctx)
                            .await;
                    }

                    info!("AgentRunner 完成: {} turns, {} tools", turn, tc);
                    debug!("[L0] L0 entries: {}", self.l0_store.count());

                    let final_summary = parsed.summary.clone()
                        .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));

                    let output_value = Value::String(parsed.content.clone());
                    let jsonld_output =
                        self.apply_output_mapping(&output_value, &agent.role, &ctx.task_iri);

                    if let Some(ref jsonld) = jsonld_output {
                        if let Ok(node) = JsonLdNode::from_json(jsonld) {
                            let emphasis_items = self.extract_emphasis(&node);
                            if !emphasis_items.is_empty() {
                                let dedup_threshold = self
                                    .emphasis_config
                                    .as_ref()
                                    .map(|c| c.dedup_threshold)
                                    .unwrap_or(0.85);
                                self.save_emphasis_to_l0(
                                    &emphasis_items,
                                    &ctx.task_iri,
                                    &agent.agent_id,
                                    dedup_threshold,
                                )
                                .await;
                            }

                            if let Ok(node_iri) =
                                self.store_jsonld_to_l2(&node, &ctx.task_iri).await
                            {
                                debug!("[L2] JSON-LD 输出已存储: {}", node_iri);
                            }
                        }
                    }

                    return Ok(TaskResult {
                        task_iri: ctx.task_iri,
                        status: "success".to_string(),
                        summary: final_summary,
                        output: Some(output_value),
                        jsonld_output,
                        artifacts: vec![],
                        errors: errs,
                        turn_count: turn,
                        tool_call_count: tc,
                        five_w2h_updates: None,
                    });
                }
                "tool_call" => {
                    if let Some(calls) = &choice.message.tool_calls {
                        let tool_names: Vec<&str> =
                            calls.iter().map(|c| c.function.name.as_str()).collect();
                        debug!("[tool_calls] {} → {:?}", calls.len(), tool_names);

                        // 🔴 PA角色禁止调用写操作工具，但允许只读工具
                        if agent.role == AgentRole::Plan {
                            let readonly_tools = [
                                "file_read",
                                "file_list",
                                "grep_search",
                                "glob_search",
                                "ToolSearch",
                                "WebSearch",
                                "WebFetch",
                            ];
                            let write_tools: Vec<&str> = calls
                                .iter()
                                .map(|c| c.function.name.as_str())
                                .filter(|name| !readonly_tools.contains(name))
                                .collect();

                            if !write_tools.is_empty() {
                                warn!(
                                    "[PA] 检测到写操作工具调用: {:?}，强制转换为finish",
                                    write_tools
                                );
                                info!("[ReAct] PA Agent 被强制结束（禁止写操作）");

                                let final_summary = parsed
                                    .summary
                                    .clone()
                                    .unwrap_or_else(|| "PA已制定计划".to_string());

                                let output_value = Value::String(parsed.content.clone());
                                let jsonld_output = self.apply_output_mapping(
                                    &output_value,
                                    &agent.role,
                                    &ctx.task_iri,
                                );

                                return Ok(TaskResult {
                                    task_iri: ctx.task_iri,
                                    status: "success".to_string(),
                                    summary: final_summary,
                                    output: Some(output_value),
                                    jsonld_output,
                                    artifacts: vec![],
                                    errors: errs,
                                    turn_count: turn,
                                    tool_call_count: tc,
                                    five_w2h_updates: None,
                                });
                            }
                        }

                        let asst_summary = parsed.summary.clone()
                            .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                        messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: asst_summary,
                            name: None,
                            tool_calls: Some(
                                calls
                                    .iter()
                                    .map(|c| crate::gateway::unified_gateway::ToolCallPayload {
                                        id: c.id.clone(),
                                        call_type: c.call_type.clone(),
                                        function:
                                            crate::gateway::unified_gateway::ToolCallFunction {
                                                name: c.function.name.clone(),
                                                arguments: c.function.arguments.clone(),
                                            },
                                    })
                                    .collect(),
                            ),
                            tool_call_id: None,
                            reasoning_content: reasoning_content.clone(),
                        });

                        for c in calls {
                            tc += 1;
                            let name = &c.function.name;
                            let args_raw = &c.function.arguments;
                            let args: Value = serde_json::from_str(args_raw).unwrap_or_default();
                            debug!(
                                "  [tool] {} args={}",
                                name,
                                &args_raw.chars().take(100).collect::<String>()
                            );

                            {
                                let mut hook_ctx = HookContext::new(
                                    HookPoint::SkillBefore,
                                    &agent.agent_id,
                                    &agent.role.to_string(),
                                )
                                .with_task(&ctx.task_iri, &ctx.task_iri)
                                .with_data("tool_name", Value::String(name.clone()));
                                self.hook_manager
                                    .execute(HookPoint::SkillBefore, &mut hook_ctx)
                                    .await;
                            }

                            let result = self
                                .tool_executor
                                .read()
                                .unwrap()
                                .execute(name, &args)
                                .unwrap_or_else(|e| json!({"error": e}));
                            let raw_result_str = serde_json::to_string(&result).unwrap_or_default();

                            let result_str = self.route_tool_result(
                                &raw_result_str,
                                name,
                                &c.id,
                            ).await;

                            debug!("  [tool] {} result: {} bytes (raw: {} bytes)", name, result_str.len(), raw_result_str.len());

                            if let Some(err) = result.get("error") {
                                warn!("[tool] {} 失败: {}", name, err);
                                errs.push(format!("{}: {}", name, err));
                                if let Some(ref event_bus) = self.event_bus {
                                    let _ = event_bus.emit(&ctx.task_iri, "AGENT_ERROR", &agent.agent_id, &serde_json::json!({"error": err, "tool": name}).to_string()).await;
                                }
                            } else {
                                info!("[tool] {} 成功", name);
                            }

                            {
                                let mut hook_ctx = HookContext::new(
                                    HookPoint::SkillAfter,
                                    &agent.agent_id,
                                    &agent.role.to_string(),
                                )
                                .with_task(&ctx.task_iri, &ctx.task_iri)
                                .with_data("tool_name", Value::String(name.clone()));
                                self.hook_manager
                                    .execute(HookPoint::SkillAfter, &mut hook_ctx)
                                    .await;
                            }

                            messages.push(ChatMessage {
                                role: "tool".to_string(),
                                content: result_str,
                                name: None,
                                tool_calls: None,
                                tool_call_id: Some(c.id.clone()),
                                reasoning_content: None,
                            });
                        }

                        // ===== Observation 阶段 =====
                        info!("[ReAct Turn {}] ===== Observation =====", turn);

                        // CycleEnd hook (tool calls path)
                        {
                            let mut hook_ctx = HookContext::new(
                                HookPoint::CycleEnd,
                                &agent.agent_id,
                                &agent.role.to_string(),
                            )
                            .with_task(&ctx.task_iri, &ctx.task_iri)
                            .with_data("turn", Value::Number(turn.into()))
                            .with_data("had_tool_calls", Value::Bool(true));
                            self.hook_manager
                                .execute(HookPoint::CycleEnd, &mut hook_ctx)
                                .await;
                        }

                        continue;
                    } else {
                        warn!("[ReAct] action=tool_call 但无 tool_calls，继续思考");
                        let asst_summary = parsed.summary.clone()
                            .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                        messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: asst_summary,
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                            reasoning_content: reasoning_content.clone(),
                        });

                        // CycleEnd hook
                        {
                            let mut hook_ctx = HookContext::new(
                                HookPoint::CycleEnd,
                                &agent.agent_id,
                                &agent.role.to_string(),
                            )
                            .with_task(&ctx.task_iri, &ctx.task_iri)
                            .with_data("turn", Value::Number(turn.into()))
                            .with_data("had_tool_calls", Value::Bool(false));
                            self.hook_manager
                                .execute(HookPoint::CycleEnd, &mut hook_ctx)
                                .await;
                        }
                    }
                }
                "continue" => {
                    let asst_summary = parsed.summary.clone()
                        .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: asst_summary,
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: reasoning_content.clone(),
                    });

                    // CycleEnd hook
                    {
                        let mut hook_ctx = HookContext::new(
                            HookPoint::CycleEnd,
                            &agent.agent_id,
                            &agent.role.to_string(),
                        )
                        .with_task(&ctx.task_iri, &ctx.task_iri)
                        .with_data("turn", Value::Number(turn.into()))
                        .with_data("had_tool_calls", Value::Bool(false));
                        self.hook_manager
                            .execute(HookPoint::CycleEnd, &mut hook_ctx)
                            .await;
                    }
                }
                _ => {
                    warn!("[ReAct] 未知 action: {}, 继续思考", action);
                    let asst_summary = parsed.summary.clone()
                        .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: asst_summary,
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: reasoning_content.clone(),
                    });

                    // CycleEnd hook
                    {
                        let mut hook_ctx = HookContext::new(
                            HookPoint::CycleEnd,
                            &agent.agent_id,
                            &agent.role.to_string(),
                        )
                        .with_task(&ctx.task_iri, &ctx.task_iri)
                        .with_data("turn", Value::Number(turn.into()))
                        .with_data("had_tool_calls", Value::Bool(false));
                        self.hook_manager
                            .execute(HookPoint::CycleEnd, &mut hook_ctx)
                            .await;
                    }
                }
            }
        }

        warn!("AgentRunner 未完成: {} turns, errors: {:?}", turn, errs);
        Ok(TaskResult {
            task_iri: ctx.task_iri,
            status: "failed".to_string(),
            summary: String::new(),
            output: None,
            jsonld_output: None,
            artifacts: vec![],
            errors: errs,
            turn_count: turn,
            tool_call_count: tc,
            five_w2h_updates: None,
        })
    }

    fn extract_summary(&self, content: &str, reasoning_content: Option<&str>) -> String {
        // 优先从 JSON 中提取 summary 字段
        if let Ok(parsed) = serde_json::from_str::<Value>(content) {
            if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                return summary.chars().take(500).collect();
            }
            // 如果有原生思考，不需要从 JSON 中提取 thought（避免重复）
            if reasoning_content.is_none() {
                if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                    return thought.chars().take(500).collect();
                }
            }
            if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                return content_str.chars().take(500).collect();
            }
        }

        // 如果有原生思考内容，使用它作为 summary
        if let Some(reasoning) = reasoning_content {
            let reasoning_summary: String = reasoning.chars().take(300).collect();
            return format!("[思考] {}", reasoning_summary);
        }

        // 最后 fallback：直接使用 content 前 500 字符
        content.chars().take(500).collect()
    }

    fn parse_llm_response(
        &self,
        content: &str,
        reasoning_content: Option<&str>,
        supports_native_reasoning: bool,
    ) -> LlmParsedResponse {
        let mut response = LlmParsedResponse {
            thought: None,
            content: content.to_string(),
            summary: None,
            action: None,
            is_valid_json: false,
            has_native_reasoning: reasoning_content.is_some(),
            emphasis: Vec::new(),
        };

        // 如果有原生思考内容，直接使用
        if let Some(reasoning) = reasoning_content {
            response.thought = Some(reasoning.to_string());
            response.has_native_reasoning = true;
        }

        // 尝试解析 JSON
        if let Ok(parsed) = serde_json::from_str::<Value>(content) {
            response.is_valid_json = true;

            // 提取 summary
            if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                response.summary = Some(summary.to_string());
            }

            // 提取 content
            if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                response.content = content_str.to_string();
            }

            // 提取 thought（仅当模型不支持原生思考时）
            if !supports_native_reasoning {
                if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                    response.thought = Some(thought.to_string());
                }
            }

            // 提取 emphasis 字段（LLM 自己识别的强调内容）
            if let Some(emphasis) = parsed.get("emphasis") {
                if let Some(arr) = emphasis.as_array() {
                    response.emphasis = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                } else if let Some(s) = emphasis.as_str() {
                    response.emphasis = vec![s.to_string()];
                }
            }

            let content_text = parsed.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let keyword_emphasis = Self::extract_emphasis_by_keywords(content_text);
            for kw_em in keyword_emphasis {
                if !response.emphasis.iter().any(|e| e == &kw_em) {
                    response.emphasis.push(kw_em);
                }
            }

            // 提取 action 字段
            if let Some(action) = parsed.get("action").and_then(|a| a.as_str()) {
                response.action = Some(action.to_string());
            }
        } else {
            if let Some(extracted) = Self::try_extract_json_from_markdown(content) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&extracted) {
                    response.is_valid_json = true;
                    if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                        response.summary = Some(summary.to_string());
                    }
                    if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                        response.content = content_str.to_string();
                    }
                    if !supports_native_reasoning {
                        if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                            response.thought = Some(thought.to_string());
                        }
                    }
                    if let Some(action) = parsed.get("action").and_then(|a| a.as_str()) {
                        response.action = Some(action.to_string());
                    }
                } else {
                    response.summary = Some(Self::generate_auto_summary(content));
                }
            } else {
                response.summary = Some(Self::generate_auto_summary(content));
            }
        }

        response
    }

    fn generate_auto_summary(content: &str) -> String {
        let content_clean = content.trim();
        if content_clean.len() <= 200 {
            return content_clean.to_string();
        }

        if let Some(first_sentence_end) =
            content_clean.find(|c| c == '。' || c == '.' || c == '！' || c == '!')
        {
            let end_byte = first_sentence_end
                + content_clean[first_sentence_end..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            if end_byte <= 200 {
                return content_clean[..end_byte].to_string();
            }
        }

        content_clean.chars().take(200).collect()
    }

    fn try_extract_json_from_markdown(content: &str) -> Option<String> {
        if let Some(start) = content.find("```json") {
            let json_start = start + 7;
            if let Some(end) = content[json_start..].find("```") {
                return Some(content[json_start..json_start + end].trim().to_string());
            }
        }
        if let Some(start) = content.find("```") {
            let json_start = start + 3;
            if let Some(end) = content[json_start..].find("```") {
                let candidate = content[json_start..json_start + end].trim();
                if candidate.starts_with('{') && candidate.ends_with('}') {
                    return Some(candidate.to_string());
                }
            }
        }
        if let Some(start) = content.find("{\n  \"thought\"") {
            if let Some(end) = content.rfind('}') {
                if end > start {
                    return Some(content[start..=end].to_string());
                }
            }
        }
        let trimmed = content.trim();
        if let Some(bracket_end) = trimmed.rfind('}') {
            if let Some(bracket_start) = trimmed.find('{') {
                if bracket_start < bracket_end {
                    let candidate = &trimmed[bracket_start..=bracket_end];
                    if candidate.contains("\"thought\"") || candidate.contains("\"action\"") {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
        None
    }

    async fn save_emphasis_to_l0(
        &self,
        emphasis_items: &[String],
        task_iri: &str,
        agent_id: &str,
        dedup_threshold: f64,
    ) {
        if emphasis_items.is_empty() {
            return;
        }

        // 先加载已有的强调内容用于去重
        let existing = self.load_emphasis_from_l0(task_iri).await;

        for content in emphasis_items {
            // 去重检测
            let is_duplicate = existing.iter().any(|existing_content| {
                let similarity = Self::calculate_similarity(content, existing_content);
                similarity >= dedup_threshold
            });

            if is_duplicate {
                debug!(
                    "[L0] 跳过重复强调内容: {}",
                    content.chars().take(50).collect::<String>()
                );
                continue;
            }

            let iri = format!(
                "iri://emphasis/{}/{}",
                task_iri.strip_prefix("iri://").unwrap_or(task_iri),
                uuid::Uuid::new_v4()
            );
            let node = json!({
                "@id": &iri,
                "@type": "EmphasisContent",
                "content": content,
                "task_iri": task_iri,
                "agent_id": agent_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "permanent": true
            });

            if let Err(e) = self.l0_store.store(&iri, &node.to_string()) {
                warn!("保存强调内容到 L0 失败: {}", e);
            } else {
                info!("[L0] 保存强调内容: {} -> {}", agent_id, &iri);
            }
        }
    }

    async fn load_emphasis_from_l0(&self, task_iri: &str) -> Vec<String> {
        let mut result = Vec::new();

        // 从 L0 搜索所有强调内容
        if let Ok(nodes) = self.l0_store.search_by_tags(&[String::from("emphasis")]) {
            for node in nodes {
                if let Ok(parsed) = serde_json::from_str::<Value>(&node.content) {
                    // 检查是否属于当前任务或全局
                    let is_global = parsed.get("task_iri").is_none();
                    let is_current_task = parsed
                        .get("task_iri")
                        .and_then(|t| t.as_str())
                        .map(|t| t == task_iri || task_iri.contains(t))
                        .unwrap_or(false);

                    if is_global || is_current_task {
                        if let Some(content) = parsed.get("content").and_then(|c| c.as_str()) {
                            result.push(content.to_string());
                        }
                    }
                }
            }
        }

        result
    }

    fn calculate_similarity(a: &str, b: &str) -> f64 {
        if a == b {
            return 1.0;
        }

        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();

        if a_chars.is_empty() || b_chars.is_empty() {
            return 0.0;
        }

        // 使用简单的 Jaccard 相似度
        let a_set: std::collections::HashSet<char> = a_chars.iter().copied().collect();
        let b_set: std::collections::HashSet<char> = b_chars.iter().copied().collect();

        let intersection = a_set.intersection(&b_set).count();
        let union = a_set.union(&b_set).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f64 / union as f64
    }

    fn build_readable_tool_menu(&self, role: &AgentRole) -> String {
        let role_str = role.to_string();
        let tool_defs = self.tool_executor.read().unwrap().tool_definitions_for_role(&role_str);

        if tool_defs.is_empty() {
            return String::new();
        }

        let mut lines = vec!["可用工具列表：".to_string()];
        for tool_def in &tool_defs {
            let name = tool_def["function"]["name"].as_str().unwrap_or("");
            let desc = tool_def["function"]["description"].as_str().unwrap_or("");
            if desc.is_empty() {
                lines.push(format!("- ID: {}", name));
            } else {
                lines.push(format!("- ID: {} | 用途: {}", name, desc));
            }
        }
        lines.join("\n")
    }

    fn parse_jsonld_response(&self, response: &str) -> Result<JsonLdNode, CoreError> {
        let parsed: Value =
            serde_json::from_str(response).map_err(|e| CoreError::InvalidJsonLd {
                message: format!("Failed to parse JSON: {}", e),
            })?;

        if let Err(e) = validate_jsonld_node(&parsed) {
            return Err(CoreError::InvalidJsonLd {
                message: format!("Invalid JSON-LD node: {}", e),
            });
        }

        JsonLdNode::from_json(&parsed).map_err(|e| CoreError::InvalidJsonLd {
            message: format!("Failed to parse JsonLdNode: {}", e),
        })
    }

    fn extract_emphasis(&self, node: &JsonLdNode) -> Vec<String> {
        let mut emphasis_items = Vec::new();

        if let Some(emphasis) = node.get_property("emphasis") {
            match emphasis {
                Value::Array(arr) => {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            if !s.is_empty() {
                                emphasis_items.push(s.to_string());
                            }
                        }
                    }
                }
                Value::String(s) => {
                    if !s.is_empty() {
                        emphasis_items.push(s.clone());
                    }
                }
                _ => {}
            }
        }

        if let Some(constraints) = node.get_property("constraints") {
            if let Some(arr) = constraints.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.is_empty() {
                            emphasis_items.push(format!("[约束] {}", s));
                        }
                    }
                }
            }
        }

        emphasis_items
    }

    fn extract_emphasis_by_keywords(text: &str) -> Vec<String> {
        let keywords = [
            "必须", "重要", "关键", "务必", "不要忘记", "切记", "一定",
            "禁止", "不允许", "注意", "千万不要", "绝不能",
            "MUST", "IMPORTANT", "CRITICAL", "NEVER", "ALWAYS",
            "REQUIRED", "MANDATORY", "ESSENTIAL", "WARNING",
        ];
        let mut results = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            for keyword in &keywords {
                if trimmed.contains(keyword) {
                    let clean = if trimmed.len() > 200 {
                        let mut end = 200;
                        while end > 0 && !trimmed.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &trimmed[..end])
                    } else {
                        trimmed.to_string()
                    };
                    if !results.contains(&clean) {
                        results.push(clean);
                    }
                    break;
                }
            }
        }
        results
    }

    fn apply_output_mapping(
        &self,
        output: &Value,
        role: &AgentRole,
        task_iri: &str,
    ) -> Option<Value> {
        let output_mapping = match role {
            AgentRole::Plan => HashMap::from([
                ("plan".to_string(), "execution_plan".to_string()),
                ("steps".to_string(), "plan_steps".to_string()),
                ("objective".to_string(), "task_objective".to_string()),
            ]),
            AgentRole::Do => HashMap::from([
                ("result".to_string(), "execution_result".to_string()),
                ("output".to_string(), "do_output".to_string()),
                ("artifacts".to_string(), "created_artifacts".to_string()),
            ]),
            AgentRole::Check => HashMap::from([
                ("review".to_string(), "check_review".to_string()),
                ("issues".to_string(), "found_issues".to_string()),
                ("passed".to_string(), "check_passed".to_string()),
            ]),
            AgentRole::Act => HashMap::from([
                ("decision".to_string(), "final_decision".to_string()),
                ("action".to_string(), "recommended_action".to_string()),
                ("summary".to_string(), "act_summary".to_string()),
            ]),
        };

        let node_id = generate_iri(
            "task",
            &format!(
                "{}_{}",
                role.to_string().to_lowercase(),
                uuid::Uuid::new_v4()
            ),
        );
        let mut node = JsonLdNode::new(node_id.clone(), format!("{}Output", role.to_string()))
            .with_context((*JsonLdContext::context_value()).clone());

        if let Some(obj) = output.as_object() {
            for (key, value) in obj {
                let mapped_key = output_mapping
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| key.clone());
                node = node.with_property(mapped_key, value.clone());
            }
        } else {
            node = node.with_property("content".to_string(), output.clone());
        }

        node = node.with_property("task_iri".to_string(), Value::String(task_iri.to_string()));
        node = node.with_property("agent_role".to_string(), Value::String(role.to_string()));
        node = node.with_property(
            "timestamp".to_string(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );

        node.to_json().ok()
    }

    async fn store_jsonld_to_l2(
        &self,
        node: &JsonLdNode,
        task_iri: &str,
    ) -> Result<String, CoreError> {
        let node_iri = node.id.clone();
        let node_json = node.to_json().map_err(|e| CoreError::Internal {
            message: format!("Failed to serialize JsonLdNode: {}", e),
        })?;

        let cfg = crate::CoreConfig::default();
        self.blackboard
            .write_node(&node_iri, &node_json.to_string(), &cfg)?;

        info!("[L2] 存储 JSON-LD 节点: {} for task {}", node_iri, task_iri);
        Ok(node_iri)
    }

    pub async fn execute_streaming<F>(
        &self,
        agent: &mut AgentInstance,
        ctx: TaskContext,
        mut on_event: F,
    ) -> Result<TaskResult, CoreError>
    where
        F: FnMut(&crate::llm::StreamEvent) + Send,
    {
        agent.status = AgentStatus::Running;

        let mut session = self.memory_manager.lock().await.create_session(
            &agent.agent_id,
            &agent.role.to_string(),
            &ctx.task_iri,
        );

        let model = self
            .gateway
            .get_model(&agent.role.to_string().to_lowercase());
        let supports_reasoning = self.gateway.supports_native_reasoning(&model);

        let context_data = self.gather_context_data_async(agent.role, &ctx).await;
        let agent_md = self.build_agent_md(agent.role, &ctx.objective, &context_data, &model);

        let mut prompt_builder = SystemPromptBuilder::new();
        prompt_builder.set_region(SystemPromptRegion::RoleDefinition, agent_md.clone());

        let emphasis_items = self.load_emphasis_from_l0(&ctx.task_iri).await;
        if !emphasis_items.is_empty() {
            let emphasis_content = emphasis_items
                .iter()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            prompt_builder.set_region(SystemPromptRegion::EmphasizedConstraints, emphasis_content);
        }

        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT.to_string()
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT.to_string()
        };
        prompt_builder.set_region(SystemPromptRegion::OutputFormat, format_constraint);

        let tool_menu = self.build_readable_tool_menu(&agent.role);
        if !tool_menu.is_empty() {
            prompt_builder.set_region(SystemPromptRegion::Tools, tool_menu);
        }

        let system_content = prompt_builder.build();

        let summary_chain = session.get_summary_chain();
        let summary_text = summary_chain
            .first()
            .and_then(|v| v.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let context_msg = if summary_text.is_empty() {
            format!(
                "## 当前任务\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective
            )
        } else {
            format!(
                "## 当前任务\n{}\n\n## 历史摘要\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective, summary_text
            )
        };

        let messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_content,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: context_msg,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        ];

        let tools = self
            .tool_executor
            .read()
            .unwrap()
            .tool_definitions_for_role(&agent.role.to_string());

        info!(
            "AgentRunner Streaming 开始: role={}, model={}, tools={}",
            agent.role,
            model,
            tools.len()
        );

        let mut stream = self
            .gateway
            .stream_chat_with_params(
                &model,
                messages,
                None,
                None,
                if tools.is_empty() { None } else { Some(tools.clone()) },
                None,
            )
            .await?;

        let mut accumulator = crate::llm::StreamAccumulator::new();
        let mut tc = 0u32;
        let mut turn = 0u32;
        let mut errs = Vec::new();

        while let Some(event) = stream.next_event().await.map_err(|e| CoreError::Internal {
            message: e.to_string(),
        })? {
            on_event(&event);
            accumulator.process_event(&event);

            if let crate::llm::StreamEvent::MessageStop(_) = event {
                break;
            }
        }

        let stream_response: crate::llm::StreamResponse = accumulator.into();

        let parsed = self.parse_llm_response(
            &stream_response.content,
            stream_response.thought.as_deref(),
            supports_reasoning,
        );

        let final_summary = parsed.summary.clone()
                .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));

        // 归档到 L0
        let l0_iri = session
            .archive_full_to_l0(
                &self.l0_store,
                &agent.role.to_string(),
                &parsed.thought.clone().unwrap_or_default(),
                &parsed.content,
            )
            .ok();

        // 保存 summary 到 L1 Session
        let summary_text = parsed.summary.clone()
            .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
        session.add_summary(
            &agent.role.to_string(),
            &summary_text,
            l0_iri.clone(),
        );

        let task_id = ctx.task_iri
            .strip_prefix("iri://task/")
            .unwrap_or_else(|| ctx.task_iri.strip_prefix("iri://").unwrap_or(&ctx.task_iri));
        let node_iri = format!("iri://task/{}/turn_{}", task_id, turn);
        let mut node_json = json!({
            "@id": &node_iri,
            "@type": "AgentTurn",
            "role": agent.role.to_string(),
            "content_len": parsed.content.len(),
            "is_valid_json": parsed.is_valid_json,
            "has_native_reasoning": parsed.has_native_reasoning,
        });
        if let Some(ref thought) = parsed.thought {
            node_json["has_thought"] = Value::Bool(true);
            node_json["thought_len"] = Value::Number(thought.len().into());
        }
        if let Some(ref act) = parsed.action {
            node_json["action"] = Value::String(act.clone());
        }
        if let Some(ref s) = parsed.summary {
            node_json["summary"] = Value::String(s.clone());
        }

        let tool_calls = stream_response.tool_calls;
        if !tool_calls.is_empty() {
            info!("[Streaming] 处理 {} 个工具调用", tool_calls.len());
        }

        let output_value = Value::String(parsed.content.clone());
        let jsonld_output = self.apply_output_mapping(&output_value, &agent.role, &ctx.task_iri);

        info!("AgentRunner Streaming 完成: {} tools", tc);

        let summary = session.summarize();
        {
            let m = self.memory_manager.lock().await;
            let _ = m.archive_to_l2(&ctx.task_iri, &summary);
            let _ = m.archive_to_l0(&summary);
        }

        Ok(TaskResult {
            task_iri: ctx.task_iri,
            status: "success".to_string(),
            summary: final_summary,
            output: Some(output_value),
            jsonld_output,
            artifacts: vec![],
            errors: errs,
            turn_count: turn,
            tool_call_count: tc,
            five_w2h_updates: None,
        })
    }

    async fn route_tool_result(
        &self,
        result_str: &str,
        tool_name: &str,
        call_id: &str,
    ) -> String {
        use crate::config::settings::ToolResultRouterSettings;
        use crate::tools::result_router::router::ResultRouter;
        use crate::tools::result_router::summary;
        use crate::tools::result_router::RouteDecision;
        use crate::tools::result_router::graphify::GraphifyEngine;
        use crate::tools::result_router::micro_tools::MicroToolGenerator;
        use crate::tools::tool_executor::MicroToolContext;

        let settings = ToolResultRouterSettings::default();
        let router = ResultRouter::new(&settings);

        let decision = router.route(result_str, tool_name, call_id);

        match decision {
            RouteDecision::PassThrough => result_str.to_string(),

            RouteDecision::Truncate { max_chars } => {
                if result_str.len() <= max_chars {
                    return result_str.to_string();
                }
                summary::smart_truncate(result_str, max_chars)
            }

            RouteDecision::Graphify { call_id, graph_name } => {
                let storage_key = format!("iri://tool-result/{}", call_id);
                
                let parsed: Option<serde_json::Value> = serde_json::from_str(result_str.trim()).ok();
                match parsed {
                    Some(json_val) => {
                        match GraphifyEngine::new(settings.max_graph_entities) {
                            Ok(mut engine) => {
                                let graphify_result = engine.graphify_json(
                                    &json_val,
                                    &call_id,
                                    settings.max_graph_entities,
                                );

                                let analysis = crate::tools::result_router::SchemaAnalysis {
                                    entity_types: graphify_result.entity_types.iter().map(|t| (t.clone(), 0)).collect(),
                                    relation_types: vec![],
                                    property_names: vec![],
                                    total_entities: graphify_result.entity_count,
                                    total_relations: graphify_result.relation_count,
                                };

                                let micro_tools = MicroToolGenerator::generate_from_schema(
                                    &analysis,
                                    &call_id,
                                    settings.max_micro_tools,
                                );

                                for mt in &micro_tools {
                                    let ctx = MicroToolContext {
                                        call_id: call_id.clone(),
                                        storage_key: storage_key.clone(),
                                        tool_name: tool_name.to_string(),
                                        entity_types: vec![],
                                        preview_size: settings.preview_size,
                                    };
                                    if let Ok(mut exe) = self.tool_executor.write() {
                                        exe.register_micro_tool(&mt.name, ctx);
                                    }
                                }

                                if let Ok(mut exe) = self.tool_executor.write() {
                                    exe.store_micro_tool_data(&storage_key, json_val.clone());
                                }

                                let injection_msg = MicroToolGenerator::format_tool_injection_message(
                                    &graphify_result.summary,
                                    &micro_tools,
                                );

                                info!(
                                    "[ResultRouter] 图谱化: {} 个实体, {} 个关系, {} 个微工具, graph={}",
                                    graphify_result.entity_count,
                                    graphify_result.relation_count,
                                    micro_tools.len(),
                                    graph_name,
                                );

                                injection_msg
                            }
                            Err(e) => {
                                warn!("[ResultRouter] 图谱化失败: {}, 回退到截断", e);
                                summary::smart_truncate(result_str, settings.threshold_large)
                            }
                        }
                    }
                    None => {
                        summary::generate_text_summary(result_str, tool_name, settings.preview_size)
                    }
                }
            }

            RouteDecision::Summarize { call_id, preview_size } => {
                let storage_key = format!("iri://tool-result/{}", call_id);
                
                if let Ok(mut exe) = self.tool_executor.write() {
                    exe.store_micro_tool_data(&storage_key, serde_json::json!({
                        "content": result_str,
                        "tool_name": tool_name,
                    }));
                }

                let read_tool_name = format!("read_full_result_{}", call_id);
                let ctx = MicroToolContext {
                    call_id: call_id.clone(),
                    storage_key: storage_key.clone(),
                    tool_name: tool_name.to_string(),
                    entity_types: vec![],
                    preview_size,
                };

                if let Ok(mut exe) = self.tool_executor.write() {
                    exe.register_micro_tool(&read_tool_name, ctx);
                }

                let preview = summary::generate_text_summary(result_str, tool_name, preview_size);
                let micro_tools = vec![crate::tools::result_router::MicroToolSchema {
                    name: read_tool_name.clone(),
                    description: format!("读取 {} 的完整结果", tool_name),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "offset": {"type": "integer", "description": "起始行"},
                            "limit": {"type": "integer", "description": "返回行数"}
                        }
                    }),
                    tool_type: crate::tools::result_router::MicroToolType::FullTextRead {
                        storage_key: storage_key.clone(),
                    },
                }];

                let injection_msg = MicroToolGenerator::format_tool_injection_message(
                    &preview,
                    &micro_tools,
                );

                info!(
                    "[ResultRouter] 摘要化: {} 字节 -> 预览 {} 字节, 微工具: {}",
                    result_str.len(),
                    preview_size,
                    read_tool_name,
                );

                injection_msg
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_test_runner() -> AgentRunner {
        use crate::config::settings::AgentSettings;
        use crate::gateway::unified_gateway::UnifiedGateway;
        use crate::memory::l0_store::L0Store;
        use crate::memory::l2_blackboard::Blackboard;
        use crate::memory::memory_manager::MemoryManager;
        use crate::templates::template_engine::TemplateEngine;
        use crate::tools::skill_registry::SkillRegistry;
        use crate::config::settings::GatewaySettings;
        use crate::CoreConfig;
        use std::path::Path;

        let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_path = format!("./data/test_l0_{}", test_id);
        let l0 = Arc::new(L0Store::new(&test_path).unwrap());
        let blackboard = Arc::new(Blackboard::new().unwrap());
        let projection = Arc::new(ProjectionEngine::new(blackboard.clone(), 1024));
        let skills = Arc::new(SkillRegistry::new());
        let gateway_settings = GatewaySettings {
            base_url: "http://localhost:3000".to_string(),
            api_key: "test-key".to_string(),
            default_model: "gpt-4".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            model_mapping: std::collections::HashMap::new(),
        };
        let gateway = Arc::new(UnifiedGateway::new(&gateway_settings).unwrap());
        let templates = Arc::new(TemplateEngine::new(Path::new("./templates")).unwrap());
        let config = CoreConfig::default();
        let memory_manager = Arc::new(tokio::sync::Mutex::new(MemoryManager::new(
            l0.clone(),
            blackboard.clone(),
            projection,
            config.clone(),
        )));
        let settings = AgentSettings::default();

        AgentRunner::new(
            gateway,
            skills,
            blackboard,
            l0,
            memory_manager,
            templates,
            settings,
        )
    }

    #[test]
    fn test_parse_jsonld_response_valid() {
        let runner = create_test_runner();
        let response = json!({
            "@context": "https://pdca-agent.org/context/task",
            "@id": "iri://task/test123",
            "@type": "TaskNode",
            "summary": "Test task",
            "emphasis": ["重要约束1", "重要约束2"]
        })
        .to_string();

        let result = runner.parse_jsonld_response(&response);
        assert!(result.is_ok());

        let node = result.unwrap();
        assert_eq!(node.id, "iri://task/test123");
        assert_eq!(node.get_property("summary"), Some(&json!("Test task")));
    }

    #[test]
    fn test_parse_jsonld_response_invalid() {
        let runner = create_test_runner();
        let response = json!({
            "summary": "Missing @id and @type"
        })
        .to_string();

        let result = runner.parse_jsonld_response(&response);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_emphasis_from_array() {
        let runner = create_test_runner();
        let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
            .with_property("emphasis".to_string(), json!(["约束1", "约束2", "约束3"]));

        let emphasis = runner.extract_emphasis(&node);
        assert_eq!(emphasis.len(), 3);
        assert_eq!(emphasis[0], "约束1");
    }

    #[test]
    fn test_extract_emphasis_from_string() {
        let runner = create_test_runner();
        let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
            .with_property("emphasis".to_string(), json!("单个强调内容"));

        let emphasis = runner.extract_emphasis(&node);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "单个强调内容");
    }

    #[test]
    fn test_extract_emphasis_with_constraints() {
        let runner = create_test_runner();
        let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
            .with_property("emphasis".to_string(), json!(["强调1"]))
            .with_property("constraints".to_string(), json!(["约束A", "约束B"]));

        let emphasis = runner.extract_emphasis(&node);
        assert_eq!(emphasis.len(), 3);
        assert!(emphasis.contains(&"强调1".to_string()));
        assert!(emphasis.contains(&"[约束] 约束A".to_string()));
    }

    #[test]
    fn test_apply_output_mapping_plan() {
        let runner = create_test_runner();
        let output = json!({
            "plan": "执行计划内容",
            "steps": ["步骤1", "步骤2"],
            "objective": "任务目标"
        });

        let result = runner.apply_output_mapping(&output, &AgentRole::Plan, "iri://task/123");
        assert!(result.is_some());

        let jsonld = result.unwrap();
        assert!(jsonld.get("@id").is_some());
        assert_eq!(jsonld.get("execution_plan"), Some(&json!("执行计划内容")));
        assert_eq!(jsonld.get("plan_steps"), Some(&json!(["步骤1", "步骤2"])));
        assert_eq!(jsonld.get("task_iri"), Some(&json!("iri://task/123")));
        assert_eq!(jsonld.get("agent_role"), Some(&json!("PA")));
    }

    #[test]
    fn test_apply_output_mapping_do() {
        let runner = create_test_runner();
        let output = json!({
            "result": "执行结果",
            "artifacts": ["文件1.py", "文件2.rs"]
        });

        let result = runner.apply_output_mapping(&output, &AgentRole::Do, "iri://task/456");
        assert!(result.is_some());

        let jsonld = result.unwrap();
        assert_eq!(jsonld.get("execution_result"), Some(&json!("执行结果")));
        assert_eq!(
            jsonld.get("created_artifacts"),
            Some(&json!(["文件1.py", "文件2.rs"]))
        );
    }

    #[test]
    fn test_apply_output_mapping_check() {
        let runner = create_test_runner();
        let output = json!({
            "review": "检查结果良好",
            "passed": true
        });

        let result = runner.apply_output_mapping(&output, &AgentRole::Check, "iri://task/789");
        assert!(result.is_some());

        let jsonld = result.unwrap();
        assert_eq!(jsonld.get("check_review"), Some(&json!("检查结果良好")));
        assert_eq!(jsonld.get("check_passed"), Some(&json!(true)));
    }

    #[test]
    fn test_apply_output_mapping_act() {
        let runner = create_test_runner();
        let output = json!({
            "decision": "最终决策",
            "action": "执行下一步"
        });

        let result = runner.apply_output_mapping(&output, &AgentRole::Act, "iri://task/abc");
        assert!(result.is_some());

        let jsonld = result.unwrap();
        assert_eq!(jsonld.get("final_decision"), Some(&json!("最终决策")));
        assert_eq!(jsonld.get("recommended_action"), Some(&json!("执行下一步")));
    }

    #[test]
    fn test_apply_output_mapping_string_output() {
        let runner = create_test_runner();
        let output = json!("简单的字符串输出");

        let result = runner.apply_output_mapping(&output, &AgentRole::Do, "iri://task/xyz");
        assert!(result.is_some());

        let jsonld = result.unwrap();
        assert_eq!(jsonld.get("content"), Some(&json!("简单的字符串输出")));
    }

    #[test]
    fn test_task_result_jsonld_output() {
        let result = TaskResult {
            task_iri: "iri://task/test".to_string(),
            status: "success".to_string(),
            summary: "任务完成".to_string(),
            output: Some(json!("输出内容")),
            jsonld_output: Some(json!({
                "@id": "iri://task/test_output",
                "@type": "DoOutput",
                "content": "输出内容"
            })),
            artifacts: vec![],
            errors: vec![],
            turn_count: 5,
            tool_call_count: 3,
            five_w2h_updates: None,
        };

        assert!(result.jsonld_output.is_some());
        let jsonld = result.jsonld_output.unwrap();
        assert_eq!(jsonld.get("@id"), Some(&json!("iri://task/test_output")));
    }
}
