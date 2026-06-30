use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, instrument, warn};

use crate::core::agent_instance::{AgentInstance, AgentRole};
use crate::core::agent_runner::{AgentRunner, TaskContext, TaskResult};
use crate::core::event_bus::{EventBus, Event, EventPriority};
use crate::core::execution_event::{ExecutionEvent, ExecutionEventKind, Thought};
use crate::core::relevance_tracker::RelevanceTracker;
use crate::core::supplementary_store::SupplementaryInputStore;
use crate::jsonld::type_router::TypeRouter;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::memory::EmbeddingService;
use crate::perception::proactive_engine::ProactiveEngine;
use crate::templates::template_engine::TemplateEngine;
use crate::tools::sharing::{SharingProtocol, ShareType, Permission};
use crate::tools::skill_registry::SkillRegistry;
use crate::CoreError;

/// 5 类 16 个预定义干预动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterventionAction {
    // === 1. 正常继续 (Normal Continuation) ===
    Continue,
    ContinueWithMonitor,

    // === 2. 参数调整 (Parameter Tuning) ===
    IncreaseRetry { additional_retries: u32 },
    IncreaseTimeout { additional_seconds: u64 },
    ReduceComplexity,
    RestrictTools { allowed_tools: Vec<String> },

    // === 3. 执行流调整 (Execution Flow Adjustment) ===
    SkipStep { step_id: String },
    RetryStep { step_id: String },
    Parallelize,
    SplitStep { step_id: String, sub_steps: Vec<String> },
    InsertExtraStep { description: String },

    // === 4. 资源与模式切换 (Resource & Mode Switch) ===
    FallbackToShallow,
    EmergencyMode,
    IncreaseBudget { additional_tokens: u64, additional_time_secs: u64 },
    FreezeAndReport,

    // === 5. 终止与升级 (Termination & Escalation) ===
    AbortTask { reason: String },
    NotifyHuman { message: String },
}

impl InterventionAction {
    pub fn from_name(name: &str, params: ActionParams) -> Result<Self, CoreError> {
        match name {
            "Continue" => Ok(InterventionAction::Continue),
            "ContinueWithMonitor" => Ok(InterventionAction::ContinueWithMonitor),
            "IncreaseRetry" => Ok(InterventionAction::IncreaseRetry {
                additional_retries: params.additional_retries.unwrap_or(3),
            }),
            "IncreaseTimeout" => Ok(InterventionAction::IncreaseTimeout {
                additional_seconds: params.additional_seconds.unwrap_or(60),
            }),
            "ReduceComplexity" => Ok(InterventionAction::ReduceComplexity),
            "RestrictTools" => Ok(InterventionAction::RestrictTools {
                allowed_tools: params.allowed_tools.unwrap_or_default(),
            }),
            "SkipStep" => Ok(InterventionAction::SkipStep {
                step_id: params.step_id.clone().unwrap_or_default(),
            }),
            "RetryStep" => Ok(InterventionAction::RetryStep {
                step_id: params.step_id.clone().unwrap_or_default(),
            }),
            "Parallelize" => Ok(InterventionAction::Parallelize),
            "SplitStep" => Ok(InterventionAction::SplitStep {
                step_id: params.step_id.clone().unwrap_or_default(),
                sub_steps: params.sub_steps.unwrap_or_default(),
            }),
            "InsertExtraStep" => Ok(InterventionAction::InsertExtraStep {
                description: params.description.clone().unwrap_or_default(),
            }),
            "FallbackToShallow" => Ok(InterventionAction::FallbackToShallow),
            "EmergencyMode" => Ok(InterventionAction::EmergencyMode),
            "IncreaseBudget" => Ok(InterventionAction::IncreaseBudget {
                additional_tokens: params.additional_tokens.unwrap_or(1000),
                additional_time_secs: params.additional_time_secs.unwrap_or(120),
            }),
            "FreezeAndReport" => Ok(InterventionAction::FreezeAndReport),
            "AbortTask" => Ok(InterventionAction::AbortTask {
                reason: params.reason.clone().unwrap_or_default(),
            }),
            "NotifyHuman" => Ok(InterventionAction::NotifyHuman {
                message: params.message.clone().unwrap_or_default(),
            }),
            _ => Err(CoreError::Internal {
                message: format!("Unknown intervention action: {}", name),
            }),
        }
    }
}

/// 动作参数（LLM 输出的结构化参数）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionParams {
    pub additional_retries: Option<u32>,
    pub additional_seconds: Option<u64>,
    pub additional_tokens: Option<u64>,
    pub additional_time_secs: Option<u64>,
    pub step_id: Option<String>,
    pub sub_steps: Option<Vec<String>>,
    pub description: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub reason: Option<String>,
    pub message: Option<String>,
}

/// LLM 分类决策的中间结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmActionDecision {
    action: String,
    #[serde(default)]
    params: ActionParams,
    reasoning: Option<String>,
}

/// 4 类 12 个预定义用户补充输入动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupplementaryInputAction {
    // === 1. 信息补充 (Information Supplement) ===
    AddContext,
    RefineObjective,
    ProvideConstraint,

    // === 2. 方向引导 (Direction Guidance) ===
    GuideDirection,
    PrioritizeStep,
    SuggestApproach,

    // === 3. 执行控制 (Execution Control) ===
    PauseExecution,
    ResumeExecution,
    SkipCurrentStep,

    // === 4. 反馈纠正 (Feedback & Correction) ===
    ConfirmDirection,
    CorrectApproach,
    AbortCurrentStep,
}

impl SupplementaryInputAction {
    pub fn from_name(name: &str) -> Result<Self, CoreError> {
        match name {
            "AddContext" => Ok(SupplementaryInputAction::AddContext),
            "RefineObjective" => Ok(SupplementaryInputAction::RefineObjective),
            "ProvideConstraint" => Ok(SupplementaryInputAction::ProvideConstraint),
            "GuideDirection" => Ok(SupplementaryInputAction::GuideDirection),
            "PrioritizeStep" => Ok(SupplementaryInputAction::PrioritizeStep),
            "SuggestApproach" => Ok(SupplementaryInputAction::SuggestApproach),
            "PauseExecution" => Ok(SupplementaryInputAction::PauseExecution),
            "ResumeExecution" => Ok(SupplementaryInputAction::ResumeExecution),
            "SkipCurrentStep" => Ok(SupplementaryInputAction::SkipCurrentStep),
            "ConfirmDirection" => Ok(SupplementaryInputAction::ConfirmDirection),
            "CorrectApproach" => Ok(SupplementaryInputAction::CorrectApproach),
            "AbortCurrentStep" => Ok(SupplementaryInputAction::AbortCurrentStep),
            _ => Err(CoreError::Internal {
                message: format!("Unknown supplementary input action: {}", name),
            }),
        }
    }
}

/// LLM 分类决策的中间结构（补充输入专用）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SupplementaryLlmDecision {
    action: String,
    #[serde(default)]
    params: ActionParams,
    reasoning: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskComplexity {
    Instant,
    Simple,
    Standard,
    Complex,
    Exploratory,
    Emergency,
    Recursive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub sub_task_id: String,
    pub objective: String,
    pub parent_step_id: String,
    pub depth: u32,
    pub status: String,
}

impl SubTask {
    pub fn new(objective: &str, parent_step_id: &str, depth: u32) -> Self {
        Self {
            sub_task_id: format!("sub_{}", uuid::Uuid::new_v4().hyphenated()),
            objective: objective.to_string(),
            parent_step_id: parent_step_id.to_string(),
            depth,
            status: "pending".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub step_id: String,
    pub role: AgentRole,
    pub objective: String,
    pub expected_output: String,
    pub dependencies: Vec<String>,
    pub tools_allowed: Vec<String>,
    pub success_criteria: String,
}

/// 人工审批节点的执行结果
#[derive(Debug, Clone)]
pub struct HumanApprovalNodeResult {
    pub node_id: String,
    pub approved: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub plan_id: String,
    pub agent_sequence: Vec<AgentRole>,
    pub parallel_groups: Vec<Vec<AgentRole>>,
    pub task_complexity: TaskComplexity,
    pub description: String,
    pub steps: Vec<PlanStep>,
    pub context_requirements: HashMap<String, String>,
    pub success_metrics: Vec<String>,
    pub max_recursion_depth: u32,
    pub sub_tasks: Vec<SubTask>,
    /// 原始 JSON-LD DAG 定义（从 --workflow 文件加载时设置）
    /// 用于在 execute_plan() 中保留 DAG 特性（条件分支、重试、并行）
    pub dag_jsonld: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CyclePhase {
    Idle,
    Analyzing,
    Dispatching,
    Executing,
    Monitoring,
    Completed,
}

#[derive(Debug, Clone)]
pub struct CycleState {
    pub cycle_id: String,
    pub task_iri: String,
    pub phase: CyclePhase,
    pub iteration: u32,
    pub max_iterations: u32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub phase_history: Vec<String>,
    pub task_completed: bool,
    pub experience_hints: Vec<String>,
}

pub struct SupervisorAgent {
    runner: Arc<AgentRunner>,
    template_engine: Arc<TemplateEngine>,
    skills: Arc<SkillRegistry>,
    event_bus: Arc<EventBus>,
    event_receiver: Option<broadcast::Receiver<Event>>,
    active_cycles: HashMap<String, CycleState>,
    max_iterations: u32,
    /// 最大 PDCA 循环重入次数（Recursive 任务专用，默认 7）
    max_pdca_cycles: u32,
    perception: ProactiveEngine,
    sharing: Arc<SharingProtocol>,
    blackboard: Option<Arc<Blackboard>>,
    prefetch_engine: Option<Arc<PrefetchEngine>>,
    scheduler: Option<Arc<MemoryScheduler>>,
    type_router: TypeRouter,
    pending_approvals: Arc<tokio::sync::Mutex<HashMap<String, bool>>>,
    supplementary_inputs: HashMap<String, Vec<(String, String)>>,
    /// 补充输入共享存储，SA 写入后由 AgentRunner 在 CycleStart 消费
    supplement_store: SupplementaryInputStore,
    /// 嵌入服务（可选，用于计算补充输入的 embedding 和 relevance_score）
    embedder: Option<Arc<dyn EmbeddingService>>,
    /// 相关性跟踪器
    relevance_tracker: RelevanceTracker,
}

impl SupervisorAgent {
    pub fn new(
        runner: Arc<AgentRunner>,
        template_engine: Arc<TemplateEngine>,
        skills: Arc<SkillRegistry>,
        event_bus: Arc<EventBus>,
        max_iterations: u32,
    ) -> Self {
        Self::with_pdca_cycles(runner, template_engine, skills, event_bus, max_iterations, 7)
    }

    pub fn with_pdca_cycles(
        mut runner: Arc<AgentRunner>,
        template_engine: Arc<TemplateEngine>,
        skills: Arc<SkillRegistry>,
        event_bus: Arc<EventBus>,
        max_iterations: u32,
        max_pdca_cycles: u32,
    ) -> Self {
        // Wire up event bus on runner so it can emit detailed execution events
        // (TOOL_CALL, TOOL_RESULT, THOUGHT) during the ReAct loop.
        if let Some(r) = Arc::get_mut(&mut runner) {
            r.set_event_bus(event_bus.clone());
        }

        let event_bus_for_perception = event_bus.clone();
        // 创建补充输入共享存储，同时注入 AgentRunner（确保 SA 和 Runner 共享同一实例）
        let supplement_store = SupplementaryInputStore::new();
        if let Some(r) = Arc::get_mut(&mut runner) {
            r.supplement_store = supplement_store.clone();
        }
        Self {
            runner: runner.clone(),
            template_engine,
            skills,
            event_receiver: Some(event_bus.subscribe()),
            event_bus,
            active_cycles: HashMap::new(),
            max_iterations,
            max_pdca_cycles,
            perception: ProactiveEngine::new(runner.l0_store.clone(), event_bus_for_perception),
            sharing: Arc::new(SharingProtocol::new()),
            blackboard: None,
            prefetch_engine: None,
            scheduler: None,
            type_router: TypeRouter::new(),
            pending_approvals: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            supplementary_inputs: HashMap::new(),
            supplement_store,
            embedder: None,
            relevance_tracker: RelevanceTracker::new(0.6),
        }
    }

    pub fn with_memory(
        mut self,
        blackboard: Option<Arc<Blackboard>>,
        prefetch_engine: Option<Arc<PrefetchEngine>>,
        scheduler: Option<Arc<MemoryScheduler>>,
    ) -> Self {
        self.blackboard = blackboard;
        self.prefetch_engine = prefetch_engine;
        self.scheduler = scheduler;
        self
    }

    /// 设置嵌入服务（用于计算补充输入的 embedding 和 relevance_score）
    /// 同时传播到 AgentRunner，使其在 ReAct 循环中也能计算 turn embedding
    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingService>) -> Self {
        self.embedder = Some(embedder.clone());
        // 传播到 AgentRunner
        if let Some(runner) = Arc::get_mut(&mut self.runner) {
            *runner = runner.clone().with_embedder(embedder);
        }
        self
    }

    /// 获取补充输入共享存储（AgentRunner 注入用）
    pub fn supplement_store(&self) -> SupplementaryInputStore {
        self.supplement_store.clone()
    }

    /// 更新 gateway 中的默认模型（使用 RwLock 内部可变性，无需 &mut self）
    /// 这避免了重建整个 Engine/L0Store 导致的 redb 文件锁冲突
    pub fn set_model(&self, model: &str) {
        self.runner.gateway.set_default_model(model.to_string());
        for task_type in &["planning", "execution", "analysis", "default"] {
            self.runner.gateway.set_model_mapping(task_type.to_string(), model.to_string());
        }
    }

    pub fn set_api_key(&self, key: &str) {
        self.runner.gateway.set_api_key(key.to_string());
    }

    pub fn set_base_url(&self, url: &str) {
        self.runner.gateway.set_base_url(url.to_string());
    }

    pub fn blackboard(&self) -> Option<&Arc<Blackboard>> {
        self.blackboard.as_ref()
    }

    pub async fn start_cycle(
        &mut self,
        user_input: &str,
        task_iri: &str,
    ) -> Result<String, CoreError> {
        let cycle_id = format!("cycle_{}", uuid::Uuid::new_v4().hyphenated());

        let perception_result = self.perception.on_task_start(user_input, task_iri)?;
        info!(
            cycle_id = %cycle_id,
            task_iri = %task_iri,
            complexity = %perception_result.complexity,
            risks = ?perception_result.risks,
            "感知分析完成"
        );

        let cycle = CycleState {
            cycle_id: cycle_id.clone(),
            task_iri: task_iri.to_string(),
            phase: CyclePhase::Analyzing,
            iteration: 0,
            max_iterations: self.max_iterations,
            started_at: chrono::Utc::now(),
            phase_history: vec!["Created".to_string()],
            task_completed: false,
            experience_hints: perception_result.relevant_experience_hints.clone(),
        };

        self.active_cycles.insert(cycle_id.clone(), cycle);

        info!(cycle_id = %cycle_id, task_iri = %task_iri, input = %user_input, "Cycle started");

        self.event_bus
            .emit(task_iri, "CYCLE_STARTED", "SA", &serde_json::json!({
                "cycle_id": &cycle_id,
                "user_input": user_input,
            }).to_string())
            .await;

        Ok(cycle_id)
    }

    pub fn analyze_task(&self, user_input: &str) -> ExecutionPlan {
        let complexity = self.classify_complexity(user_input);

        let (agent_sequence, parallel_groups, description) = match &complexity {
            TaskComplexity::Instant => (
                vec![AgentRole::Do],
                vec![],
                "Instant query: single DA agent".to_string(),
            ),
            TaskComplexity::Simple => (
                vec![AgentRole::Do],
                vec![],
                "Simple query: single DA agent".to_string(),
            ),
            TaskComplexity::Standard => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Standard task: PA → DA → CA → AA".to_string(),
            ),
            TaskComplexity::Complex => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Complex task: PA → DA → CA → AA with full validation".to_string(),
            ),
            TaskComplexity::Exploratory => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Do, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![vec![AgentRole::Do, AgentRole::Do, AgentRole::Do]],
                "Exploratory: PA → [DA1, DA2, DA3] → CA → AA".to_string(),
            ),
            TaskComplexity::Emergency => (
                vec![AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Emergency: DA → CA → AA (skip PA)".to_string(),
            ),
            TaskComplexity::Recursive => {
                // Recursive: 只执行 1 轮 SA 级 PDCA。
                // DA 内部通过 execute_recursive_sub_cycle 进行微观递归
                // 分解子任务，无需 SA 级多轮重放。
                let seq = vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act];
                (seq, vec![], "Recursive: 1 PDCA with DA-internal sub-cycles".to_string())
            },
        };

        let steps = self.generate_default_steps(&agent_sequence);

        let max_recursion_depth = match &complexity {
            TaskComplexity::Recursive => self.max_pdca_cycles,
            TaskComplexity::Complex => 2,
            _ => 0,
        };

        ExecutionPlan {
            plan_id: format!("plan_{}", uuid::Uuid::new_v4().hyphenated()),
            agent_sequence,
            parallel_groups,
            task_complexity: complexity,
            description,
            steps,
            context_requirements: HashMap::new(),
            success_metrics: vec!["任务完成".to_string()],
            max_recursion_depth,
            sub_tasks: vec![],
            dag_jsonld: None,
        }
    }

    fn build_plan_from_complexity(&self, complexity: TaskComplexity) -> ExecutionPlan {
        let (agent_sequence, parallel_groups, description) = match &complexity {
            TaskComplexity::Instant => (
                vec![AgentRole::Do],
                vec![],
                "Instant query: single DA agent".to_string(),
            ),
            TaskComplexity::Simple => (
                vec![AgentRole::Do],
                vec![],
                "Simple query: single DA agent".to_string(),
            ),
            TaskComplexity::Standard => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Standard task: PA → DA → CA → AA".to_string(),
            ),
            TaskComplexity::Complex => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Complex task: PA → DA → CA → AA with full validation".to_string(),
            ),
            TaskComplexity::Exploratory => (
                vec![AgentRole::Plan, AgentRole::Do, AgentRole::Do, AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![vec![AgentRole::Do, AgentRole::Do, AgentRole::Do]],
                "Exploratory: PA → [DA1, DA2, DA3] → CA → AA".to_string(),
            ),
            TaskComplexity::Emergency => (
                vec![AgentRole::Do, AgentRole::Check, AgentRole::Act],
                vec![],
                "Emergency: DA → CA → AA (skip PA)".to_string(),
            ),
            TaskComplexity::Recursive => {
                // Recursive: 只执行 1 轮 SA 级 PDCA（同 Standard）。
                // DA 内部通过 execute_recursive_sub_cycle 进行微观递归
                // 分解子任务，无需 SA 级多轮重放。
                let seq = vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act];
                (seq, vec![], "Recursive: 1 PDCA with DA-internal sub-cycles".to_string())
            },
        };

        let steps = self.generate_default_steps(&agent_sequence);

        let max_recursion_depth = match &complexity {
            TaskComplexity::Recursive => self.max_pdca_cycles,
            TaskComplexity::Complex => 2,
            _ => 0,
        };

        ExecutionPlan {
            plan_id: format!("plan_{}", uuid::Uuid::new_v4().hyphenated()),
            agent_sequence,
            parallel_groups,
            task_complexity: complexity,
            description,
            steps,
            context_requirements: HashMap::new(),
            success_metrics: vec!["任务完成".to_string()],
            max_recursion_depth,
            sub_tasks: vec![],
            dag_jsonld: None,
        }
    }

    /// 构建 resume 模式的执行计划：标准 PDCA 序列
    /// execute_plan 会根据 resumed_messages 跳过已完成的阶段
    fn build_resume_plan(&self) -> ExecutionPlan {
        let agent_sequence = vec![AgentRole::Plan, AgentRole::Do, AgentRole::Check, AgentRole::Act];
        let steps = self.generate_default_steps(&agent_sequence);
        ExecutionPlan {
            plan_id: format!("plan_resume_{}", uuid::Uuid::new_v4().hyphenated()),
            agent_sequence,
            parallel_groups: vec![],
            task_complexity: TaskComplexity::Standard,
            description: "Resume: continue from checkpoint".to_string(),
            steps,
            context_requirements: HashMap::new(),
            success_metrics: vec!["任务完成".to_string()],
            max_recursion_depth: 0,
            sub_tasks: vec![],
            dag_jsonld: None,
        }
    }

    fn generate_default_steps(&self, agent_sequence: &[AgentRole]) -> Vec<PlanStep> {
        agent_sequence
            .iter()
            .enumerate()
            .map(|(i, role)| {
                let (objective, expected_output, success_criteria) = match role {
                    AgentRole::Plan => (
                        "分析任务需求，制定详细执行计划".to_string(),
                        "JSON格式的计划，包含步骤、依赖关系、资源需求".to_string(),
                        "计划清晰、步骤完整、依赖关系明确".to_string(),
                    ),
                    AgentRole::Do => (
                        "按照计划执行具体任务".to_string(),
                        "执行结果、生成的文件或数据".to_string(),
                        "任务按计划完成，输出符合预期".to_string(),
                    ),
                    AgentRole::Check => (
                        "验证执行结果的质量和正确性".to_string(),
                        "检查报告，包含问题列表和建议".to_string(),
                        "验证通过或问题已识别".to_string(),
                    ),
                    AgentRole::Act => (
                        "汇总结果，做出最终决策".to_string(),
                        "最终决策和总结报告".to_string(),
                        "决策明确，总结完整".to_string(),
                    ),
                };

                PlanStep {
                    step_id: format!("step_{}", i + 1),
                    role: *role,
                    objective,
                    expected_output,
                    dependencies: if i > 0 { vec![format!("step_{}", i)] } else { vec![] },
                    tools_allowed: vec![],
                    success_criteria,
                }
            })
            .collect()
    }

    async fn extract_5w2h_from_input(&self, user_input: &str) -> crate::core::five_w2h::Task5W2H {
        use crate::core::five_w2h::*;

        if user_input.len() < 20 && !user_input.contains(' ') {
            let mut w2h = Task5W2H::new(user_input, "用户任务");
            w2h.why.priority = Priority::Low;
            return w2h;
        }

        let prompt = format!(
            r#"分析以下用户任务，提取 5W2H 元数据的最小集（What + Why）。

用户任务: {}

请以 JSON 格式输出：
{{
  "what": "任务目标的核心描述（一句话）",
  "why_description": "任务意图/价值描述",
  "success_criteria": ["可验证条件1", "条件2"],
  "priority": "high|medium|low",
  "deadline": "ISO8601格式截止时间（可选）",
  "required_role": "Plan|Do|Check|Act（可选）"
}}

只输出 JSON，不要其他内容。"#,
            user_input
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![crate::gateway::unified_gateway::ChatMessage {
            role: "user".to_string(),
            content: prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];

        match self.runner.gateway.chat_with_params(&model, messages, Some(0.3), Some(500), None, None).await {
            Ok(response) => {
                if let Some(content) = response.choices.first().and_then(|c| c.message.content.clone()) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                        let what = parsed.get("what").and_then(|v| v.as_str()).unwrap_or(user_input).to_string();
                        let why_desc = parsed.get("why_description").and_then(|v| v.as_str()).unwrap_or("用户任务").to_string();
                        let success_criteria = parsed.get("success_criteria")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let priority = match parsed.get("priority").and_then(|v| v.as_str()).unwrap_or("medium") {
                            "high" => Priority::High,
                            "low" => Priority::Low,
                            _ => Priority::Medium,
                        };

                        let mut w2h = Task5W2H::new(&what, &why_desc);
                        w2h.why.success_criteria = success_criteria;
                        w2h.why.priority = priority;

                        if let Some(deadline_str) = parsed.get("deadline").and_then(|v| v.as_str()) {
                            if let Ok(dt) = deadline_str.parse::<chrono::DateTime<chrono::Utc>>() {
                                w2h = w2h.with_when(WhenDetail {
                                    deadline: Some(dt),
                                    start_after: None,
                                    estimated_duration: None,
                                    timezone: None,
                                    reminder_before: None,
                                });
                            }
                        }

                        if let Some(role_str) = parsed.get("required_role").and_then(|v| v.as_str()) {
                            w2h = w2h.with_who(WhoDetail {
                                requestor: None,
                                assignees: vec![],
                                stakeholders: vec![],
                                required_role: Some(role_str.to_string()),
                                access_level: None,
                            });
                        }

                        return w2h;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("5W2H 提取失败: {}, 使用默认值", e);
            }
        }

        Task5W2H::new(user_input, "用户任务")
    }

    pub async fn analyze_task_with_llm(&self, user_input: &str, five_w2h: &crate::core::five_w2h::Task5W2H, experience_hints: &[String]) -> ExecutionPlan {
        // 先通过关键词分类器检测递归/复杂任务（关键词路径比 LLM 更可靠地捕获 Recursive）
        let keyword_complexity = self.classify_complexity(user_input);
        if keyword_complexity == TaskComplexity::Recursive {
            info!("关键词分类为 Recursive，跳过 LLM 计划生成，直接使用关键词路径构建循环计划");
            return self.build_plan_from_complexity(TaskComplexity::Recursive);
        }

        let enhanced_input = if experience_hints.is_empty() {
            user_input.to_string()
        } else {
            format!("## 历史经验参考\n{}\n\n## 当前任务\n{}",
                experience_hints.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n"),
                user_input
            )
        };

        let complexity = match five_w2h.why.priority {
            crate::core::five_w2h::Priority::High => {
                // 关键词分类为 Complex 则直接使用，否则 High→Complex
                if keyword_complexity == TaskComplexity::Complex {
                    TaskComplexity::Complex
                } else {
                    TaskComplexity::Complex
                }
            },
            crate::core::five_w2h::Priority::Medium => TaskComplexity::Standard,
            crate::core::five_w2h::Priority::Low => TaskComplexity::Simple,
        };

        match self.generate_detailed_plan_with_llm(&enhanced_input, five_w2h).await {
            Ok(mut plan) => {
                info!(plan_id = %plan.plan_id, steps = plan.steps.len(), "LLM 生成详细计划成功");
                // 如果关键词分类为 Complex，但 LLM 返回的复杂度不对，纠正它
                if keyword_complexity == TaskComplexity::Complex && plan.task_complexity != TaskComplexity::Complex {
                    plan.task_complexity = TaskComplexity::Complex;
                    plan.max_recursion_depth = 2;
                }
                return plan;
            }
            Err(e) => {
                warn!("LLM 生成详细计划失败: {}, 使用默认计划", e);
            }
        }

        self.build_plan_from_complexity(complexity)
    }

    async fn generate_detailed_plan_with_llm(&self, user_input: &str, five_w2h: &crate::core::five_w2h::Task5W2H) -> Result<ExecutionPlan, CoreError> {
        let mut w2h_section = String::new();
        if let Some(ref when) = five_w2h.when {
            if let Some(ref deadline) = when.deadline {
                w2h_section.push_str(&format!("\n- 截止时间: {}", deadline.to_rfc3339()));
            }
        }
        if let Some(ref how_much) = five_w2h.how_much {
            if let Some(budget) = how_much.token_budget {
                w2h_section.push_str(&format!("\n- Token 预算: {}", budget));
            }
            if let Some(cycles) = how_much.max_pdca_cycles {
                w2h_section.push_str(&format!("\n- 最大 PDCA 循环数: {}", cycles));
            }
        }
        if !five_w2h.why.success_criteria.is_empty() {
            w2h_section.push_str(&format!("\n- 成功标准: {}", five_w2h.why.success_criteria.join(", ")));
        }

        let w2h_block = if w2h_section.is_empty() {
            String::new()
        } else {
            format!("\n## 5W2H 约束信息{}", w2h_section)
        };

        let sa_constitution_prompt = {
            use crate::core::constitution::{ConstitutionRegistry, ConstitutionRole};
            let registry = ConstitutionRegistry::new();
            let constitution_text = registry.build_prompt_for_role_exact(ConstitutionRole::Supervisor);
            // 注入方法论层纪律（包含自动触发协议、始终激活方法论）
            let methodology_text = crate::methodology::integration::MethodologyPromptInjector::build_for_sa();
            format!("{}\n{}", constitution_text, methodology_text)
        };

        let prompt = format!(
            r#"你是一个任务规划专家。请分析以下任务并生成精简高效的执行计划。

## 任务描述
{}{}
## 输出要求
请以 JSON 格式输出计划，包含以下字段：

```json
{{
  "complexity": "simple|standard|complex|exploratory|emergency",
  "description": "任务描述",
  "steps": [
    {{
      "step_id": "step_1",
      "role": "Plan|Do|Check|Act",
      "objective": "该步骤的具体目标",
      "expected_output": "预期输出",
      "dependencies": [],
      "tools_allowed": ["file_read", "file_write", "grep_search", "glob_search", "web_search", "web_fetch", "bash"],
      "success_criteria": "成功标准"
    }}
  ],
  "success_metrics": ["成功指标1", "成功指标2"]
}}
```

## 角色说明
- **Plan (PA)**: 分析任务、制定计划、分解子任务
- **Do (DA)**: 执行具体任务、创建产物（一个DA步骤应完成多个相关操作）
- **Check (CA)**: 验证结果、检查质量
- **Act (AA)**: 汇总决策、最终总结

## 复杂度定义
- **simple**: 简单查询，单步可完成（仅 DA）
- **standard**: 标准任务，需要 PA→DA→CA→AA 流程
- **complex**: 复杂任务，需要 PA→DA→CA→AA 完整验证，DA 完成后内部会触发子循环优化
- **exploratory**: 探索性任务，需要多个并行 DA
- **emergency**: 紧急修复，跳过 PA，DA→CA→AA

## 重要约束
1. **步骤数量限制**: 总步骤数不超过 6 个（含 PA 和 CA/AA）
2. **DA 步骤合并**: 将多个相关操作合并到一个 DA 步骤中。例如创建多个文件应在一个 DA 步骤中完成，而非每个文件一个步骤
3. **推荐模式**: PA(1步) → DA(1-3步) → CA(1步) → AA(1步)
4. 每个 DA 步骤的 objective 应描述要完成的一组相关操作，而非单个原子操作

## 行为准则
作为 Supervisor Agent，你必须遵守以下准则：

{}

请直接输出 JSON，不要有其他内容。"#,
            user_input, w2h_block, sa_constitution_prompt
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![crate::gateway::unified_gateway::ChatMessage {
            role: "user".to_string(),
            content: prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];

        let response = self.runner.gateway.chat_with_params(
            &model,
            messages,
            Some(0.3),
            Some(2000),
            None,
            None,
        ).await?;

        let content = response.choices.first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| CoreError::Internal { message: "No response content".to_string() })?;

        self.parse_llm_plan(&content)
    }

    fn parse_llm_plan(&self, content: &str) -> Result<ExecutionPlan, CoreError> {
        let trimmed = content.trim();
        let json_str = if trimmed.starts_with('{') {
            trimmed.to_string()
        } else if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                trimmed[start..=end].to_string()
            } else {
                trimmed.to_string()
            }
        } else {
            return Err(CoreError::Internal { message: "No JSON found in LLM plan response".to_string() });
        };

        #[derive(Deserialize)]
        struct LlmPlanResponse {
            complexity: String,
            description: String,
            steps: Vec<LlmPlanStep>,
            success_metrics: Vec<String>,
        }

        #[derive(Deserialize)]
        struct LlmPlanStep {
            step_id: String,
            role: String,
            objective: String,
            expected_output: String,
            dependencies: Vec<String>,
            tools_allowed: Vec<String>,
            success_criteria: String,
        }

        let parsed: LlmPlanResponse = parse_or_repair_json(&json_str)
            .map_err(|e| CoreError::Internal { message: format!("JSON parse error after repair attempt: {}", e) })?;

        let complexity = match parsed.complexity.as_str() {
            "simple" => TaskComplexity::Simple,
            "complex" => TaskComplexity::Complex,
            "recursive" => TaskComplexity::Recursive,
            "exploratory" => TaskComplexity::Exploratory,
            "emergency" => TaskComplexity::Emergency,
            _ => TaskComplexity::Standard,
        };

        let steps: Vec<PlanStep> = parsed.steps.into_iter().map(|s| {
            let role = match s.role.as_str() {
                "Plan" => AgentRole::Plan,
                "Do" => AgentRole::Do,
                "Check" => AgentRole::Check,
                "Act" => AgentRole::Act,
                _ => AgentRole::Do,
            };
            PlanStep {
                step_id: s.step_id,
                role,
                objective: s.objective,
                expected_output: s.expected_output,
                dependencies: s.dependencies,
                tools_allowed: s.tools_allowed,
                success_criteria: s.success_criteria,
            }
        }).collect();

        let max_plan_steps = 8;
        let steps = if steps.len() > max_plan_steps {
            warn!("计划步骤数 {} 超过限制 {}, 截断保留前 {} 步", steps.len(), max_plan_steps, max_plan_steps);
            steps.into_iter().take(max_plan_steps).collect()
        } else {
            steps
        };

        let agent_sequence: Vec<AgentRole> = steps.iter().map(|s| s.role).collect();

        let max_recursion_depth = match &complexity {
            TaskComplexity::Recursive => 3,
            TaskComplexity::Complex => 2,
            _ => 0,
        };

        Ok(ExecutionPlan {
            plan_id: format!("plan_{}", uuid::Uuid::new_v4().hyphenated()),
            agent_sequence,
            parallel_groups: vec![],
            task_complexity: complexity,
            description: parsed.description,
            steps,
            context_requirements: HashMap::new(),
            success_metrics: parsed.success_metrics,
            max_recursion_depth,
            sub_tasks: vec![],
            dag_jsonld: None,
        })
    }

    async fn classify_with_llm(&self, user_input: &str) -> Result<TaskComplexity, CoreError> {
        let prompt = format!(
            r#"分析以下任务的复杂度，返回 JSON 格式结果。

任务: {}

请分析任务的：
1. 是否需要多步骤执行？
2. 是否需要规划阶段？
3. 是否需要验证阶段？
4. 是否需要多个并行探索？

返回 JSON:
{{"complexity": "simple|standard|complex|exploratory|emergency", "reason": "简短原因"}}

复杂度定义：
- simple: 简单查询，单步可完成
- standard: 标准任务，需要计划→执行→检查→决策流程
- complex: 复杂任务，需要多步执行和验证
- exploratory: 探索性任务，需要多个并行探索
- emergency: 紧急修复任务，跳过计划直接执行"#,
            user_input
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![crate::gateway::unified_gateway::ChatMessage {
            role: "user".to_string(),
            content: prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];

        let response = self.runner.gateway.chat_with_params(
            &model,
            messages,
            Some(0.3),
            Some(200),
            None,
            None,
        ).await?;

        let content = response.choices.first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        // 解析 LLM 响应
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(complexity_str) = parsed.get("complexity").and_then(|c| c.as_str()) {
                let complexity = match complexity_str {
                    "simple" => TaskComplexity::Simple,
                    "complex" => TaskComplexity::Complex,
                    "exploratory" => TaskComplexity::Exploratory,
                    "emergency" => TaskComplexity::Emergency,
                    _ => TaskComplexity::Standard,
                };
                info!(complexity = ?complexity, reason = ?parsed.get("reason"), "LLM 分类结果");
                return Ok(complexity);
            }
        }

        // 尝试从文本中提取
        let lower = content.to_lowercase();
        if lower.contains("simple") {
            return Ok(TaskComplexity::Simple);
        } else if lower.contains("exploratory") {
            return Ok(TaskComplexity::Exploratory);
        } else if lower.contains("emergency") {
            return Ok(TaskComplexity::Emergency);
        }

        Err(CoreError::Internal { message: "Failed to parse LLM classification".to_string() })
    }

    fn classify_complexity(&self, user_input: &str) -> TaskComplexity {
        let lower = user_input.to_lowercase();

        // Instant: 非常简短的输入（如问候语）
        if user_input.len() < 15 && !user_input.contains(' ') {
            return TaskComplexity::Instant;
        }

        // Emergency: 紧急修复类
        let emergency_keywords = ["fix", "bug", "error", "crash", "urgent", "broken", "repair",
            "修复", "紧急", "崩溃", "故障"];
        if emergency_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Emergency;
        }

        // 递归分解：复杂多步骤任务，需要 DA 内部微观 PDCA 子循环
        let recursive_keywords = [
            "重构", "refactor", "重写", "rewrite", "迁移", "migrate",
            "拆分", "split into", "分解", "decompose",
            "逐步实现", "分步实现", "多阶段", "multi-phase",
            "端到端", "end-to-end", "全栈", "full-stack",
            "从头搭建", "从零搭建", "搭建完整",
            // 项目构建类
            "写一个", "写一", "开发", "develop", "创建", "create",
            "实现", "implement", "构建", "building", "build",
            "程序", "program", "项目", "project", "应用", "app",
            "网站", "website", "系统", "system", "平台", "platform",
            "生成", "generate", "制作",
        ];
        if recursive_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Recursive;
        }

        // 探索性任务 → Exploratory（优先于 research_patterns）
        let exploratory_keywords = [
            "research", "explore", "investigate",
            "多个方案", "多种方法", "探索", "对比分析",
        ];
        if exploratory_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Exploratory;
        }

        let compare_keywords = [
            "compare", "对比", "比较",
        ];
        if compare_keywords.iter().any(|k| lower.contains(k)) {
            let multi_patterns = ["different", "various", "multiple", "several", "多个", "各种"];
            if multi_patterns.iter().any(|p| lower.contains(p)) {
                return TaskComplexity::Exploratory;
            }
            return TaskComplexity::Complex;
        }

        // 调研分析类问题 → Standard 或 Complex
        let research_patterns = [
            "有哪些应用", "有哪些场景", "有哪些案例", "有哪些方法",
            "应用场景", "应用案例", "应用方向",
            "如何实现", "如何设计", "如何解决",
            "分析", "研究", "调研", "对比", "比较", "评估",
            "介绍", "概述", "综述", "总结",
            "优缺点", "利弊", "最佳实践",
            "发展趋势", "前景", "现状",
        ];
        if research_patterns.iter().any(|p| lower.contains(p)) {
            let deep_patterns = ["深入", "详细", "全面", "系统", "综合", "多角度"];
            if deep_patterns.iter().any(|p| lower.contains(p)) {
                return TaskComplexity::Complex;
            }
            return TaskComplexity::Standard;
        }

        // Simple: 简单事实查询，一句话能回答
        let simple_query_patterns = [
            "什么是", "是什么", "是谁", "在哪里", "什么时候",
            "定义", "含义", "意思",
            "吗？", "吗?", "能否", "可以",
        ];
        let is_simple_query = user_input.len() < 50 
            && simple_query_patterns.iter().any(|p| lower.contains(p))
            && !lower.contains("应用") 
            && !lower.contains("场景")
            && !lower.contains("分析")
            && !lower.contains("实现")
            && !lower.contains("设计");
        
        if is_simple_query {
            return TaskComplexity::Simple;
        }

        // 英文简单查询
        if user_input.len() < 50
            && (lower.starts_with("what is") 
                || lower.starts_with("who is")
                || lower.starts_with("where is")
                || lower.starts_with("when is"))
        {
            return TaskComplexity::Simple;
        }

        // 默认：Standard
        TaskComplexity::Standard
    }

    fn create_agent(&self, role: AgentRole, cycle_id: &str) -> AgentInstance {
        let agent_id = format!("{}_{}_{}", cycle_id, role, uuid::Uuid::new_v4().hyphenated());
        AgentInstance::new(agent_id, role)
    }

    async fn dispatch_agent(
        &self,
        role: AgentRole,
        context: TaskContext,
        cycle_id: &str,
        plan_step: Option<PlanStep>,
    ) -> Result<TaskResult, CoreError> {
        let agent = self.create_agent(role, cycle_id);

        // 从 L2 黑板查询上下文（替代 prev_summary）
        // 改进：优先使用节点 content（完整输出），回退到 summary（短摘要）
        let prev_agent_summary = context.prev_agent_summary.clone();
        let prev_summary = if let Some(blackboard) = &self.blackboard {
            let nodes = blackboard.query_nodes(&context.task_iri).unwrap_or_default();
            if !nodes.is_empty() {
                let mut contents: Vec<String> = Vec::new();
                let mut summaries: Vec<String> = Vec::new();
                for n in nodes.iter() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&n.json_ld) {
                        let role = parsed.get("role").and_then(|v| v.as_str()).unwrap_or("");
                        let prefix = if !role.is_empty() { format!("[{}] ", role) } else { String::new() };
                        // 优先 content 字段（完整 LLM 输出）
                        if let Some(content) = parsed.get("content").and_then(|s| s.as_str()) {
                            let trimmed = content.trim();
                            if !trimmed.is_empty() && trimmed.len() > 20 {
                                contents.push(format!("{}{}", prefix, trimmed));
                                continue;
                            }
                        }
                        // 回退到 summary 字段
                        if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                            let trimmed = summary.trim();
                            if !trimmed.is_empty() {
                                summaries.push(format!("{}{}", prefix, trimmed));
                            }
                        }
                    }
                }
                // 优先使用有实质内容的 content
                if !contents.is_empty() {
                    Some(contents.join("\n\n---\n\n"))
                } else if !summaries.is_empty() {
                    Some(summaries.join("\n"))
                } else {
                    prev_agent_summary.clone()
                }
            } else {
                prev_agent_summary.clone()
            }
        } else {
            prev_agent_summary.clone()
        };

        let context = if let Some(ref summary) = prev_summary {
            context.with_prev_summary(summary)
        } else {
            context
        };

        info!(agent_id = %agent.agent_id, role = ?role, task = %context.task_iri, "Dispatching agent with isolation");

        self.event_bus
            .emit(&context.task_iri, &format!("{:?}_STARTED", role), &agent.agent_id,
                &serde_json::json!({"cycle_id": cycle_id}).to_string())
            .await;

        // 使用独立的 BizAgent 实例执行（Agent 隔离）
        let result = self.runner.execute_with_biz_agent(&agent, context, plan_step).await?;

        match result.status.as_str() {
            "success" => {
                let task_result = serde_json::json!({"status": "success", "summary": &result.summary});
                self.perception.on_task_end(&task_result, &result.task_iri);
            }
            "failed" => {
                let task_result = serde_json::json!({"status": "failed", "summary": &result.summary});
                self.perception.on_task_end(&task_result, &result.task_iri);
            }
            _ => {}
        }

        self.event_bus
            .emit(&result.task_iri, &format!("{:?}_COMPLETED", role), &agent.agent_id,
                &serde_json::json!({"status": &result.status, "summary": &result.summary}).to_string())
            .await;

        Ok(result)
    }

    async fn dispatch_agents_parallel(
        &self,
        role: AgentRole,
        count: usize,
        base_objective: &str,
        task_iri: &str,
        cycle_id: &str,
        max_iterations: u32,
    ) -> Result<Vec<TaskResult>, CoreError> {
        let _ = self.event_bus.emit(
            task_iri,
            "PARALLEL_START",
            "system:sa",
            &serde_json::json!({
                "role": format!("{:?}", role),
                "count": count,
                "cycle_id": cycle_id,
            }).to_string(),
        ).await;

        let runner = self.runner.clone();
        let mut handles = Vec::new();

        for i in 0..count {
            let objective = format!("[{}-{}] {}", role, i + 1, base_objective);
            let ctx = TaskContext::new(task_iri, &objective, max_iterations);
            let tid = cycle_id.to_string();
            let runner_clone = runner.clone();

            handles.push(tokio::spawn(async move {
                let agent_id = format!("{}_{}_{}", tid, role, uuid::Uuid::new_v4().hyphenated());
                let mut agent = AgentInstance::new(agent_id, role);
                runner_clone.execute(&mut agent, ctx).await
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(res)) => results.push(res),
                Ok(Err(e)) => warn!("Parallel agent failed: {}", e),
                Err(e) => warn!("Parallel agent panicked: {}", e),
            }
        }

        let _ = self.event_bus.emit(
            task_iri,
            "PARALLEL_COMPLETE",
            "system:sa",
            &serde_json::json!({
                "role": format!("{:?}", role),
                "success_count": results.len(),
                "total_count": count,
            }).to_string(),
        ).await;

        info!(count = results.len(), "Parallel agents completed");
        Ok(results)
    }

    pub async fn execute_plan(
        &mut self,
        plan: ExecutionPlan,
        task_iri: &str,
        user_input: &str,
        mut five_w2h: crate::core::five_w2h::Task5W2H,
        five_w2h_iri: &str,
        resumed_messages: Option<Vec<crate::gateway::unified_gateway::ChatMessage>>,
    ) -> Result<TaskResult, CoreError> {
        use crate::core::five_w2h::FillStage;
        
        let cycle_id = self
            .active_cycles
            .iter()
            .find(|(_, c)| c.task_iri == task_iri)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| format!("cycle_{}", uuid::Uuid::new_v4().hyphenated()));
        
        let task_id = task_iri.strip_prefix("iri://task/")
            .unwrap_or_else(|| task_iri.strip_prefix("iri://").unwrap_or(task_iri));

        if let Some(cycle) = self.active_cycles.get_mut(&cycle_id) {
            cycle.phase = CyclePhase::Dispatching;
            cycle.phase_history.push("Dispatching".to_string());
        }

        info!(plan_id = %plan.plan_id, steps = plan.steps.len(), "Executing plan with detailed steps");

        if let Some(prefetch) = &self.prefetch_engine {
            let entities: Vec<String> = plan.steps.iter()
                .filter_map(|s| {
                    if s.expected_output.starts_with("iri://") {
                        Some(s.expected_output.clone())
                    } else {
                        None
                    }
                })
                .collect();
            prefetch.on_intent_change(&plan.description, &entities).await;
        }

        let mut last_result: Option<TaskResult> = None;
        let mut prev_summary: Option<String> = None;

        // Resume 模式：确定从哪个阶段开始
        // 从 L0 加载最新的 checkpoint 来解析 phase 标签
        let resume_skip_phases: Vec<AgentRole> = if resumed_messages.is_some() {
            let cm = crate::core::checkpoint::CheckpointManager::with_persistence(self.runner.l0_store.clone());
            let skip_roles = cm.restore_latest_with_skip_roles(task_iri)
                .ok()
                .flatten()
                .map(|(_, roles)| roles)
                .unwrap_or_else(|| vec!["Plan".to_string()]);
            skip_roles.iter().filter_map(|r| {
                match r.as_str() {
                    "Plan" => Some(AgentRole::Plan),
                    "Do" => Some(AgentRole::Do),
                    "Check" => Some(AgentRole::Check),
                    "Act" => Some(AgentRole::Act),
                    _ => None,
                }
            }).collect()
        } else {
            vec![]
        };
        info!("[resume] skip phases: {:?}", resume_skip_phases);

        // Resume 模式：优先从 checkpoint 的 prev_summary 字段恢复
        // 如果 checkpoint 中没有 prev_summary，则从历史消息中提取 PA 输出
        let resume_prev_summary: Option<String> = if resumed_messages.is_some() {
            // 尝试从 L0 的 checkpoint 中读取保存的 prev_summary
            let cm = crate::core::checkpoint::CheckpointManager::with_persistence(self.runner.l0_store.clone());
            let from_cp: Option<String> = cm.restore_latest(task_iri).ok().flatten()
                .and_then(|cp| cp.prev_summary);
            if from_cp.is_some() {
                from_cp
            } else {
                // fallback：从历史消息中提取 PA 阶段的输出作为 prev_summary
                resumed_messages.as_ref().and_then(|msgs| {
                    let mut found_first_user = false;
                    for msg in msgs.iter() {
                        if msg.role == "user" && !found_first_user {
                            found_first_user = true;
                            continue;
                        }
                        if msg.role == "assistant" && found_first_user {
                            return Some(msg.content.clone());
                        }
                    }
                    msgs.iter().rev()
                        .find(|m| m.role == "assistant")
                        .map(|m| m.content.clone())
                })
            }
        } else {
            None
        };

        let task_level = match plan.task_complexity {
            TaskComplexity::Instant => "Instant",
            TaskComplexity::Simple => "Simple",
            TaskComplexity::Standard => "Standard",
            TaskComplexity::Complex => "Complex",
            TaskComplexity::Exploratory => "Complex",
            TaskComplexity::Emergency => "Standard",
            TaskComplexity::Recursive => "Recursive",
        };

        // --- 统一 DAG 执行路径 ---
        // 将 ExecutionPlan 转换为 DAG（LLM 路径适配）或直接使用外部 JSON-LD DAG（--workflow 路径）
        let dag = if let Some(ref dag_jsonld) = plan.dag_jsonld {
            let def = crate::core::workflow::loader::load_workflow_jsonld(dag_jsonld)
                .map_err(|e| CoreError::Internal { message: format!("工作流解析失败: {}", e) })?;
            crate::core::workflow::loader::build_dag(&def)
                .map_err(|e| CoreError::Internal { message: format!("DAG 构建失败: {}", e) })?
        } else {
            let wf = crate::core::workflow::adapter::plan_to_workflow(&plan, task_iri);
            crate::core::workflow::loader::build_dag(&wf)
                .map_err(|e| CoreError::Internal { message: format!("DAG 构建失败: {}", e) })?
        };
        let order = crate::core::workflow::loader::topological_order(&dag)
            .map_err(|e| CoreError::Internal { message: format!("拓扑排序失败: {}", e) })?;

        let mut completed_node_results: std::collections::HashMap<String, crate::core::workflow::NodeResult> = std::collections::HashMap::new();
        let mut skip_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 按 DAG 拓扑序执行
        for (i, &node_idx) in order.iter().enumerate() {
            let node_def = &dag.graph[node_idx].def;
            let step = crate::core::workflow::adapter::node_to_planstep(node_def);

            // 检查是否在跨节点跳过的集合中（来自 HumanApprovalNode 的分支跳转）
            if skip_nodes.contains(&node_def.id) {
                info!(node_id = %node_def.id, "HumanApprovalNode 分支跳转: 跳过此节点");
                continue;
            }

            // Resume 模式：跳过已完成的阶段
            if resume_skip_phases.contains(&step.role) {
                info!(role = ?step.role, "[resume] 跳过已完成的阶段");
                if prev_summary.is_none() {
                    prev_summary = resume_prev_summary.clone().or_else(|| Some("从 checkpoint 恢复，前序阶段已完成。".to_string()));
                }
                continue;
            }

            // HumanApprovalNode：人工审批节点，不派遣 Agent
            if node_def.node_type == "HumanApprovalNode" {
                let approval = self.request_human_approval_general(
                    &node_def.approval_prompt, &node_def.id, task_iri
                ).await?;

                let status = if approval.approved { "approved" } else { "rejected" };
                let summary = format!("[HumanApproval] {}: {}",
                    if approval.approved { "批准" } else { "拒绝" },
                    approval.comment.as_deref().unwrap_or(""));

                completed_node_results.insert(node_def.id.clone(), crate::core::workflow::NodeResult {
                    node_id: node_def.id.clone(),
                    status: status.to_string(),
                    summary: summary.clone(),
                    archive_iri: None,
                    turn_count: 0,
                    tool_call_count: 0,
                    error: if approval.approved { None } else { Some("用户拒绝".to_string()) },
                    output: None,
                    artifacts: vec![],
                });

                prev_summary = Some(format!("## 人工审批结果\n{}", summary));
                last_result = Some(TaskResult {
                    task_iri: task_iri.to_string(),
                    status: status.to_string(),
                    summary,
                    output: None,
                    jsonld_output: None,
                    artifacts: vec![],
                    errors: vec![],
                    turn_count: 0,
                    tool_call_count: 0,
                    five_w2h_updates: None,
                    tracked_actions: vec![],
                    archive_iri: None,
                });

                // 审批拒绝且设定了拒绝跳转 → 跳过中间节点直达目标
                if !approval.approved {
                    if let Some(ref reject_target) = node_def.approval_next_on_reject {
                        let mut found = false;
                        for skip_idx in (i + 1)..order.len() {
                            let skip_id = dag.graph[order[skip_idx]].def.id.clone();
                            if skip_id == *reject_target {
                                found = true;
                                break;
                            }
                            skip_nodes.insert(skip_id);
                        }
                        if !found {
                            // 找不到目标节点则跳过所有剩余节点
                            for skip_idx in (i + 1)..order.len() {
                                skip_nodes.insert(dag.graph[order[skip_idx]].def.id.clone());
                            }
                        }
                    }
                }

                // 审批通过且设定了通过跳转 → 跳过中间节点
                if approval.approved {
                    if let Some(ref approve_target) = node_def.approval_next_on_approve {
                        let mut found = false;
                        for skip_idx in (i + 1)..order.len() {
                            let skip_id = dag.graph[order[skip_idx]].def.id.clone();
                            if skip_id == *approve_target {
                                found = true;
                                break;
                            }
                            skip_nodes.insert(skip_id);
                        }
                        if !found {
                            for skip_idx in (i + 1)..order.len() {
                                skip_nodes.insert(dag.graph[order[skip_idx]].def.id.clone());
                            }
                        }
                    }
                }

                info!(node_id = %node_def.id, status = %status, "HumanApprovalNode 处理完成");
                continue;
            }

            let cycle_hints = self.active_cycles
                .values()
                .find(|c| c.task_iri == task_iri)
                .map(|c| c.experience_hints.clone())
                .unwrap_or_default();
            let hints_block = if cycle_hints.is_empty() {
                String::new()
            } else {
                format!("\n\n## 历史经验\n{}", cycle_hints.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n"))
            };
            let objective = match (&prev_summary, step.role) {
                (Some(_summary), AgentRole::Plan) => {
                    format!("{}\n\n## 用户任务\n{}{}\n\n请为上述用户任务制定详细的执行计划。", step.objective, user_input, hints_block)
                }
                (Some(summary), AgentRole::Do) => {
                    format!("{}\n\n上级PA的计划:\n{}{}\n\n请按照计划执行任务。", step.objective, summary, hints_block)
                }
                (Some(summary), AgentRole::Check) => {
                    format!("{}\n\n执行结果:\n{}{}\n\n请验证执行结果是否正确和完整。", step.objective, summary, hints_block)
                }
                (Some(summary), AgentRole::Act) => {
                    format!("{}\n\n检查结论:\n{}{}\n\n请做出最终决策和总结。", step.objective, summary, hints_block)
                }
                (None, AgentRole::Plan) => {
                    format!("{}\n\n## 用户任务\n{}{}\n\n请为上述用户任务制定详细的执行计划。", step.objective, user_input, hints_block)
                }
                _ => step.objective.clone(),
            };

            if step.role == AgentRole::Check {
                let missing = five_w2h.check_completeness(task_level);
                if !missing.is_empty() {
                    info!(missing_dims = ?missing, "5W2H 完形校验：缺失维度，补充默认值");
                    for dim in &missing {
                        match dim.as_str() {
                            "who" => {
                                five_w2h.record_fill("who", FillStage::Do, "SA-Default");
                            }
                            "when" => {
                                five_w2h.record_fill("when", FillStage::Do, "SA-Default");
                            }
                            "where" => {
                                five_w2h.record_fill("where", FillStage::Do, "SA-Default");
                            }
                            "how" => {
                                five_w2h.record_fill("how", FillStage::Do, "SA-Default");
                            }
                            "how_much" => {
                                five_w2h.record_fill("how_much", FillStage::Do, "SA-Default");
                            }
                            _ => {}
                        }
                    }
                }
            }

            let mut context = TaskContext::new(
                task_iri,
                &objective,
                self.max_iterations,
            )
            .with_original_task(user_input)
            .with_step_info(&step.expected_output, &step.success_criteria);

            context = context.with_five_w2h(five_w2h_iri, five_w2h.clone());

            // Resume 模式：在第一个实际执行的 step 恢复历史消息
            // 注意：resume 模式下 PA (i=0) 被跳过，第一个执行的是 DA (i=1)
            let is_first_executed_step = if resume_skip_phases.is_empty() {
                i == 0
            } else {
                // 跳过阶段之后的第一个 step
                !resume_skip_phases.contains(&step.role) 
                    && plan.steps[..i].iter().all(|s| resume_skip_phases.contains(&s.role))
            };
            if is_first_executed_step {
                if let Some(ref msgs) = resumed_messages {
                    // 从 resumed_messages 推算 turn/tool 计数
                    let turn_count = msgs.iter().filter(|m| m.role == "assistant").count() as u32;
                    let tool_count = msgs.iter().filter(|m| m.role == "tool" || m.tool_call_id.is_some()).count() as u32;
                    context = context.with_resumed_messages(msgs.clone(), turn_count, tool_count);
                }
            }

            if let Some(ref summary) = prev_summary {
                context = context.with_prev_summary(summary);
            }

            self.check_and_process_supplementary_inputs(
                task_iri, &step.role, &step.objective,
            ).await?;

            {
                let cycle_start = self.active_cycles.get(&cycle_id).map(|c| c.started_at);
                if let Some(started_at) = cycle_start {
                    let elapsed = chrono::Utc::now().signed_duration_since(started_at);
                    if elapsed.num_seconds() > self.perception.cycle_timeout_secs() {
                        let intervention = self.perception.on_cycle_timeout(&cycle_id, task_iri, elapsed.num_seconds() as f64);
                        if intervention.should_interrupt {
                            // 使用超时防止干预处理阻塞步骤调度
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                self.execute_intervention(intervention, task_iri),
                            ).await;
                        }
                    }
                }
            }

            // 检查执行是否被暂停（通过补充输入动作 PauseExecution）
            let paused = self.active_cycles.get(&cycle_id)
                .map(|c| c.phase == CyclePhase::Idle)
                .unwrap_or(false);
            if paused {
                info!(step_id = %step.step_id, role = ?step.role, "执行已暂停，等待恢复");
                // 循环等待恢复，同时检查补充输入
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let mut payloads = Vec::new();
                    if let Some(ref mut receiver) = self.event_receiver {
                        while let Ok(event) = receiver.try_recv() {
                            if event.task_iri != task_iri {
                                continue;
                            }
                            if event.event_type == "USER_SUPPLEMENTARY_INPUT" {
                                payloads.push(event.payload.clone());
                            }
                        }
                    }
                    for payload in payloads {
                        self.enqueue_supplementary_input(task_iri, &payload);
                    }
                    let resumed = self.active_cycles.get(&cycle_id)
                        .map(|c| c.phase == CyclePhase::Executing)
                        .unwrap_or(false);
                    if resumed {
                        break;
                    }
                }
            }

            let role_name = format!("{:?}", step.role);
            self.emit_sa_thought(task_iri,
                &format!("Phase {}/{}: dispatching {} — {}",
                    i + 1, plan.steps.len(), role_name, step.objective),
                &format!("dispatch_{}", role_name.to_lowercase())).await;

            if plan.parallel_groups.iter().any(|g| g.len() > 1 && g.contains(&step.role)) {
                let matching_groups: Vec<_> = plan.parallel_groups.iter()
                    .filter(|g| g.contains(&step.role))
                    .collect();
                let parallel_group = match matching_groups.first() {
                    Some(g) => (*g).clone(),
                    None => {
                        warn!(role = ?step.role, "No parallel group found for role despite any() check");
                        continue;
                    }
                };
                let count = parallel_group.len();
                let results = self.dispatch_agents_parallel(
                    step.role, count, &step.objective, task_iri, &cycle_id, self.max_iterations,
                ).await?;

                let failed = results.iter().find(|r| r.status == "failed");
                if let Some(f) = failed {
                    warn!(role = ?step.role, step_id = %step.step_id, "Parallel agent failed");
                    return Ok(TaskResult {
                        task_iri: task_iri.to_string(),
                        status: "partial_failure".to_string(),
                        summary: format!("Some parallel {:?} agents failed", step.role),
                        output: None,
                        jsonld_output: None,
                        artifacts: Vec::new(),
                        errors: f.errors.clone(),
                        turn_count: results.iter().map(|r| r.turn_count).sum(),
                        tool_call_count: results.iter().map(|r| r.tool_call_count).sum(),
                        five_w2h_updates: None,
                        tracked_actions: Vec::new(),
                        archive_iri: None,
                    });
                }

                let combined_summary: String = results.iter()
                    .map(|r| {
                        let iri_part = r.archive_iri.as_ref()
                            .map(|iri| format!(" | read_agent_output 查询: {}", iri))
                            .unwrap_or_default();
                        format!("[{}] {}{}", r.task_iri, r.summary, iri_part)
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                prev_summary = Some(combined_summary);
                last_result = results.into_iter().last();
            } else {
                let result = self.dispatch_agent(step.role, context, &cycle_id, Some(step.clone())).await?;

                if result.status == "failed" {
                    warn!(role = ?step.role, step_id = %step.step_id, "Agent failed, aborting plan");
                    let error_detail = result.errors.first()
                        .map(|e| format!("\n\n**Error details**: {}", e))
                        .unwrap_or_default();
                    return Ok(TaskResult {
                        task_iri: task_iri.to_string(),
                        status: "failed".to_string(),
                        summary: format!("Agent {:?} failed at step {}{}", step.role, step.step_id, error_detail),
                        output: None,
                        jsonld_output: None,
                        artifacts: Vec::new(),
                        errors: result.errors,
                        turn_count: result.turn_count,
                        tool_call_count: result.tool_call_count,
                        five_w2h_updates: None,
                        tracked_actions: Vec::new(),
                        archive_iri: None,
                    });
                }

                if let Some(ref updates) = result.five_w2h_updates {
                    if let Ok(Some(snapshot)) = self.runner.l0_store.retrieve(&five_w2h_iri) {
                        if let Ok(mut node) = serde_json::from_str::<serde_json::Value>(&snapshot.content) {
                            let fill_stage = match step.role {
                                AgentRole::Plan => FillStage::Plan,
                                AgentRole::Do => FillStage::Do,
                                AgentRole::Check => FillStage::Check,
                                AgentRole::Act => FillStage::Act,
                            };
                            let filled_by = format!("{:?}", step.role);
                            
                            for (key, _value) in updates.as_object().unwrap_or(&serde_json::Map::new()) {
                                node[key] = updates.get(key).cloned().unwrap_or(serde_json::Value::Null);
                                five_w2h.record_fill(key, fill_stage.clone(), &filled_by);
                            }
                            
                            if let Ok(updated_json_ld) = five_w2h.to_json_ld(task_iri) {
                                let _ = self.runner.l0_store.store(&five_w2h_iri, &updated_json_ld.to_string());
                                let cfg = crate::CoreConfig::default();
                                if let Some(ref bb) = self.blackboard {
                                    if bb.write_node(&five_w2h_iri, &updated_json_ld.to_string(), &cfg).is_ok() {
                                        tracing::debug!(five_w2h_iri = %five_w2h_iri, "5W2H 更新同步到黑板");
                                    }
                                }
                            }
                        }
                    }
                }

                if step.role == AgentRole::Act && result.status == "success" {
                    // 只在最后一个 AA 步骤冻结 5W2H（多轮 PDCA 的中间 AA 不冻结）
                    let is_last_aa = plan.steps.iter().rposition(|s| s.role == AgentRole::Act)
                        .map(|last_act| plan.steps.iter().position(|s| s.step_id == step.step_id)
                            .map(|idx| idx >= last_act)
                            .unwrap_or(true))
                        .unwrap_or(true);
                    if is_last_aa {
                        five_w2h.freeze();
                        if let Ok(frozen_json_ld) = five_w2h.to_json_ld(task_iri) {
                            let snapshot_iri = format!("iri://task/{}/snapshot", task_id);
                            let _ = self.runner.l0_store.store(&snapshot_iri, &frozen_json_ld.to_string());
                            let _ = self.runner.l0_store.store(&five_w2h_iri, &frozen_json_ld.to_string());
                            let cfg = crate::CoreConfig::default();
                            if let Some(ref bb) = self.blackboard {
                                let _ = bb.write_node(&snapshot_iri, &frozen_json_ld.to_string(), &cfg);
                                let _ = bb.write_node(&five_w2h_iri, &frozen_json_ld.to_string(), &cfg);
                            }
                            info!(task_iri = %task_iri, "5W2H 已冻结归档");
                        }
                    } else {
                        info!(task_iri = %task_iri, step_id = %step.step_id, "中间 AA 步骤：5W2H 暂不冻结");
                    }
                }

                self.sharing.create_share(
                    &format!("iri://agent/{}", step.role),
                    "iri://agent/next",
                    &[format!("iri://task/{}/result", task_iri)],
                    ShareType::Projection,
                    Permission::Read,
                    Some(3600),
                    None,
                );

                if step.role == AgentRole::Plan && result.status == "success" {
                    let plan_data = serde_json::json!({
                        "summary": &result.summary,
                        "objective": &step.objective,
                    });
                    let advisories = self.perception.on_plan_completed(&plan_data, task_iri);
                    if !advisories.is_empty() {
                        info!(count = advisories.len(), "PA 感知建议已生成");
                    }
                }

                if step.role == AgentRole::Check && result.status == "success" {
                    let check_data = serde_json::json!({
                        "summary": &result.summary,
                        "objective": &step.objective,
                    });
                    if let Some(advisory) = self.perception.on_check_completed(&check_data, task_iri) {
                        info!(advisory = ?advisory, "CA 感知建议已生成");
                    }
                }

                // 多轮 PDCA 提前退出检查：AA 评估是 PDCA 循环的终审，
                // 无论通过还是失败，都应该终止后续循环，避免重复执行。
                if step.role == AgentRole::Act {
                    let has_remaining = (i + 1) < order.len();
                    if has_remaining {
                        let reason = match result.status.as_str() {
                            "success" => "AA 通过，任务已完成",
                            "failed" | "partial_success" => "AA 未通过",
                            _ => "AA 已评估",
                        };
                        info!(step_id = %step.step_id, status = %result.status, "{}，跳过剩余 PDCA 循环", reason);
                        for skip_idx in (i + 1)..order.len() {
                            skip_nodes.insert(dag.graph[order[skip_idx]].def.id.clone());
                        }
                    }
                }

                if step.role == AgentRole::Do
                    && (result.status == "success" || result.status == "partial_success")
                    && plan.max_recursion_depth > 0
                    && (plan.task_complexity == TaskComplexity::Recursive || plan.task_complexity == TaskComplexity::Complex)
                {
                    let sub_results = self.execute_recursive_sub_cycle(
                        &result.summary,
                        task_iri,
                        &cycle_id,
                        &step.step_id,
                        plan.max_recursion_depth,
                        1,
                        &five_w2h,
                        five_w2h_iri,
                    ).await;

                    match sub_results {
                        Ok(sub_summary) => {
                            let combined = format!(
                                "{}\n\n## 子任务执行结果\n{}",
                                result.summary, sub_summary
                            );
                            prev_summary = Some(combined);
                        }
                        Err(e) => {
                            warn!(error = %e, "递归子循环执行失败，继续使用 DA 原始结果");
                            let prev = match result.archive_iri {
                                Some(ref iri) => format!("{}\n\n如需查看完整报告，可使用 read_agent_output 工具查询: {}", result.summary, iri),
                                None => result.summary.clone(),
                            };
                            prev_summary = Some(prev);
                        }
                    }
                } else {
                    let prev = match result.archive_iri {
                        Some(ref iri) => format!("{}\n\n如需查看完整报告，可使用 read_agent_output 工具查询: {}", result.summary, iri),
                        None => result.summary.clone(),
                    };
                    prev_summary = Some(prev);
                }

                last_result = Some(result);
            }

            if let Some(alert) = self.perception.check_5w2h_constraints(five_w2h_iri) {
                tracing::warn!(alert = %alert, "5W2H 约束告警");
                self.event_bus.emit(task_iri, &alert, "SA", &serde_json::json!({"task_iri": task_iri}).to_string()).await;
            }

            info!(step_id = %step.step_id, role = ?step.role, status = ?last_result.as_ref().map(|r| &r.status), "Step completed");

            // ── 步骤级 checkpoint：保存完整执行上下文 ──
            {
                let cm = crate::core::checkpoint::CheckpointManager::with_persistence(self.runner.l0_store.clone());
                let role_name = format!("{:?}", step.role);
                let state_json = serde_json::json!({
                    "turn": last_result.as_ref().map(|r| r.turn_count).unwrap_or(0),
                    "tc": last_result.as_ref().map(|r| r.tool_call_count).unwrap_or(0),
                    "prompt_tokens": self.runner.total_prompt_tokens.load(std::sync::atomic::Ordering::Relaxed),
                    "completion_tokens": self.runner.total_completion_tokens.load(std::sync::atomic::Ordering::Relaxed),
                }).to_string();

                let cycle_state = self.active_cycles.get(&cycle_id).map(|c| serde_json::json!({
                    "phase": format!("{:?}", c.phase),
                    "iteration": c.iteration,
                    "phase_history": c.phase_history,
                    "task_completed": c.task_completed,
                    "experience_hints": c.experience_hints,
                }).to_string());

                let completed_nodes = if completed_node_results.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&completed_node_results).unwrap_or_default())
                };

                let pending_approvals = {
                    let map = self.pending_approvals.lock().await;
                    if map.is_empty() {
                        None
                    } else {
                        Some(serde_json::to_string(&*map).unwrap_or_default())
                    }
                };

                let supplement_data = {
                    let pending = self.supplement_store.take_pending(task_iri);
                    if pending.is_empty() {
                        None
                    } else {
                        let entries: Vec<serde_json::Value> = pending.iter().map(|e| serde_json::json!({
                            "content": e.content,
                            "relevance_score": e.relevance_score,
                            "timestamp": e.timestamp,
                        })).collect();
                        Some(serde_json::to_string(&entries).unwrap_or_default())
                    }
                };

                let cp_name = format!("step_complete_{}", role_name);
                let tags = vec![role_name.clone(), "step_complete".to_string()];

                if let Err(e) = cm.create_ext(
                    task_iri,
                    &cp_name,
                    "[]",
                    "[]",  // step_boundary: messages are in finer-grained React loop checkpoints
                    &state_json,
                    &tags,
                    Some(&role_name),
                    None, // five_w2h_json — 5W2H 已通过 L0 持久化
                    prev_summary.as_deref(),
                    cycle_state.as_deref(),
                    completed_nodes.as_deref(),
                    pending_approvals.as_deref(),
                    supplement_data.as_deref(),
                    None,
                    None,
                    None,
                ) {
                    warn!("[checkpoint] step_complete 保存失败: {}", e);
                } else {
                    info!("[checkpoint] step_complete_{} 已保存", role_name);
                }
            }
        }

        if let Some(cycle) = self.active_cycles.get_mut(&cycle_id) {
            cycle.phase = CyclePhase::Completed;
            cycle.task_completed = true;
            cycle.phase_history.push("Completed".to_string());
        }

        self.event_bus
            .emit(task_iri, "CYCLE_COMPLETED", "SA",
                &serde_json::json!({"cycle_id": &cycle_id}).to_string())
            .await;

        Ok(last_result.unwrap_or(TaskResult {
            task_iri: task_iri.to_string(),
            status: "completed".to_string(),
            summary: "No agents executed".to_string(),
            output: None,
            jsonld_output: None,
            artifacts: Vec::new(),
            errors: Vec::new(),
            turn_count: 0,
            tool_call_count: 0,
            five_w2h_updates: None,
            tracked_actions: Vec::new(),
            archive_iri: None,
        }))
    }

    fn execute_recursive_sub_cycle<'a>(
        &'a self,
        da_summary: &'a str,
        task_iri: &'a str,
        cycle_id: &'a str,
        parent_step_id: &'a str,
        max_depth: u32,
        current_depth: u32,
        five_w2h: &'a crate::core::five_w2h::Task5W2H,
        five_w2h_iri: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, CoreError>> + Send + 'a>> {
        Box::pin(async move {
        if current_depth > max_depth {
            info!(depth = current_depth, max_depth, "递归深度已达上限，停止子循环");
            return Ok("递归深度已达上限".to_string());
        }

        self.emit_sa_thought(task_iri,
            &format!("▶ 递归子循环 (depth {}/{})", current_depth, max_depth),
            "recursive_sub_cycle_start").await;

        let sub_task = SubTask::new(
            &format!("从 DA 结果中分解子任务 (depth={})", current_depth),
            parent_step_id,
            current_depth,
        );

        info!(
            sub_task_id = %sub_task.sub_task_id,
            depth = current_depth,
            max_depth,
            "开始递归子循环"
        );

        self.emit_sa_thought(task_iri,
            &format!("正在分解 DA 结果，识别子任务... (depth {}/{})", current_depth, max_depth),
            "recursive_decompose").await;

        let decompose_prompt = format!(
            r#"你是一个任务分解专家。以下是一个 DA (Do Agent) 的执行结果摘要，请分析其中是否有需要进一步执行的子任务。

## DA 执行结果
{}

## 任务上下文
- 原始目标: {}
- 当前递归深度: {}/{}

## 输出要求
请以 JSON 格式输出需要进一步执行的子任务列表。如果没有需要进一步执行的子任务，返回空数组。

```json
{{
  "has_sub_tasks": true/false,
  "sub_tasks": [
    {{
      "objective": "子任务目标描述",
      "role": "Do",
      "success_criteria": "成功标准"
    }}
  ]
}}
```

## 判断标准
1. 如果 DA 结果中明确提到"还需要..."、"下一步需要..."等，则存在子任务
2. 如果 DA 结果已经完整完成目标，没有遗留工作，则无子任务
3. 子任务应该是具体可执行的，而非抽象的
4. 最多分解 3 个子任务

请直接输出 JSON。"#,
            da_summary,
            five_w2h.what,
            current_depth,
            max_depth,
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![crate::gateway::unified_gateway::ChatMessage {
            role: "user".to_string(),
            content: decompose_prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];

        let response = self.runner.gateway.chat_with_params(
            &model,
            messages,
            Some(0.3),
            Some(1000),
            None,
            None,
        ).await.map_err(|e| CoreError::Internal { message: format!("递归分解 LLM 调用失败: {}", e) })?;

        let content = response.choices.first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let json_str = if content.starts_with('{') {
            content.clone()
        } else if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                content[start..=end].to_string()
            } else {
                content.clone()
            }
        } else {
            return Ok("LLM 未返回有效分解结果".to_string());
        };

        #[derive(Deserialize)]
        struct DecomposeResult {
            has_sub_tasks: bool,
            sub_tasks: Vec<SubTaskDef>,
        }

        #[derive(Deserialize)]
        struct SubTaskDef {
            objective: String,
            #[serde(default = "default_role")]
            role: String,
            success_criteria: String,
        }

        fn default_role() -> String { "Do".to_string() }

        let parsed: DecomposeResult = serde_json::from_str(&json_str)
            .map_err(|e| CoreError::Internal { message: format!("递归分解 JSON 解析失败: {}", e) })?;

        if !parsed.has_sub_tasks || parsed.sub_tasks.is_empty() {
            info!(depth = current_depth, "DA 结果无需进一步分解");
            self.emit_sa_thought(task_iri,
                &format!("子任务分解完成：无需进一步分解 (depth {}/{})", current_depth, max_depth),
                "recursive_no_tasks").await;
            return Ok("无需进一步分解".to_string());
        }

        self.emit_sa_thought(task_iri,
            &format!("识别到 {} 个子任务 (depth {}/{})", parsed.sub_tasks.len(), current_depth, max_depth),
            "recursive_tasks_found").await;

        let mut sub_summaries = Vec::new();

        for (idx, sub_def) in parsed.sub_tasks.iter().enumerate() {
            let sub_objective = format!("[递归depth={}] {}", current_depth, sub_def.objective);
            info!(sub_idx = idx, objective = %sub_def.objective, "执行递归子任务");

            let mut sub_ctx = TaskContext::new(
                task_iri,
                &sub_objective,
                self.max_iterations.min(8),
            ).with_original_task(&sub_def.objective);

            sub_ctx = sub_ctx.with_five_w2h(five_w2h_iri, five_w2h.clone());

            if let Some(ref bb) = self.blackboard {
                let nodes = bb.query_nodes(task_iri).unwrap_or_default();
                if !nodes.is_empty() {
                    let summaries: Vec<String> = nodes.iter()
                        .filter_map(|n| {
                            let parsed: serde_json::Value = serde_json::from_str(&n.json_ld).ok()?;
                            parsed.get("summary").and_then(|s| s.as_str()).map(String::from)
                        })
                        .collect();
                    if !summaries.is_empty() {
                        sub_ctx = sub_ctx.with_prev_summary(&summaries.join("\n"));
                    }
                }
            }

            let sub_step = PlanStep {
                step_id: format!("{}_sub_{}", parent_step_id, idx),
                role: AgentRole::Do,
                objective: sub_def.objective.clone(),
                expected_output: sub_def.success_criteria.clone(),
                dependencies: vec![parent_step_id.to_string()],
                tools_allowed: vec![],
                success_criteria: sub_def.success_criteria.clone(),
            };

            let total = parsed.sub_tasks.len();
            self.emit_sa_thought(task_iri,
                &format!("▶ 执行子任务 {}/{}: {} (depth {})", idx + 1, total, sub_def.objective, current_depth),
                "recursive_sub_task_start").await;

            let sub_result = self.dispatch_agent(AgentRole::Do, sub_ctx, cycle_id, Some(sub_step)).await?;

            self.emit_sa_thought(task_iri,
                &format!("{}/{} 子任务完成 [{}]: {}", idx + 1, total,
                    sub_result.status, sub_def.objective),
                "recursive_sub_task_end").await;

            if sub_result.status == "success" || sub_result.status == "partial_success" {
                let icon = if sub_result.status == "success" { "✅" } else { "⚠️" };
                sub_summaries.push(format!("### 子任务 {} {}\n{}", idx + 1, icon, sub_result.summary));

                if current_depth < max_depth && sub_result.status == "success" {
                    // 只有完全成功的子任务才继续深层递归；partial_success 在上层递归中继续
                    self.emit_sa_thought(task_iri,
                        &format!("进入深层递归 (depth {}/{})", current_depth + 1, max_depth),
                        "recursive_deeper").await;
                    match self.execute_recursive_sub_cycle(
                        &sub_result.summary,
                        task_iri,
                        cycle_id,
                        &format!("{}_sub_{}", parent_step_id, idx),
                        max_depth,
                        current_depth + 1,
                        five_w2h,
                        five_w2h_iri,
                    ).await {
                        Ok(deeper_summary) => {
                            sub_summaries.push(format!("#### 深层子任务 (depth={})\n{}", current_depth + 1, deeper_summary));
                        }
                        Err(e) => {
                            warn!(error = %e, "深层递归子循环失败");
                        }
                    }
                }
            } else {
                sub_summaries.push(format!("### 子任务 {} ❌\n执行失败: {}", idx + 1, sub_result.summary));
            }
        }

        self.emit_sa_thought(task_iri,
            &format!("递归子循环完成 (depth {}/{})", current_depth, max_depth),
            "recursive_sub_cycle_end").await;
        Ok(sub_summaries.join("\n\n"))
        })
    }

    async fn execute_intervention(
        &mut self,
        plan: crate::perception::proactive_engine::InterventionPlan,
        task_iri: &str,
    ) -> Result<(), CoreError> {
        if !plan.should_interrupt {
            warn!(actions = ?plan.actions, "非中断性干预建议，仅记录");
            return Ok(());
        }

        warn!(actions = ?plan.actions, "执行干预计划");

        // 1. LLM 分类决策：将事件映射到预定义动作
        let (action, params) = self.analyze_anomaly_with_llm(&plan, task_iri).await
            .unwrap_or_else(|e| {
                warn!(error = %e, "LLM 分类决策失败，使用默认 ContinueWithMonitor");
                (InterventionAction::ContinueWithMonitor, ActionParams::default())
            });

        info!(action = ?action, "LLM 分类决策结果");

        // 2. IncreaseBudget 特殊处理：需要人工确认
        if matches!(action, InterventionAction::IncreaseBudget { .. }) {
            info!("IncreaseBudget 需要人工确认");
            let approved = self.request_human_approval(&action, task_iri).await?;
            if !approved {
                info!("IncreaseBudget 未获人工确认，降级为 FreezeAndReport");
                let fallback_action = InterventionAction::FreezeAndReport;
                if let Some(handler) = get_action_handler(&fallback_action) {
                    return handler(self, ActionParams::default(), task_iri).await;
                }
                return Ok(());
            }
        }

        // 3. 注册表分发：查找并执行动作 handler
        let handler = get_action_handler(&action)
            .ok_or_else(|| CoreError::Internal {
                message: format!("Unknown action handler for: {:?}", action),
            })?;
        handler(self, params, task_iri).await?;

        // 4. 发射干预执行事件
        self.event_bus.emit(task_iri, "INTERVENTION_EXECUTED", "SA",
            &serde_json::json!({"action": format!("{:?}", action)}).to_string()).await;

        Ok(())
    }

    /// LLM 分类决策：将干预计划映射到预定义动作
    async fn analyze_anomaly_with_llm(
        &self,
        plan: &crate::perception::proactive_engine::InterventionPlan,
        _task_iri: &str,
    ) -> Result<(InterventionAction, ActionParams), CoreError> {
        use crate::gateway::unified_gateway::ChatMessage;

        let prompt = format!(
            r#"你是一个异常诊断专家。根据以下干预计划，从预定义动作中选择最合适的动作。

## 当前干预计划
- 诊断: {}
- 建议动作: {}
- 优先级: {}
- 是否中断: {}

## 预定义动作列表（请严格从以下选择 ONE 个最合适的动作）

### 1. 正常继续（无需中断）
- Continue: 不做干预，继续执行
- ContinueWithMonitor: 继续执行但加强监控

### 2. 参数调整（无需中断）
- IncreaseRetry: 增加重试次数
- IncreaseTimeout: 增加超时时间
- ReduceComplexity: 降低复杂度预期
- RestrictTools: 限制可用工具集

### 3. 执行流调整（需要中断）
- SkipStep: 跳过当前步骤
- RetryStep: 重试当前步骤
- Parallelize: 并行化执行
- SplitStep: 拆分为多个子步骤
- InsertExtraStep: 插入额外的验证/修复步骤

### 4. 资源与模式切换（需要中断）
- FallbackToShallow: 回退到浅层模式
- EmergencyMode: 进入紧急模式
- FreezeAndReport: 冻结状态并生成报告

### 5. 终止与升级（需要中断）
- AbortTask: 终止当前任务
- NotifyHuman: 通知人工介入

## 输出要求
请以 JSON 格式输出，仅包含以下字段：
{{
  "action": "选中的动作名",
  "params": {{ /* 动作参数 */ }},
  "reasoning": "选择该动作的原因"
}}

注意：
1. 只输出 JSON，不要额外内容
2. action 必须从以上列表中严格选择
3. IncreaseBudget 需要人工确认，只有确认资源预算不足时才选择
4. AbortTask 是最后手段，仅在无法恢复时使用"#,
            plan.diagnosis,
            plan.actions.join(", "),
            plan.priority,
            plan.should_interrupt,
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.runner.gateway.chat_with_params(
                &model, messages, Some(0.1), Some(1000), None, None,
            ),
        ).await
            .map_err(|_| CoreError::Internal {
                message: "LLM intervention analysis timed out after 30s".to_string(),
            })?
            .map_err(|e| CoreError::Internal {
                message: format!("LLM intervention analysis failed: {}", e),
            })?;
        let content = response.choices.first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| CoreError::Internal {
                message: "No LLM response content".to_string(),
            })?;

        let json_str = if content.starts_with('{') {
            content
        } else if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                content[start..=end].to_string()
            } else {
                return Err(CoreError::Internal {
                    message: "No JSON found in LLM response".to_string(),
                });
            }
        } else {
            return Err(CoreError::Internal {
                message: "No JSON found in LLM response".to_string(),
            });
        };
        let parsed: LlmActionDecision = serde_json::from_str(&json_str)
            .map_err(|e| CoreError::Internal {
                message: format!("Failed to parse LLM action decision: {}", e),
            })?;

        let action = InterventionAction::from_name(&parsed.action, parsed.params.clone())?;
        Ok((action, parsed.params))
    }

    /// IncreaseBudget 人工确认流程
    async fn request_human_approval(
        &self,
        action: &InterventionAction,
        task_iri: &str,
    ) -> Result<bool, CoreError> {
        let request_id = format!("approval_{}", uuid::Uuid::new_v4().hyphenated());
        let details = match action {
            InterventionAction::IncreaseBudget { additional_tokens, additional_time_secs } => {
                serde_json::json!({
                    "request_id": request_id,
                    "action": "IncreaseBudget",
                    "additional_tokens": additional_tokens,
                    "additional_time_secs": additional_time_secs,
                    "task_iri": task_iri,
                    "message": format!(
                        "需要人工确认: 是否增加 Token 预算 {} tokens, 额外时间 {} 秒?",
                        additional_tokens, additional_time_secs
                    ),
                    "status": "pending",
                })
            }
            _ => return Ok(true),
        };

        self.event_bus.emit_with_priority(
            task_iri,
            "HUMAN_APPROVAL_REQUIRED",
            "SA",
            &details.to_string(),
            EventPriority::High,
        ).await;

        info!(request_id = %request_id, "等待人工确认");

        let iri = format!("iri://approval/{}", request_id);
        let _ = self.runner.l0_store.store(&iri, &details.to_string());

        // 非阻塞等待：将待确认请求注册到 pending_approvals
        // 外部系统通过 EventBus 的 HUMAN_APPROVAL_RESULT 事件返回确认结果
        // SA 在 process_task 主循环中检查该事件并更新 pending_approvals
        self.pending_approvals.lock().await.insert(request_id.clone(), false);

        // 等待一小段时间检查是否有即时审批结果
        let mut receiver = self.event_bus.subscribe();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Ok(event) = receiver.try_recv() {
                if event.event_type == "HUMAN_APPROVAL_RESULT" {
                    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        if result.get("request_id").and_then(|v| v.as_str()) == Some(&request_id) {
                            let approved = result.get("approved").and_then(|v| v.as_bool()).unwrap_or(false);
                            self.pending_approvals.lock().await.insert(request_id, approved);
                            return Ok(approved);
                        }
                    }
                }
            }
        }

        info!(request_id = %request_id, "人工确认等待超时（5s），任务继续，等待异步确认");
        Ok(false)
    }

    /// 通用人工审批请求（用于 HumanApprovalNode 工作流节点）
    async fn request_human_approval_general(
        &self,
        prompt: &str,
        node_id: &str,
        task_iri: &str,
    ) -> Result<HumanApprovalNodeResult, CoreError> {
        let request_id = format!("approval_{}", uuid::Uuid::new_v4().hyphenated());
        let details = serde_json::json!({
            "request_id": request_id,
            "action": "WorkflowNodeApproval",
            "node_id": node_id,
            "task_iri": task_iri,
            "prompt": prompt,
            "status": "pending",
        });

        self.event_bus.emit_with_priority(
            task_iri,
            "HUMAN_APPROVAL_REQUIRED",
            "SA",
            &details.to_string(),
            EventPriority::High,
        ).await;

        info!(request_id = %request_id, node_id = %node_id, "HumanApprovalNode: 等待人工确认");

        let iri = format!("iri://approval/{}", request_id);
        let _ = self.runner.l0_store.store(&iri, &details.to_string());

        self.pending_approvals.lock().await.insert(request_id.clone(), false);

        // 等待一小段时间检查是否有即时审批结果
        let mut receiver = self.event_bus.subscribe();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Ok(event) = receiver.try_recv() {
                if event.event_type == "HUMAN_APPROVAL_RESULT" {
                    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        if result.get("request_id").and_then(|v| v.as_str()) == Some(&request_id) {
                            let approved = result.get("approved").and_then(|v| v.as_bool()).unwrap_or(false);
                            let comment = result.get("comment").and_then(|v| v.as_str()).map(String::from);
                            self.pending_approvals.lock().await.insert(request_id, approved);
                            return Ok(HumanApprovalNodeResult { node_id: node_id.to_string(), approved, comment });
                        }
                    }
                }
            }
        }

        info!(request_id = %request_id, "HumanApprovalNode: 等待超时（5s），任务继续，等待异步确认");
        // 超时后默认不阻塞流程——标记为未审批，走拒绝逻辑
        Ok(HumanApprovalNodeResult {
            node_id: node_id.to_string(),
            approved: false,
            comment: Some("审批超时，默认拒绝".to_string()),
        })
    }

    /// 将用户补充输入加入队列，等待 SA 处理
    pub fn enqueue_supplementary_input(&mut self, task_iri: &str, content: &str) {
        self.supplementary_inputs
            .entry(task_iri.to_string())
            .or_default()
            .push((content.to_string(), "pending".to_string()));
        info!(task_iri = %task_iri, "用户补充输入已入队");
    }

    /// 在 execute_plan 步骤间检查和执行补充输入
    async fn check_and_process_supplementary_inputs(
        &mut self,
        task_iri: &str,
        step_role: &AgentRole,
        step_objective: &str,
    ) -> Result<(), CoreError> {
        let mut supp_payloads = Vec::new();
        let mut pending_interventions: Vec<crate::perception::proactive_engine::InterventionPlan> = Vec::new();
        if let Some(ref mut receiver) = self.event_receiver {
            while let Ok(event) = receiver.try_recv() {
                if event.task_iri != task_iri {
                    continue;
                }
                match event.event_type.as_str() {
                    "USER_SUPPLEMENTARY_INPUT" => {
                        supp_payloads.push(event.payload.clone());
                    }
                    "AGENT_ERROR" => {
                        let plan = self.perception.on_agent_blocked(&event.source_agent_iri, task_iri);
                        if plan.should_interrupt {
                            pending_interventions.push(plan);
                        }
                    }
                    "THRESHOLD_EXCEEDED" => {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let plan = self.perception.on_quality_degradation(&payload, task_iri);
                            if plan.should_interrupt {
                                pending_interventions.push(plan);
                            }
                        }
                    }
                    "CYCLE_ITERATION" => {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let plan = self.perception.on_progress_anomaly(&payload, task_iri);
                            if plan.should_interrupt {
                                pending_interventions.push(plan);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for plan in pending_interventions {
            let _ = self.execute_intervention(plan, task_iri).await;
        }
        for payload in supp_payloads {
            self.enqueue_supplementary_input(task_iri, &payload);
        }

        // 2. 收集待处理的补充输入（避免借用冲突）
        let pending = {
            let inputs = self.supplementary_inputs.get_mut(task_iri);
            inputs.map(|list| {
                list.iter()
                    .filter(|(_, status)| status == "pending")
                    .map(|(content, _)| content.clone())
                    .collect::<Vec<_>>()
            }).unwrap_or_default()
        };

        if pending.is_empty() {
            return Ok(());
        }

        // 3. 逐个处理补充输入
        for supplement in &pending {
            let context = format!("当前步骤: {:?} - {}", step_role, step_objective);
            match self.classify_supplementary_input_with_llm(supplement, &context).await {
                Ok((action, params)) => {
                    info!(action = ?action, "补充输入分类结果");
                    self.execute_supplementary_action(action, params, task_iri, supplement).await?;
                }
                Err(e) => {
                    warn!(error = %e, supplement = %supplement, "补充输入分类失败，默认注入上下文");
                    self.inject_to_current_agent(task_iri, supplement).await;
                }
            }
        }

        // 4. 标记为已处理
        if let Some(input_list) = self.supplementary_inputs.get_mut(task_iri) {
            for item in input_list.iter_mut() {
                item.1 = "processed".to_string();
            }
        }

        Ok(())
    }

    /// LLM 分类决策：将用户补充输入映射到预定义动作
    async fn classify_supplementary_input_with_llm(
        &self,
        user_supplement: &str,
        task_context: &str,
    ) -> Result<(SupplementaryInputAction, ActionParams), CoreError> {
        use crate::gateway::unified_gateway::ChatMessage;

        let prompt = format!(
            r#"你是一个任务引导专家。根据用户的补充输入，从以下预定义动作中选择最合适的动作。

## 当前任务上下文
{}

## 用户补充输入
{}

## 预定义动作列表（请严格选择 ONE 个）

### 1. 信息补充
- AddContext: 用户提供额外上下文/信息
- RefineObjective: 用户细化或调整目标
- ProvideConstraint: 用户提供新的约束条件，如时间限制

### 2. 方向引导
- GuideDirection: 用户指示执行方向/重点
- PrioritizeStep: 用户指定某步骤应优先处理
- SuggestApproach: 用户建议具体方法或方案

### 3. 执行控制
- PauseExecution: 用户请求暂停当前执行
- ResumeExecution: 用户请求恢复执行
- SkipCurrentStep: 用户要求跳过当前步骤

### 4. 反馈纠正
- ConfirmDirection: 用户确认当前方向正确
- CorrectApproach: 用户指出错误并纠正方向
- AbortCurrentStep: 用户要求中止当前步骤

## 输出要求
请以 JSON 格式输出，仅包含以下字段：
{{
  "action": "选中的动作名",
  "params": {{ /* 动作参数，不同动作不同 */ }},
  "reasoning": "选择该动作的原因"
}}

注意：
1. 只输出 JSON，不要额外内容
2. action 必须从以上列表中严格选择
3. 如果用户只是补充信息而非指示，选择 AddContext
4. 只有用户明确要求中止或跳过时才选择 AbortCurrentStep 或 SkipCurrentStep"#,
            task_context,
            user_supplement,
        );

        let model = self.runner.gateway.get_model("default");
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }];
        let response = self.runner.gateway.chat_with_params(
            &model, messages, Some(0.1), Some(1000), None, None,
        ).await?;
        let content = response.choices.first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| CoreError::Internal {
                message: "No LLM response content".to_string(),
            })?;

        let json_str = if content.starts_with('{') {
            content
        } else if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                content[start..=end].to_string()
            } else {
                return Err(CoreError::Internal {
                    message: "No JSON found in LLM response".to_string(),
                });
            }
        } else {
            return Err(CoreError::Internal {
                message: "No JSON found in LLM response".to_string(),
            });
        };

        let parsed: SupplementaryLlmDecision = serde_json::from_str(&json_str)
            .map_err(|e| CoreError::Internal {
                message: format!("Failed to parse LLM supplementary decision: {}", e),
            })?;

        let action = SupplementaryInputAction::from_name(&parsed.action)?;
        Ok((action, parsed.params))
    }

    /// 执行补充输入动作
    async fn execute_supplementary_action(
        &mut self,
        action: SupplementaryInputAction,
        _params: ActionParams,
        task_iri: &str,
        supplement: &str,
    ) -> Result<(), CoreError> {
        match action {
            SupplementaryInputAction::AddContext
            | SupplementaryInputAction::GuideDirection
            | SupplementaryInputAction::ConfirmDirection
            | SupplementaryInputAction::CorrectApproach
            | SupplementaryInputAction::SuggestApproach => {
                // 1. 计算 embedding 和 relevance_score
                let embedding = if let Some(ref embedder) = self.embedder {
                    embedder.embed(supplement).await.ok()
                } else {
                    None
                };
                let relevance_score = embedding.as_ref()
                    .map(|emb| self.relevance_tracker.on_new_input(emb))
                    .unwrap_or(0.5);

                // 2. 存入 SupplementaryInputStore（AgentRunner 在 CycleStart 消费）
                self.supplement_store.store(task_iri, supplement, embedding, relevance_score);
                info!(
                    task_iri = %task_iri,
                    score = relevance_score,
                    "补充输入已存入 SupplementaryInputStore"
                );

                // 3. 保持向后兼容：emit SUPPLEMENTARY_CONTEXT 事件（TUI 渲染用）
                self.inject_to_current_agent(task_iri, supplement).await;
            }
            SupplementaryInputAction::RefineObjective => {
                info!("补充输入: 细化目标");
                self.event_bus.emit(task_iri, "OBJECTIVE_REFINED", "SA",
                    &serde_json::json!({"refinement": supplement}).to_string()).await;
            }
            SupplementaryInputAction::ProvideConstraint => {
                info!("补充输入: 提供约束");
                self.event_bus.emit(task_iri, "CONSTRAINT_ADDED", "SA",
                    &serde_json::json!({"constraint": supplement}).to_string()).await;
            }
            SupplementaryInputAction::PrioritizeStep => {
                info!("补充输入: 指定优先步骤");
                self.event_bus.emit(task_iri, "STEP_PRIORITIZED", "SA",
                    &serde_json::json!({"priority": supplement}).to_string()).await;
            }
            SupplementaryInputAction::PauseExecution => {
                warn!("补充输入: 暂停执行");
                if let Some(cycle) = self.active_cycles.values_mut()
                    .find(|c| c.task_iri == task_iri)
                {
                    cycle.phase = CyclePhase::Idle;
                    cycle.phase_history.push(format!("Paused by user: {}", supplement));
                }
                self.event_bus.emit(task_iri, "EXECUTION_PAUSED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
            SupplementaryInputAction::ResumeExecution => {
                info!("补充输入: 恢复执行");
                if let Some(cycle) = self.active_cycles.values_mut()
                    .find(|c| c.task_iri == task_iri)
                {
                    cycle.phase = CyclePhase::Executing;
                    cycle.phase_history.push(format!("Resumed by user: {}", supplement));
                }
                self.event_bus.emit(task_iri, "EXECUTION_RESUMED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
            SupplementaryInputAction::SkipCurrentStep => {
                info!("补充输入: 跳过当前步骤");
                self.event_bus.emit(task_iri, "STEP_SKIPPED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
            SupplementaryInputAction::AbortCurrentStep => {
                warn!("补充输入: 中止当前步骤");
                self.event_bus.emit(task_iri, "STEP_ABORTED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
        }
        Ok(())
    }

    /// 将补充内容注入到当前 Agent 的上下文中
    async fn inject_to_current_agent(&self, task_iri: &str, supplement: &str) {
        info!(task_iri = %task_iri, "注入补充上下文到当前 Agent");
        self.event_bus.emit(task_iri, "SUPPLEMENTARY_CONTEXT", "SA",
            &serde_json::json!({
                "supplement": supplement,
                "task_iri": task_iri,
            }).to_string()).await;
    }

    /// Emit a THOUGHT event from the SA so the TUI can display what the
    /// Supervisor Agent is doing (planning, classifying, evaluating, …).
    async fn emit_sa_thought(&self, task_iri: &str, thought: &str, action: &str) {
        let event = ExecutionEvent {
            event_id: format!("evt_{}", uuid::Uuid::new_v4().hyphenated()),
            task_iri: task_iri.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            event: ExecutionEventKind::Thought(Thought {
                agent_id: "SA".into(),
                thought: thought.to_string(),
                action: action.to_string(),
                emphasis: Vec::new(),
            }),
        };
        let _ = self.event_bus.emit(
            task_iri,
            "THOUGHT",
            "SA",
            &serde_json::to_string(&event).unwrap_or_default(),
        ).await;
    }

    #[instrument(skip(self, user_input), fields(task_iri = %task_iri))]
    pub async fn process_task(
        &mut self,
        user_input: &str,
        task_iri: &str,
    ) -> Result<TaskResult, CoreError> {
        // F1 多租户接线：从 blackboard 读取 HTTP 层写入的 user_id/tenant_id，
        // 注入 TaskContext，使身份血缘从 HTTP 入口贯穿 PA→DA→CA 全链路 session。
        let (identity_user, identity_tenant) = self.read_task_identity(task_iri);
        let ctx = TaskContext::new(task_iri, user_input, self.max_iterations)
            .with_identity(identity_user, identity_tenant);
        self.process_task_with_context(user_input, task_iri, ctx).await
    }

    /// 从 blackboard 中的任务节点提取 HTTP 层写入的 user_id 与 tenant_id。
    /// 返回 `(user_id, tenant_id)`，字段缺失时为 `None`。
    fn read_task_identity(&self, task_iri: &str) -> (Option<String>, Option<String>) {
        let Some(ref bb) = self.blackboard else {
            return (None, None);
        };
        let Ok(Some(node)) = bb.read_node(task_iri) else {
            return (None, None);
        };
        let v: serde_json::Value = serde_json::from_str(&node.json_ld).unwrap_or_default();
        let user_id = v.get("user_id").and_then(|s| s.as_str()).map(String::from);
        let tenant_id = v.get("tenant_id").and_then(|s| s.as_str()).map(String::from);
        (user_id, tenant_id)
    }

    /// 带自定义 TaskContext 的任务处理，支持 resume 模式
    #[instrument(skip(self, user_input, ctx), fields(task_iri = %task_iri))]
    pub async fn process_task_with_context(
        &mut self,
        user_input: &str,
        task_iri: &str,
        ctx: TaskContext,
    ) -> Result<TaskResult, CoreError> {
        let cycle_id = self.start_cycle(user_input, task_iri).await?;

        let mut five_w2h = self.extract_5w2h_from_input(user_input).await;
        let task_id = task_iri.strip_prefix("iri://task/").unwrap_or_else(|| task_iri.strip_prefix("iri://").unwrap_or(task_iri));
        let five_w2h_iri = format!("iri://task/{}/5w2h", task_id);

        // A3: 从 5W2H 计算 task_embedding → 设置到 relevance_tracker
        if let Some(ref embedder) = self.embedder {
            let task_text = format!("{}\n{}", five_w2h.what, five_w2h.why.description);
            if let Ok(task_emb) = embedder.embed(&task_text).await {
                self.relevance_tracker.set_task_context(task_emb);
            }
        }

        // 注入当前工作目录作为执行环境，使 LLM 知道在哪里创建文件
        if five_w2h.where_.as_ref().and_then(|w| w.execution_environment.as_ref()).is_none() {
            if let Ok(cwd) = std::env::current_dir() {
                let cwd_str = cwd.to_string_lossy().to_string();
                five_w2h = five_w2h.with_where(crate::core::five_w2h::WhereDetail {
                    data_sources: vec![],
                    execution_environment: Some(cwd_str),
                    target_repository: None,
                    target_branch: None,
                });
            }
        }
        if let Ok(json_ld) = five_w2h.to_json_ld(task_iri) {
            let _ = self.runner.l0_store.store(&five_w2h_iri, &json_ld.to_string());
            let cfg = crate::CoreConfig::default();
            if let Some(ref bb) = self.blackboard {
                if bb.write_node(&five_w2h_iri, &json_ld.to_string(), &cfg).is_ok() {
                    tracing::debug!(five_w2h_iri = %five_w2h_iri, "5W2H 写入黑板");
                    let route = self.type_router.get_route("task:5W2H");
                    if let Some(route) = route {
                        for event in &route.events {
                            let _ = self.event_bus.emit(task_iri, event, "system:sa", &five_w2h_iri).await;
                        }
                    }
                }
            }
            tracing::info!(task_iri = %task_iri, what = %five_w2h.what, "5W2H 初始化完成");
        }

        let perception_hints = self.perception.on_task_start(user_input, task_iri)
            .map(|a| a.relevant_experience_hints)
            .unwrap_or_default();

        // 统一执行路径：从 JSON-LD 工作流或 LLM 构建 ExecutionPlan
        let plan = if let Some(ref wf_jsonld) = ctx.workflow_jsonld {
            info!(task_iri = %task_iri, "使用 JSON-LD 工作流模式 — 通过 adapter 转换为 ExecutionPlan");
            // 外部工作流：通过 DAG → ExecutionPlan 统一路径
            let def = crate::core::workflow::loader::load_workflow_jsonld(wf_jsonld)
                .map_err(|e| CoreError::Internal { message: format!("工作流解析失败: {}", e) })?;
            let dag = crate::core::workflow::loader::build_dag(&def)
                .map_err(|e| CoreError::Internal { message: format!("DAG 构建失败: {}", e) })?;
            let mut plan = crate::core::workflow::adapter::dag_to_execution_plan(&dag, &def, task_iri);
            plan.dag_jsonld = Some(wf_jsonld.clone());
            plan
        } else if ctx.resumed_messages.is_some() {
            // Resume 模式：跳过完整分析，创建从中断阶段继续的计划
            self.build_resume_plan()
        } else {
            self.analyze_task_with_llm(user_input, &five_w2h, &perception_hints).await
        };

        // Let the UI know what the SA decided
        let step_roles: Vec<String> = plan.steps.iter().map(|s| format!("{:?}", s.role)).collect();
        self.emit_sa_thought(task_iri,
            &format!("Task classified. Plan: {} ({} steps: {})",
                plan.description, plan.steps.len(), step_roles.join(" → ")),
            "plan_created").await;

        if let Some(cycle) = self.active_cycles.get_mut(&cycle_id) {
            cycle.phase = CyclePhase::Executing;
            cycle.phase_history.push(format!("Plan: {}", plan.description));
        }

        let mut pending_interventions: Vec<crate::perception::proactive_engine::InterventionPlan> = Vec::new();
        if let Some(ref mut receiver) = self.event_receiver {
            while let Ok(event) = receiver.try_recv() {
                if event.task_iri != task_iri {
                    continue;
                }
                match event.event_type.as_str() {
                    "INTERVENTION_REQUIRED" => {
                        if let Ok(plan) = serde_json::from_str::<crate::perception::proactive_engine::InterventionPlan>(&event.payload) {
                            pending_interventions.push(plan);
                        }
                    }
                    "DEADLINE_APPROACHING" => {
                        warn!("截止时间临近，标记任务紧急");
                    }
                    "HUMAN_APPROVAL_RESULT" => {
                        if let Ok(result) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let request_id = result.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
                            let approved = result.get("approved").and_then(|v| v.as_bool()).unwrap_or(false);
                            if !request_id.is_empty() {
                                self.pending_approvals.lock().await.insert(request_id.to_string(), approved);
                                info!(request_id = %request_id, approved = %approved, "收到人工确认结果");
                            }
                        }
                    }
                    "AGENT_ERROR" => {
                        let plan = self.perception.on_agent_blocked(&event.source_agent_iri, task_iri);
                        if plan.should_interrupt {
                            pending_interventions.push(plan);
                        }
                    }
                    "THRESHOLD_EXCEEDED" => {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let plan = self.perception.on_quality_degradation(&payload, task_iri);
                            if plan.should_interrupt {
                                pending_interventions.push(plan);
                            }
                        }
                    }
                    "CYCLE_ITERATION" => {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let plan = self.perception.on_progress_anomaly(&payload, task_iri);
                            if plan.should_interrupt {
                                pending_interventions.push(plan);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for plan in pending_interventions {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.execute_intervention(plan, task_iri),
            ).await;
        }

        let result = self.execute_plan(plan, task_iri, user_input, five_w2h, &five_w2h_iri, ctx.resumed_messages).await?;

        if let Some(scheduler) = &self.scheduler {
            let _ = scheduler.on_task_complete(task_iri).await;
        }

        Ok(result)
    }

    pub fn get_cycle_status(&self, cycle_id: &str) -> Option<&CycleState> {
        self.active_cycles.get(cycle_id)
    }

    pub fn active_cycles(&self) -> Vec<&CycleState> {
        self.active_cycles.values().collect()
    }

    pub fn cleanup_expired_cycles(&mut self, max_age_secs: i64) {
        let now = chrono::Utc::now();
        self.active_cycles.retain(|_, cycle| {
            now.signed_duration_since(cycle.started_at).num_seconds() < max_age_secs
                || !cycle.task_completed
        });
    }

    /// Try to read L1 session count from the memory manager using its atomic
    /// counter — does not block if the memory_manager lock is contended.
    pub fn try_l1_session_count(&self) -> Option<u64> {
        self.runner
            .memory_manager
            .try_lock()
            .ok()
            .map(|mm| mm.l1_session_count())
    }

    /// Returns the atomic token counters from the agent runner.
    /// Returns (total_prompt, total_completion, last_prompt, last_completion).
    pub fn token_usage_arcs(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>, Arc<AtomicU64>, Arc<AtomicU64>) {
        (
            self.runner.total_prompt_tokens.clone(),
            self.runner.total_completion_tokens.clone(),
            self.runner.last_prompt_tokens.clone(),
            self.runner.last_completion_tokens.clone(),
        )
    }

    /// Query L1 session count and L3 projection cache count from the memory manager.
    pub fn memory_stats(&self) -> (usize, usize) {
        let mm = self.runner.memory_manager.blocking_lock();
        let l1 = mm.session_count();
        let l3 = mm.projection().cache_stats().total_views;
        (l1, l3)
    }

    fn query_historical_5w2h(&self, limit: usize) -> Vec<(String, crate::core::five_w2h::Task5W2H)> {
        let mut results = Vec::new();
        let tags = vec!["5w2h".to_string(), "frozen".to_string()];
        if let Ok(entries) = self.runner.l0_store.search_by_tags(&tags) {
            for entry in entries.into_iter().take(limit) {
                if let Ok(node) = serde_json::from_str::<serde_json::Value>(&entry.content) {
                    if let Ok(w2h) = crate::core::five_w2h::Task5W2H::from_json_ld(&node) {
                        if w2h.frozen {
                            results.push((entry.iri.clone(), w2h));
                        }
                    }
                }
            }
        }
        results
    }

    fn match_similar_tasks(
        &self,
        current_what: &str,
        current_why: &str,
        historical: &[(String, crate::core::five_w2h::Task5W2H)],
        top_k: usize,
    ) -> Vec<(String, crate::core::five_w2h::Task5W2H, f32)> {
        let mut scored: Vec<_> = historical
            .iter()
            .map(|(iri, w2h)| {
                let what_sim = Self::text_similarity(&w2h.what, current_what);
                let why_sim = Self::text_similarity(&w2h.why.description, current_why);
                let combined = what_sim * 0.6 + why_sim * 0.4;
                (iri.clone(), w2h.clone(), combined)
            })
            .collect();
        
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_k).collect()
    }

    fn text_similarity(a: &str, b: &str) -> f32 {
        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();
        
        let a_words: std::collections::HashSet<&str> = a_lower.split_whitespace().collect();
        let b_words: std::collections::HashSet<&str> = b_lower.split_whitespace().collect();
        
        if a_words.is_empty() || b_words.is_empty() {
            return 0.0;
        }
        
        let intersection = a_words.intersection(&b_words).count();
        let union = a_words.union(&b_words).count();
        
        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    fn format_historical_experience(
        &self,
        similar: &[(String, crate::core::five_w2h::Task5W2H, f32)],
    ) -> String {
        if similar.is_empty() {
            return String::new();
        }

        let mut experience_section = String::from("\n## 📚 历史经验参考（相似任务）\n");
        experience_section.push_str("以下历史任务与当前任务相似，仅供参考：\n\n");

        for (i, (iri, w2h, score)) in similar.iter().enumerate() {
            experience_section.push_str(&format!(
                "### 相似任务 {} (相似度: {:.0}%)\n",
                i + 1,
                score * 100.0
            ));
            experience_section.push_str(&format!("- **What**: {}\n", w2h.what));
            experience_section.push_str(&format!("- **Why**: {}\n", w2h.why.description));
            if let Some(ref how) = w2h.how {
                if let Some(ref steps) = how.required_steps {
                    experience_section.push_str(&format!("- **执行步骤**: {}\n", steps));
                }
            }
            experience_section.push_str(&format!("- **来源**: {}\n\n", iri));
        }

        experience_section.push_str("**注意**: 历史经验仅供参考，请根据当前任务实际情况调整。\n");
        experience_section
    }

}

mod actions;

use self::actions::{get_action_handler, parse_or_repair_json};

#[cfg(test)]
mod tests;
