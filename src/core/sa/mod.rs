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
use crate::memory::l2_blackboard::{Blackboard, QueryFilter};
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::memory::EmbeddingService;
use crate::perception::proactive_engine::ProactiveEngine;
use crate::templates::template_engine::TemplateEngine;
use crate::tools::sharing::{SharingProtocol, ShareType, Permission};
use crate::tools::skill_registry::SkillRegistry;
use crate::CoreError;

/// 5 categories, 16 predefined intervention actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterventionAction {
    // === 1. Normal Continuation ===
    Continue,
    ContinueWithMonitor,

    // === 2. Parameter Tuning ===
    IncreaseRetry { additional_retries: u32 },
    IncreaseTimeout { additional_seconds: u64 },
    ReduceComplexity,
    RestrictTools { allowed_tools: Vec<String> },

    // === 3. Execution Flow Adjustment ===
    SkipStep { step_id: String },
    RetryStep { step_id: String },
    Parallelize,
    SplitStep { step_id: String, sub_steps: Vec<String> },
    InsertExtraStep { description: String },

    // === 4. Resource & Mode Switch ===
    FallbackToShallow,
    EmergencyMode,
    IncreaseBudget { additional_tokens: u64, additional_time_secs: u64 },
    FreezeAndReport,

    // === 5. Termination & Escalation ===
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

/// Action parameters (structured parameters from LLM output)
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

/// Intermediate structure for LLM classification decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmActionDecision {
    action: String,
    #[serde(default)]
    params: ActionParams,
    reasoning: Option<String>,
}

/// 4 categories, 12 predefined supplementary input actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupplementaryInputAction {
    // === 1. Information Supplement ===
    AddContext,
    RefineObjective,
    ProvideConstraint,

    // === 2. Direction Guidance ===
    GuideDirection,
    PrioritizeStep,
    SuggestApproach,

    // === 3. Execution Control ===
    PauseExecution,
    ResumeExecution,
    SkipCurrentStep,

    // === 4. Feedback & Correction ===
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

/// Intermediate structure for LLM classification decisions (supplementary input)
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

/// Execution result of a human approval node
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
    /// Original JSON-LD DAG definition (set when loading from --workflow file)
    /// Used in execute_plan() to preserve DAG features (conditional branching, retry, parallelism)
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
    /// Maximum PDCA cycle re-entry count (for Recursive tasks, default 7)
    max_pdca_cycles: u32,
    perception: ProactiveEngine,
    sharing: Arc<SharingProtocol>,
    blackboard: Option<Arc<Blackboard>>,
    prefetch_engine: Option<Arc<PrefetchEngine>>,
    scheduler: Option<Arc<MemoryScheduler>>,
    type_router: TypeRouter,
    pending_approvals: Arc<tokio::sync::Mutex<HashMap<String, bool>>>,
    supplementary_inputs: HashMap<String, Vec<(String, String)>>,
    /// Supplementary input shared store, written by SA and consumed by AgentRunner at CycleStart
    supplement_store: SupplementaryInputStore,
    /// Embedding service (optional, for computing supplementary input embeddings and relevance scores)
    embedder: Option<Arc<dyn EmbeddingService>>,
    /// Relevance tracker
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
        // Create supplementary input shared store and inject into AgentRunner (ensure SA and Runner share the same instance)
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

    /// Set embedding service (for computing supplementary input embeddings and relevance scores)
    /// Also propagates to AgentRunner so it can compute turn embeddings in the ReAct loop
    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingService>) -> Self {
        self.embedder = Some(embedder.clone());
        // Propagate to AgentRunner
        if let Some(runner) = Arc::get_mut(&mut self.runner) {
            *runner = runner.clone().with_embedder(embedder);
        }
        self
    }

    /// Get supplementary input shared store (for AgentRunner injection)
    pub fn supplement_store(&self) -> SupplementaryInputStore {
        self.supplement_store.clone()
    }

    /// Update default model in gateway (uses RwLock interior mutability, no &mut self needed)
    /// This avoids redb file lock conflicts from rebuilding the entire Engine/L0Store
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
            "Perception analysis complete"
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
                // Recursive: only 1 round of SA-level PDCA.
                // DA internally executes micro-recursion via execute_recursive_sub_cycle
                // Sub-task decomposition, no SA-level multi-round replay needed.
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
            success_metrics: vec!["Task completed".to_string()],
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
                // Recursive: only 1 round of SA-level PDCA (same as Standard).
                // DA internally executes micro-recursion via execute_recursive_sub_cycle
                // Sub-task decomposition, no SA-level multi-round replay needed.
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
            success_metrics: vec!["Task completed".to_string()],
            max_recursion_depth,
            sub_tasks: vec![],
            dag_jsonld: None,
        }
    }

    /// Build resume mode execution plan: standard PDCA sequence
    /// execute_plan will skip completed phases based on resumed_messages
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
            success_metrics: vec!["Task completed".to_string()],
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
                        "Analyze task requirements, create detailed execution plan".to_string(),
                        "JSON-formatted plan with steps, dependencies, resource requirements".to_string(),
                        "Plan is clear, steps complete, dependencies explicit".to_string(),
                    ),
                    AgentRole::Do => (
                        "Execute the task according to the plan".to_string(),
                        "Execution results, generated files or data".to_string(),
                        "Task completed per plan, output matches expectations".to_string(),
                    ),
                    AgentRole::Check => (
                        "Verify the quality and correctness of execution results".to_string(),
                        "Inspection report with issue list and recommendations".to_string(),
                        "Verification passed or issues identified".to_string(),
                    ),
                    AgentRole::Act => (
                        "Summarize results and make final decision".to_string(),
                        "Final decision and summary report".to_string(),
                        "Decision clear, summary complete".to_string(),
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
            let mut w2h = Task5W2H::new(user_input, "User task");
            w2h.why.priority = Priority::Low;
            return w2h;
        }

        let prompt = format!(
            r#"Analyze the user task below and extract the minimal 5W2H metadata set (What + Why).

User task: {}

Output in JSON format:
{{
  "what": "Core description of the task goal (one sentence)",
  "why_description": "Task intent/value description",
  "success_criteria": ["verifiable condition 1", "condition 2"],
  "priority": "high|medium|low",
  "deadline": "ISO8601 deadline (optional)",
  "required_role": "Plan|Do|Check|Act (optional)"
}}

Output only JSON, no other content."#,
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
                        let why_desc = parsed.get("why_description").and_then(|v| v.as_str()).unwrap_or("User task").to_string();
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
                tracing::warn!("5W2H extraction failed: {}, using defaults", e);
            }
        }

        Task5W2H::new(user_input, "User task")
    }

    pub async fn analyze_task_with_llm(&self, user_input: &str, five_w2h: &crate::core::five_w2h::Task5W2H, experience_hints: &[String]) -> ExecutionPlan {
        // First detect recursive/complex tasks via keyword classifier (keyword path captures Recursive more reliably than LLM)
        let keyword_complexity = self.classify_complexity(user_input);
        if keyword_complexity == TaskComplexity::Recursive {
            info!("Keyword classification: Recursive, skipping LLM plan generation, using keyword path for cyclic plan");
            return self.build_plan_from_complexity(TaskComplexity::Recursive);
        }

        let enhanced_input = if experience_hints.is_empty() {
            user_input.to_string()
        } else {
            format!("## Historical Experience Reference\n{}\n\n## Current Task\n{}",
                experience_hints.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n"),
                user_input
            )
        };

        let complexity = match five_w2h.why.priority {
            crate::core::five_w2h::Priority::High => {
                // Use Complex if keyword classifier says Complex, otherwise High→Complex
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
                info!(plan_id = %plan.plan_id, steps = plan.steps.len(), "LLM generated detailed plan successfully");
                // If keyword classifier says Complex but LLM returned wrong complexity, fix it
                if keyword_complexity == TaskComplexity::Complex && plan.task_complexity != TaskComplexity::Complex {
                    plan.task_complexity = TaskComplexity::Complex;
                    plan.max_recursion_depth = 2;
                }
                return plan;
            }
            Err(e) => {
                warn!("LLM failed to generate detailed plan: {}, using default plan", e);
            }
        }

        self.build_plan_from_complexity(complexity)
    }

    async fn generate_detailed_plan_with_llm(&self, user_input: &str, five_w2h: &crate::core::five_w2h::Task5W2H) -> Result<ExecutionPlan, CoreError> {
        let mut w2h_section = String::new();
        if let Some(ref when) = five_w2h.when {
            if let Some(ref deadline) = when.deadline {
                w2h_section.push_str(&format!("\n- Deadline: {}", deadline.to_rfc3339()));
            }
        }
        if let Some(ref how_much) = five_w2h.how_much {
            if let Some(budget) = how_much.token_budget {
                w2h_section.push_str(&format!("\n- Token Budget: {}", budget));
            }
            if let Some(cycles) = how_much.max_pdca_cycles {
                w2h_section.push_str(&format!("\n- Max PDCA Cycles: {}", cycles));
            }
        }
        if !five_w2h.why.success_criteria.is_empty() {
            w2h_section.push_str(&format!("\n- Success Criteria: {}", five_w2h.why.success_criteria.join(", ")));
        }

        let w2h_block = if w2h_section.is_empty() {
            String::new()
        } else {
            format!("\n## 5W2H Constraint Info{}", w2h_section)
        };

        let sa_constitution_prompt = {
            use crate::core::constitution::{ConstitutionRegistry, ConstitutionRole};
            let registry = ConstitutionRegistry::new();
            let constitution_text = registry.build_prompt_for_role_exact(ConstitutionRole::Supervisor);
            // Inject methodology layer discipline (includes auto-trigger protocol, always-active methodology)
            let methodology_text = crate::methodology::integration::MethodologyPromptInjector::build_for_sa();
            format!("{}\n{}", constitution_text, methodology_text)
        };

        let prompt = format!(
            r#"You are a task planning expert. Analyze the following task and generate a concise and efficient execution plan.

## Task Description
{}{}
## Output Requirements
Output the plan in JSON format with the following fields:

```json
{{
  "complexity": "simple|standard|complex|exploratory|emergency",
  "description": "Task description",
  "steps": [
    {{
      "step_id": "step_1",
      "role": "Plan|Do|Check|Act",
      "objective": "Specific goal of this step",
      "expected_output": "Expected output",
      "dependencies": [],
      "tools_allowed": ["file_read", "file_write", "grep_search", "glob_search", "web_search", "web_fetch", "bash"],
      "success_criteria": "Success criteria"
    }}
  ],
  "success_metrics": ["Success metric 1", "Success metric 2"]
}}
```

## Role Descriptions
- **Plan (PA)**: Analyze tasks, create plans, decompose sub-tasks
- **Do (DA)**: Execute specific tasks, create artifacts (one DA step should complete multiple related operations)
- **Check (CA)**: Verify results, check quality
- **Act (AA)**: Summarize decisions, final summary

## Complexity Definitions
- **simple**: Simple query, single step (DA only)
- **standard**: Standard task, requires PA→DA→CA→AA flow
- **complex**: Complex task, requires full PA→DA→CA→AA validation, DA internally triggers sub-cycle optimization
- **exploratory**: Exploratory task, requires multiple parallel DAs
- **emergency**: Emergency fix, skip PA, DA→CA→AA

## Important Constraints
1. **Step count limit**: Total steps not to exceed 6 (including PA and CA/AA)
2. **DA step merging**: Merge multiple related operations into one DA step. For example, creating multiple files should be done in one DA step, not one step per file
3. **Recommended pattern**: PA(1step) → DA(1-3steps) → CA(1step) → AA(1step)
4. Each DA step's objective should describe a group of related operations to complete, not a single atomic operation

## Code of Conduct
As the Supervisor Agent, you must follow these guidelines:

{}

Output only JSON, no other content."#,
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
            warn!("Plan step count {} exceeds limit {}, truncating to first {} steps", steps.len(), max_plan_steps, max_plan_steps);
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
            r#"Analyze the complexity of the following task, return JSON format result.

Task: {}

Analyze the task:
1. Does it require multi-step execution?
2. Does it require a planning phase?
3. Does it require a verification phase?
4. Does it require multiple parallel explorations?

Return JSON:
{{"complexity": "simple|standard|complex|exploratory|emergency", "reason": "Brief reason"}}

Complexity definitions:
- simple: Simple query, single step
- standard: Standard task, requires plan→execute→check→decide flow
- complex: Complex task, requires multi-step execution and verification
- exploratory: Exploratory task, requires multiple parallel explorations
- emergency: Emergency fix task, skip planning and execute directly"#,
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

        // Parse LLM response
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(complexity_str) = parsed.get("complexity").and_then(|c| c.as_str()) {
                let complexity = match complexity_str {
                    "simple" => TaskComplexity::Simple,
                    "complex" => TaskComplexity::Complex,
                    "exploratory" => TaskComplexity::Exploratory,
                    "emergency" => TaskComplexity::Emergency,
                    _ => TaskComplexity::Standard,
                };
                info!(complexity = ?complexity, reason = ?parsed.get("reason"), "LLM classification result");
                return Ok(complexity);
            }
        }

        // Try to extract from text
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

        // Instant: very short input (e.g., greetings)
        if user_input.len() < 15 && !user_input.contains(' ') {
            return TaskComplexity::Instant;
        }

        // Emergency: emergency fix category
        let emergency_keywords = ["fix", "bug", "error", "crash", "urgent", "broken", "repair",
            "repair", "urgent", "crash", "fault"];
        if emergency_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Emergency;
        }

        // Recursive decomposition: complex multi-step tasks, requires DA internal micro PDCA sub-cycles
        let recursive_keywords = [
            "refactor", "rewrite", "migrate",
            "split into", "decompose",
            "multi-phase",
            "end-to-end", "full-stack",
            // Project building category
            "develop", "create",
            "implement", "building", "build",
            "program", "project", "app",
            "website", "system", "platform",
            "generate",
        ];
        if recursive_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Recursive;
        }

        // Exploratory task → Exploratory (prioritized over research_patterns)
        let exploratory_keywords = [
            "research", "explore", "investigate",
        ];
        if exploratory_keywords.iter().any(|k| lower.contains(k)) {
            return TaskComplexity::Exploratory;
        }

        let compare_keywords = [
            "compare",
        ];
        if compare_keywords.iter().any(|k| lower.contains(k)) {
            let multi_patterns = ["different", "various", "multiple", "several"];
            if multi_patterns.iter().any(|p| lower.contains(p)) {
                return TaskComplexity::Exploratory;
            }
            return TaskComplexity::Complex;
        }

        // Research/analysis questions → Standard or Complex
        let research_patterns: [&str; 0] = [];
        if research_patterns.iter().any(|p| lower.contains(p)) {
            let deep_patterns = ["deep", "thorough", "comprehensive", "systematic", "in-depth"];
            if deep_patterns.iter().any(|p| lower.contains(p)) {
                return TaskComplexity::Complex;
            }
            return TaskComplexity::Standard;
        }

        // Simple: simple fact query, answerable in one sentence
        let simple_query_patterns: [&str; 0] = [];
        let is_simple_query = user_input.len() < 50 
            && simple_query_patterns.iter().any(|p| lower.contains(p))
            && !lower.contains("application")
            && !lower.contains("scenario")
            && !lower.contains("analysis")
            && !lower.contains("implementation")
            && !lower.contains("design");
        
        if is_simple_query {
            return TaskComplexity::Simple;
        }

        // English simple query
        if user_input.len() < 50
            && (lower.starts_with("what is") 
                || lower.starts_with("who is")
                || lower.starts_with("where is")
                || lower.starts_with("when is"))
        {
            return TaskComplexity::Simple;
        }

        // Default: Standard
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

        // Query context from L2 blackboard (replaces prev_summary)
        // Use query_nodes_filtered for role/cycle-aware context (AA uses prev_summary)
        let prev_agent_summary = context.prev_agent_summary.clone();
        let prev_summary = if role == AgentRole::Act {
            prev_agent_summary.clone()
        } else if let Some(blackboard) = &self.blackboard {
            let prev_role = match role {
                AgentRole::Do => Some(AgentRole::Plan),
                AgentRole::Check => Some(AgentRole::Do),
                _ => None,
            };
            // Only apply cycle_id filter when we have a specific role filter
            // (PA sees all context nodes; DA/CA see only the previous agent's output from this cycle)
            let filter = QueryFilter {
                role: prev_role.clone(),
                cycle_id: prev_role.map(|_| cycle_id.to_string()),
                node_type: None,
            };
            let nodes = blackboard.query_nodes_filtered(&context.task_iri, &filter).unwrap_or_default();
            if !nodes.is_empty() {
                let mut contents: Vec<String> = Vec::new();
                let mut summaries: Vec<String> = Vec::new();
                for n in nodes.iter() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&n.json_ld) {
                        let role = parsed.get("role").and_then(|v| v.as_str()).unwrap_or("");
                        let prefix = if !role.is_empty() { format!("[{}] ", role) } else { String::new() };
                        // Prefer content field (full LLM output)
                        if let Some(content) = parsed.get("content").and_then(|s| s.as_str()) {
                            let trimmed = content.trim();
                            if !trimmed.is_empty() && trimmed.len() > 20 {
                                contents.push(format!("{}{}", prefix, trimmed));
                                continue;
                            }
                        }
                        // Fallback to summary field
                        if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                            let trimmed = summary.trim();
                            if !trimmed.is_empty() {
                                summaries.push(format!("{}{}", prefix, trimmed));
                            }
                        }
                    }
                }
                // Prefer content with substance
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

        // Execute with independent BizAgent instance (agent isolation)
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
        initial_prev_summary: Option<String>,
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
        let mut prev_summary: Option<String> = initial_prev_summary;

        // Resume mode: determine which phase to start from
        // Load latest checkpoint from L0 to resolve phase tags
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

        // Resume mode: prefer restoring from checkpoint's prev_summary field
        // If no prev_summary in checkpoint, extract PA output from history messages
        let resume_prev_summary: Option<String> = if resumed_messages.is_some() {
            // Try to read saved prev_summary from L0 checkpoint
            let cm = crate::core::checkpoint::CheckpointManager::with_persistence(self.runner.l0_store.clone());
            let from_cp: Option<String> = cm.restore_latest(task_iri).ok().flatten()
                .and_then(|cp| cp.prev_summary);
            if from_cp.is_some() {
                from_cp
            } else {
                // Fallback: extract PA phase output from history messages as prev_summary
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

        // --- Unified DAG execution path ---
        // Convert ExecutionPlan to DAG (LLM path adapter) or use external JSON-LD DAG directly (--workflow path)
        let dag = if let Some(ref dag_jsonld) = plan.dag_jsonld {
            let def = crate::core::workflow::loader::load_workflow_jsonld(dag_jsonld)
                .map_err(|e| CoreError::Internal { message: format!("Workflow parsing failed: {}", e) })?;
            crate::core::workflow::loader::build_dag(&def)
                .map_err(|e| CoreError::Internal { message: format!("DAG build failed: {}", e) })?
        } else {
            let wf = crate::core::workflow::adapter::plan_to_workflow(&plan, task_iri);
            crate::core::workflow::loader::build_dag(&wf)
                .map_err(|e| CoreError::Internal { message: format!("DAG build failed: {}", e) })?
        };
        let order = crate::core::workflow::loader::topological_order(&dag)
            .map_err(|e| CoreError::Internal { message: format!("Topological sort failed: {}", e) })?;

        let mut completed_node_results: std::collections::HashMap<String, crate::core::workflow::NodeResult> = std::collections::HashMap::new();
        let mut skip_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Execute in DAG topological order
        for (i, &node_idx) in order.iter().enumerate() {
            let node_def = &dag.graph[node_idx].def;
            let step = crate::core::workflow::adapter::node_to_planstep(node_def);

            // Check if in cross-node skip set (branch jump from HumanApprovalNode)
            if skip_nodes.contains(&node_def.id) {
                info!(node_id = %node_def.id, "HumanApprovalNode branch jump: skipping this node");
                continue;
            }

            // Resume mode: skip completed phases
            if resume_skip_phases.contains(&step.role) {
                info!(role = ?step.role, "[resume] skipping completed phase");
                if prev_summary.is_none() {
                    prev_summary = resume_prev_summary.clone().or_else(|| Some("Restored from checkpoint, preceding phase completed.".to_string()));
                }
                continue;
            }

            // HumanApprovalNode: human approval node, does not dispatch agent
            if node_def.node_type == "HumanApprovalNode" {
                let approval = self.request_human_approval_general(
                    &node_def.approval_prompt, &node_def.id, task_iri
                ).await?;

                let status = if approval.approved { "approved" } else { "rejected" };
                let summary = format!("[HumanApproval] {}: {}",
                    if approval.approved { "Approved" } else { "Rejected" },
                    approval.comment.as_deref().unwrap_or(""));

                completed_node_results.insert(node_def.id.clone(), crate::core::workflow::NodeResult {
                    node_id: node_def.id.clone(),
                    status: status.to_string(),
                    summary: summary.clone(),
                    archive_iri: None,
                    turn_count: 0,
                    tool_call_count: 0,
                    error: if approval.approved { None } else { Some("User rejected".to_string()) },
                    output: None,
                    artifacts: vec![],
                });

                prev_summary = Some(format!("## Human Approval Result\n{}", summary));
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

                // Rejected and rejection jump set → skip intermediate nodes to target
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
                            // Target node not found, skip all remaining nodes
                            for skip_idx in (i + 1)..order.len() {
                                skip_nodes.insert(dag.graph[order[skip_idx]].def.id.clone());
                            }
                        }
                    }
                }

                // Approved and approval jump set → skip intermediate nodes
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

                info!(node_id = %node_def.id, status = %status, "HumanApprovalNode processing complete");
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
                format!("\n\n## Historical Experience\n{}", cycle_hints.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n"))
            };
            let objective = match (&prev_summary, step.role) {
                (Some(summary), AgentRole::Plan) => {
                    format!("{}\n\n## User Task\n{}{}\n\n## Feedback from Previous PDCA Cycle\n{}\n\nPlease create a detailed execution plan for the above user task, addressing all feedback from the previous cycle.", step.objective, user_input, hints_block, summary)
                }
                (Some(summary), AgentRole::Do) => {
                    format!("{}\n\nUpper PA's Plan:\n{}{}\n\nPlease execute the task according to the plan.", step.objective, summary, hints_block)
                }
                (Some(summary), AgentRole::Check) => {
                    format!("{}\n\nExecution Results:\n{}{}\n\nPlease verify whether the execution results are correct and complete.", step.objective, summary, hints_block)
                }
                (Some(summary), AgentRole::Act) => {
                    format!("{}\n\nCheck Conclusions:\n{}{}\n\nPlease make final decisions and summarize.", step.objective, summary, hints_block)
                }
                (None, AgentRole::Plan) => {
                    format!("{}\n\n## User Task\n{}{}\n\nPlease create a detailed execution plan for the above user task.", step.objective, user_input, hints_block)
                }
                _ => step.objective.clone(),
            };

            if step.role == AgentRole::Check {
                let missing = five_w2h.check_completeness(task_level);
                if !missing.is_empty() {
                    info!(missing_dims = ?missing, "5W2H completeness check: missing dimensions, filling defaults");
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
            .with_step_info(&step.expected_output, &step.success_criteria)
            .with_cycle_id(&cycle_id);

            context = context.with_five_w2h(five_w2h_iri, five_w2h.clone());

            // Resume mode: restore history messages on the first actually executed step
            // Note: in resume mode PA (i=0) is skipped, first executed is DA (i=1)
            let is_first_executed_step = if resume_skip_phases.is_empty() {
                i == 0
            } else {
                // First step after the skipped phases
                !resume_skip_phases.contains(&step.role) 
                    && plan.steps[..i].iter().all(|s| resume_skip_phases.contains(&s.role))
            };
            if is_first_executed_step {
                if let Some(ref msgs) = resumed_messages {
                    // Calculate turn/tool count from resumed_messages
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
                            // Use timeout to prevent intervention processing from blocking step scheduling
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                self.execute_intervention(intervention, task_iri),
                            ).await;
                        }
                    }
                }
            }

            // Check if execution is paused (via supplementary input action PauseExecution)
            let paused = self.active_cycles.get(&cycle_id)
                .map(|c| c.phase == CyclePhase::Idle)
                .unwrap_or(false);
            if paused {
                info!(step_id = %step.step_id, role = ?step.role, "Execution paused, waiting for resume");
                // Loop waiting for resume, checking supplementary inputs simultaneously
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
                            .map(|iri| format!(" | read_agent_output query: {}", iri))
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
                                        tracing::debug!(five_w2h_iri = %five_w2h_iri, "5W2H update synced to blackboard");
                                    }
                                }
                            }
                        }
                    }
                }

                if step.role == AgentRole::Act && result.status == "success" {
                    // Only freeze 5W2H on the last AA step (intermediate AA in multi-cycle PDCA does not freeze)
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
                            info!(task_iri = %task_iri, "5W2H frozen and archived");
                        }
                    } else {
                        info!(task_iri = %task_iri, step_id = %step.step_id, "Intermediate AA step: 5W2H not frozen yet");
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
                        info!(count = advisories.len(), "PA perception advisories generated");
                    }
                }

                if step.role == AgentRole::Check && result.status == "success" {
                    let check_data = serde_json::json!({
                        "summary": &result.summary,
                        "objective": &step.objective,
                    });
                    if let Some(advisory) = self.perception.on_check_completed(&check_data, task_iri) {
                        info!(advisory = ?advisory, "CA perception advisories generated");
                    }
                }

                // Multi-cycle PDCA early exit check: AA evaluation is the final review of a PDCA cycle,
                // whether pass or fail, terminate subsequent cycles to avoid duplicate execution.
                if step.role == AgentRole::Act {
                    let has_remaining = (i + 1) < order.len();
                    if has_remaining {
                        let reason = match result.status.as_str() {
                            "success" => "AA passed, task completed",
                            "failed" | "partial_success" => "AA did not pass",
                            _ => "AA evaluated",
                        };
                        info!(step_id = %step.step_id, status = %result.status, "{}, skipping remaining PDCA cycles", reason);
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
                                "{}\n\n## Sub-task Execution Results\n{}",
                                result.summary, sub_summary
                            );
                            prev_summary = Some(combined);
                        }
                        Err(e) => {
                            warn!(error = %e, "Recursive sub-cycle execution failed, using DA original result");
                            let prev = match result.archive_iri {
                                Some(ref iri) => format!("{}\n\nFor the full report, use read_agent_output tool to query: {}", result.summary, iri),
                                None => result.summary.clone(),
                            };
                            prev_summary = Some(prev);
                        }
                    }
                } else {
                    let prev = match result.archive_iri {
                        Some(ref iri) => format!("{}\n\nFor the full report, use read_agent_output tool to query: {}", result.summary, iri),
                        None => result.summary.clone(),
                    };
                    prev_summary = Some(prev);
                }

                last_result = Some(result);
            }

            if let Some(alert) = self.perception.check_5w2h_constraints(five_w2h_iri) {
                tracing::warn!(alert = %alert, "5W2H constraint alert");
                self.event_bus.emit(task_iri, &alert, "SA", &serde_json::json!({"task_iri": task_iri}).to_string()).await;
            }

            info!(step_id = %step.step_id, role = ?step.role, status = ?last_result.as_ref().map(|r| &r.status), "Step completed");

            // ── Step-level checkpoint: save full execution context ──
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

                // Capture session messages from blackboard AgentTurn nodes for checkpoint resume
                let session_msgs_json: String = if let Some(ref bb) = self.blackboard {
                    let filter = crate::memory::l2_blackboard::QueryFilter {
                        role: None,
                        cycle_id: Some(cycle_id.clone()),
                        node_type: Some("AgentTurn".to_string()),
                    };
                    let nodes = bb.query_nodes_filtered(task_iri, &filter).unwrap_or_default();
                    let msgs: Vec<serde_json::Value> = nodes.iter().filter_map(|n| {
                        let parsed: serde_json::Value = serde_json::from_str(&n.json_ld).ok()?;
                        Some(serde_json::json!({
                            "role": parsed.get("role").and_then(|r| r.as_str()).unwrap_or("assistant"),
                            "content": parsed.get("content").and_then(|c| c.as_str()).unwrap_or(""),
                            "summary": parsed.get("summary").and_then(|s| s.as_str()),
                        }))
                    }).collect();
                    serde_json::to_string(&msgs).unwrap_or_else(|_| "[]".to_string())
                } else {
                    "[]".to_string()
                };

                if let Err(e) = cm.create_ext(
                    task_iri,
                    &cp_name,
                    "[]",
                    &session_msgs_json,
                    &state_json,
                    &tags,
                    Some(&role_name),
                    None, // five_w2h_json — 5W2H already persisted via L0
                    prev_summary.as_deref(),
                    cycle_state.as_deref(),
                    completed_nodes.as_deref(),
                    pending_approvals.as_deref(),
                    supplement_data.as_deref(),
                    None,
                    None,
                    None,
                ) {
                    warn!("[checkpoint] step_complete save failed: {}", e);
                } else {
                    info!("[checkpoint] step_complete_{} saved", role_name);
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
            info!(depth = current_depth, max_depth, "Recursive depth limit reached, stopping sub-cycle");
            return Ok("Recursive depth limit reached".to_string());
        }

        self.emit_sa_thought(task_iri,
            &format!("▶ Recursive sub-cycle (depth {}/{})", current_depth, max_depth),
            "recursive_sub_cycle_start").await;

        let sub_task = SubTask::new(
            &format!("Decomposing sub-tasks from DA result (depth={})", current_depth),
            parent_step_id,
            current_depth,
        );

        info!(
            sub_task_id = %sub_task.sub_task_id,
            depth = current_depth,
            max_depth,
            "Starting recursive sub-cycle"
        );

        self.emit_sa_thought(task_iri,
            &format!("Decomposing DA result, identifying sub-tasks... (depth {}/{})", current_depth, max_depth),
            "recursive_decompose").await;

        let decompose_prompt = format!(
            r#"You are a task decomposition expert. Below is an execution result summary of a DA (Do Agent). Analyze whether there are sub-tasks that need further execution.

## DA Execution Result
{}

## Task Context
- Original goal: {}
- Current recursion depth: {}/{}

## Output Requirements
Output the list of sub-tasks that need further execution in JSON format. If no further sub-tasks are needed, return an empty array.

```json
{{
  "has_sub_tasks": true/false,
  "sub_tasks": [
    {{
      "objective": "Sub-task objective description",
      "role": "Do",
      "success_criteria": "Success criteria"
    }}
  ]
}}
```

## Evaluation Criteria
1. If the DA result explicitly mentions "still needs...", "next step needs...", etc., there are sub-tasks
2. If the DA result has fully completed the goal with no remaining work, there are no sub-tasks
3. Sub-tasks should be concrete and executable, not abstract
4. Maximum of 3 sub-tasks

Output only JSON."#,
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
        ).await.map_err(|e| CoreError::Internal { message: format!("Recursive decomposition LLM call failed: {}", e) })?;

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
            return Ok("LLM did not return valid decomposition result".to_string());
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
            .map_err(|e| CoreError::Internal { message: format!("Recursive decomposition JSON parse failed: {}", e) })?;

        if !parsed.has_sub_tasks || parsed.sub_tasks.is_empty() {
            info!(depth = current_depth, "No further decomposition needed for DA result");
            self.emit_sa_thought(task_iri,
                &format!("Sub-task decomposition complete: no further decomposition needed (depth {}/{})", current_depth, max_depth),
                "recursive_no_tasks").await;
            return Ok("No further decomposition needed".to_string());
        }

        self.emit_sa_thought(task_iri,
            &format!("Identified {} sub-tasks (depth {}/{})", parsed.sub_tasks.len(), current_depth, max_depth),
            "recursive_tasks_found").await;

        let mut sub_summaries = Vec::new();

        for (idx, sub_def) in parsed.sub_tasks.iter().enumerate() {
            let sub_objective = format!("[recursive depth={}] {}", current_depth, sub_def.objective);
            info!(sub_idx = idx, objective = %sub_def.objective, "Executing recursive sub-task");

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
                &format!("▶ Executing sub-task {}/{}: {} (depth {})", idx + 1, total, sub_def.objective, current_depth),
                "recursive_sub_task_start").await;

            let sub_result = self.dispatch_agent(AgentRole::Do, sub_ctx, cycle_id, Some(sub_step)).await?;

            self.emit_sa_thought(task_iri,
                &format!("{}/{} sub-task complete [{}]: {}", idx + 1, total,
                    sub_result.status, sub_def.objective),
                "recursive_sub_task_end").await;

            if sub_result.status == "success" || sub_result.status == "partial_success" {
                let icon = if sub_result.status == "success" { "✅" } else { "⚠️" };
                sub_summaries.push(format!("### Sub-task {} {}\n{}", idx + 1, icon, sub_result.summary));

                if current_depth < max_depth && sub_result.status == "success" {
                    // Only fully successful sub-tasks continue deeper recursion; partial_success continues in upper recursion
                    self.emit_sa_thought(task_iri,
                        &format!("Entering deeper recursion (depth {}/{})", current_depth + 1, max_depth),
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
                            sub_summaries.push(format!("#### Deep sub-task (depth={})\n{}", current_depth + 1, deeper_summary));
                        }
                        Err(e) => {
                            warn!(error = %e, "Deep recursive sub-cycle failed");
                        }
                    }
                }
            } else {
                sub_summaries.push(format!("### Sub-task {} ❌\nExecution failed: {}", idx + 1, sub_result.summary));
            }
        }

        self.emit_sa_thought(task_iri,
            &format!("Recursive sub-cycle complete (depth {}/{})", current_depth, max_depth),
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
            warn!(actions = ?plan.actions, "Non-interruptive intervention advice, logging only");
            return Ok(());
        }

        warn!(actions = ?plan.actions, "Executing intervention plan");

        // 1. LLM classification: map event to predefined action
        let (action, params) = self.analyze_anomaly_with_llm(&plan, task_iri).await
            .unwrap_or_else(|e| {
                warn!(error = %e, "LLM classification failed, falling back to ContinueWithMonitor");
                (InterventionAction::ContinueWithMonitor, ActionParams::default())
            });

        info!(action = ?action, "LLM classification result");

        // 2. IncreaseBudget special handling: needs human confirmation
        if matches!(action, InterventionAction::IncreaseBudget { .. }) {
            info!("IncreaseBudget needs human confirmation");
            let approved = self.request_human_approval(&action, task_iri).await?;
            if !approved {
                info!("IncreaseBudget not confirmed by human, downgraded to FreezeAndReport");
                let fallback_action = InterventionAction::FreezeAndReport;
                if let Some(handler) = get_action_handler(&fallback_action) {
                    return handler(self, ActionParams::default(), task_iri).await;
                }
                return Ok(());
            }
        }

        // 3. Registry dispatch: find and execute action handler
        let handler = get_action_handler(&action)
            .ok_or_else(|| CoreError::Internal {
                message: format!("Unknown action handler for: {:?}", action),
            })?;
        handler(self, params, task_iri).await?;

        // 4. Emit intervention execution event
        self.event_bus.emit(task_iri, "INTERVENTION_EXECUTED", "SA",
            &serde_json::json!({"action": format!("{:?}", action)}).to_string()).await;

        Ok(())
    }

    /// LLM classification: map intervention plan to predefined action
    async fn analyze_anomaly_with_llm(
        &self,
        plan: &crate::perception::proactive_engine::InterventionPlan,
        _task_iri: &str,
    ) -> Result<(InterventionAction, ActionParams), CoreError> {
        use crate::gateway::unified_gateway::ChatMessage;

        let prompt = format!(
            r#"You are an anomaly diagnosis expert. Based on the following intervention plan, select the most appropriate action from the predefined actions.

## Current Intervention Plan
- Diagnosis: {}
- Suggested action: {}
- Priority: {}
- Is interrupt: {}

## Predefined Action List (strictly select ONE most appropriate action)

### 1. Normal Continuation (no interrupt needed)
- Continue: Do nothing, continue execution
- ContinueWithMonitor: Continue execution but with enhanced monitoring

### 2. Parameter Tuning (no interrupt needed)
- IncreaseRetry: Increase retry count
- IncreaseTimeout: Increase timeout
- ReduceComplexity: Reduce complexity expectation
- RestrictTools: Restrict available tool set

### 3. Execution Flow Adjustment (interrupt needed)
- SkipStep: Skip current step
- RetryStep: Retry current step
- Parallelize: Parallelize execution
- SplitStep: Split into multiple sub-steps
- InsertExtraStep: Insert additional verification/fix steps

### 4. Resource & Mode Switch (interrupt needed)
- FallbackToShallow: Fallback to shallow mode
- EmergencyMode: Enter emergency mode
- FreezeAndReport: Freeze state and generate report

### 5. Termination & Escalation (interrupt needed)
- AbortTask: Abort current task
- NotifyHuman: Notify human intervention

## Output Requirements
Output only JSON with the following fields:
{{
  "action": "Selected action name",
  "params": {{ /* Action parameters */ }},
  "reasoning": "Reason for selecting this action"
}}

Notes:
1. Output only JSON, no extra content
2. action must be strictly selected from the above list
3. IncreaseBudget requires human confirmation, only select when resource budget is truly insufficient
4. AbortTask is the last resort, only use when unrecoverable"#,
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

    /// IncreaseBudget human confirmation flow
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
                        "Human confirmation needed: Increase Token budget by {} tokens, additional time {} seconds?",
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

        info!(request_id = %request_id, "Waiting for human confirmation");

        let iri = format!("iri://approval/{}", request_id);
        let _ = self.runner.l0_store.store(&iri, &details.to_string());

        // Non-blocking wait: register pending approval request
        // External systems return confirmation via EventBus HUMAN_APPROVAL_RESULT event
        // SA checks the event and updates pending_approvals in the process_task main loop
        self.pending_approvals.lock().await.insert(request_id.clone(), false);

        // Wait briefly for any instant approval result
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

        info!(request_id = %request_id, "Human confirmation wait timed out (5s), task continuing, waiting for async confirmation");
        Ok(false)
    }

    /// General human approval request (for HumanApprovalNode workflow nodes)
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

        info!(request_id = %request_id, node_id = %node_id, "HumanApprovalNode: waiting for human confirmation");

        let iri = format!("iri://approval/{}", request_id);
        let _ = self.runner.l0_store.store(&iri, &details.to_string());

        self.pending_approvals.lock().await.insert(request_id.clone(), false);

        // Wait briefly for any instant approval result
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

        info!(request_id = %request_id, "HumanApprovalNode: wait timed out (5s), task continuing, waiting for async confirmation");
        // After timeout, do not block by default — mark as unapproved, follow reject logic
        Ok(HumanApprovalNodeResult {
            node_id: node_id.to_string(),
            approved: false,
            comment: Some("Approval timeout, default rejected".to_string()),
        })
    }

    /// Enqueue user supplementary input, waiting for SA processing
    pub fn enqueue_supplementary_input(&mut self, task_iri: &str, content: &str) {
        self.supplementary_inputs
            .entry(task_iri.to_string())
            .or_default()
            .push((content.to_string(), "pending".to_string()));
        info!(task_iri = %task_iri, "User supplementary input enqueued");
    }

    /// Check and execute supplementary inputs between execute_plan steps
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

        // 2. Collect pending supplementary inputs (avoid borrow conflicts)
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

        // 3. Process supplementary inputs one by one
        for supplement in &pending {
            let context = format!("Current step: {:?} - {}", step_role, step_objective);
            match self.classify_supplementary_input_with_llm(supplement, &context).await {
                Ok((action, params)) => {
                    info!(action = ?action, "Supplementary input classification result");
                    self.execute_supplementary_action(action, params, task_iri, supplement).await?;
                }
                Err(e) => {
                    warn!(error = %e, supplement = %supplement, "Supplementary input classification failed, defaulting to context injection");
                    self.inject_to_current_agent(task_iri, supplement).await;
                }
            }
        }

        // 4. Mark as processed
        if let Some(input_list) = self.supplementary_inputs.get_mut(task_iri) {
            for item in input_list.iter_mut() {
                item.1 = "processed".to_string();
            }
        }

        Ok(())
    }

    /// LLM classification: map user supplementary input to predefined action
    async fn classify_supplementary_input_with_llm(
        &self,
        user_supplement: &str,
        task_context: &str,
    ) -> Result<(SupplementaryInputAction, ActionParams), CoreError> {
        use crate::gateway::unified_gateway::ChatMessage;

        let prompt = format!(
            r#"You are a task guidance expert. Based on the user's supplementary input, select the most appropriate action from the predefined actions.

## Current Task Context
{}

## User Supplementary Input
{}

## Predefined Action List (strictly select ONE)

### 1. Information Supplement
- AddContext: User provides additional context/information
- RefineObjective: User refines or adjusts goals
- ProvideConstraint: User provides new constraints, e.g., time limits

### 2. Direction Guidance
- GuideDirection: User indicates execution direction/priority
- PrioritizeStep: User specifies a step to prioritize
- SuggestApproach: User suggests a specific method or approach

### 3. Execution Control
- PauseExecution: User requests to pause current execution
- ResumeExecution: User requests to resume execution
- SkipCurrentStep: User requests to skip the current step

### 4. Feedback & Correction
- ConfirmDirection: User confirms the current direction is correct
- CorrectApproach: User points out errors and corrects direction
- AbortCurrentStep: User requests to abort the current step

## Output Requirements
Output only JSON with the following fields:
{{
  "action": "Selected action name",
  "params": {{ /* Action parameters, varies per action */ }},
  "reasoning": "Reason for selecting this action"
}}

Notes:
1. Output only JSON, no extra content
2. action must be strictly selected from the above list
3. If the user is supplementing information rather than giving instructions, select AddContext
4. Only select AbortCurrentStep or SkipCurrentStep if the user explicitly requests abort or skip"#,
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

    /// Execute supplementary input action
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
                // 1. Calculate embedding and relevance_score
                let embedding = if let Some(ref embedder) = self.embedder {
                    embedder.embed(supplement).await.ok()
                } else {
                    None
                };
                let relevance_score = embedding.as_ref()
                    .map(|emb| self.relevance_tracker.on_new_input(emb))
                    .unwrap_or(0.5);

                // 2. Store in SupplementaryInputStore (consumed by AgentRunner at CycleStart)
                self.supplement_store.store(task_iri, supplement, embedding, relevance_score);
                info!(
                    task_iri = %task_iri,
                    score = relevance_score,
                    "Supplementary input stored in SupplementaryInputStore"
                );

                // 3. Backward compatibility: emit SUPPLEMENTARY_CONTEXT event (for TUI rendering)
                self.inject_to_current_agent(task_iri, supplement).await;
            }
            SupplementaryInputAction::RefineObjective => {
                info!("Supplementary input: refine objective");
                self.event_bus.emit(task_iri, "OBJECTIVE_REFINED", "SA",
                    &serde_json::json!({"refinement": supplement}).to_string()).await;
            }
            SupplementaryInputAction::ProvideConstraint => {
                info!("Supplementary input: provide constraint");
                self.event_bus.emit(task_iri, "CONSTRAINT_ADDED", "SA",
                    &serde_json::json!({"constraint": supplement}).to_string()).await;
            }
            SupplementaryInputAction::PrioritizeStep => {
                info!("Supplementary input: prioritize step");
                self.event_bus.emit(task_iri, "STEP_PRIORITIZED", "SA",
                    &serde_json::json!({"priority": supplement}).to_string()).await;
            }
            SupplementaryInputAction::PauseExecution => {
                warn!("Supplementary input: pause execution");
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
                info!("Supplementary input: resume execution");
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
                info!("Supplementary input: skip current step");
                self.event_bus.emit(task_iri, "STEP_SKIPPED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
            SupplementaryInputAction::AbortCurrentStep => {
                warn!("Supplementary input: abort current step");
                self.event_bus.emit(task_iri, "STEP_ABORTED", "SA",
                    &serde_json::json!({"reason": supplement}).to_string()).await;
            }
        }
        Ok(())
    }

    /// Inject supplementary content into current Agent context
    async fn inject_to_current_agent(&self, task_iri: &str, supplement: &str) {
        info!(task_iri = %task_iri, "Injecting supplementary context into current Agent");
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

    /// Process task with custom TaskContext, supports resume mode
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

        // A3: Calculate task_embedding from 5W2H → set to relevance_tracker
        if let Some(ref embedder) = self.embedder {
            let task_text = format!("{}\n{}", five_w2h.what, five_w2h.why.description);
            if let Ok(task_emb) = embedder.embed(&task_text).await {
                self.relevance_tracker.set_task_context(task_emb);
            }
        }

        // Inject current working directory as execution environment, so LLM knows where to create files
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
                    tracing::debug!(five_w2h_iri = %five_w2h_iri, "5W2H written to blackboard");
                    let route = self.type_router.get_route("task:5W2H");
                    if let Some(route) = route {
                        for event in &route.events {
                            let _ = self.event_bus.emit(task_iri, event, "system:sa", &five_w2h_iri).await;
                        }
                    }
                }
            }
            tracing::info!(task_iri = %task_iri, what = %five_w2h.what, "5W2H initialization complete");
        }

        let perception_hints = self.perception.on_task_start(user_input, task_iri)
            .map(|a| a.relevant_experience_hints)
            .unwrap_or_default();

        // Unified execution path: build ExecutionPlan from JSON-LD workflow or LLM
        let plan = if let Some(ref wf_jsonld) = ctx.workflow_jsonld {
            info!(task_iri = %task_iri, "Using JSON-LD workflow mode — converting through adapter to ExecutionPlan");
            let def = crate::core::workflow::loader::load_workflow_jsonld(wf_jsonld)
                .map_err(|e| CoreError::Internal { message: format!("Workflow parsing failed: {}", e) })?;
            let dag = crate::core::workflow::loader::build_dag(&def)
                .map_err(|e| CoreError::Internal { message: format!("DAG build failed: {}", e) })?;
            let mut plan = crate::core::workflow::adapter::dag_to_execution_plan(&dag, &def, task_iri);
            plan.dag_jsonld = Some(wf_jsonld.clone());
            plan
        } else if ctx.resumed_messages.is_some() {
            self.build_resume_plan()
        } else {
            self.analyze_task_with_llm(user_input, &five_w2h, &perception_hints).await
        };

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
                        warn!("Deadline approaching, marking task as urgent");
                    }
                    "HUMAN_APPROVAL_RESULT" => {
                        if let Ok(result) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                            let request_id = result.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
                            let approved = result.get("approved").and_then(|v| v.as_bool()).unwrap_or(false);
                            if !request_id.is_empty() {
                                self.pending_approvals.lock().await.insert(request_id.to_string(), approved);
                                info!(request_id = %request_id, approved = %approved, "Received human approval result");
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

        // ── Outer SA-level PDCA retry loop ──
        let max_cycles = self.max_pdca_cycles.max(1);
        let mut cycle_feedback: Option<String> = None;
        let mut final_result: Option<TaskResult> = None;

        for cycle_num in 0..max_cycles {
            let resumed = if cycle_num == 0 { ctx.resumed_messages.clone() } else { None };

            info!(
                task_iri = %task_iri,
                cycle_num = cycle_num + 1,
                max_cycles = max_cycles,
                has_feedback = cycle_feedback.is_some(),
                "Starting SA-level PDCA cycle"
            );

            if let Some(ref _feedback) = cycle_feedback {
                self.emit_sa_thought(task_iri,
                    &format!("⚠️ AA did not pass cycle #{} — restarting PDCA with feedback", cycle_num + 1),
                    "pdca_retry_start").await;
            } else {
                self.emit_sa_thought(task_iri,
                    &format!("Starting PDCA cycle {}/{}", cycle_num + 1, max_cycles),
                    "pdca_cycle_start").await;
            }

            let result = self.execute_plan(
                plan.clone(),
                task_iri,
                user_input,
                five_w2h.clone(),
                &five_w2h_iri,
                resumed,
                cycle_feedback.clone(),
            ).await?;

            if result.status == "success" {
                info!(task_iri = %task_iri, cycle_num = cycle_num + 1, "PDCA cycle passed");
                self.emit_sa_thought(task_iri,
                    &format!("✅ PDCA cycle #{} passed — task complete", cycle_num + 1),
                    "pdca_cycle_passed").await;

                if let Some(scheduler) = &self.scheduler {
                    let _ = scheduler.on_task_complete(task_iri).await;
                }
                return Ok(result);
            }

            let last_cycle = cycle_num + 1 >= max_cycles;
            if last_cycle {
                info!(task_iri = %task_iri, cycle_num = cycle_num + 1, "All PDCA cycles exhausted");
                self.emit_sa_thought(task_iri,
                    &format!("⚠️ All {} PDCA cycles completed without full pass — returning last result", max_cycles),
                    "pdca_cycles_exhausted").await;
                final_result = Some(result);
                break;
            }

            cycle_feedback = Some(format!(
                "PDCA Cycle #{} result:\nStatus: {}\n\nAA Evaluation:\n{}\n\n---\nPlease analyze the issues above and create an improved approach.",
                cycle_num + 1, result.status, result.summary
            ));
            final_result = Some(result);
        }

        if let Some(scheduler) = &self.scheduler {
            let _ = scheduler.on_task_complete(task_iri).await;
        }
        Ok(final_result.unwrap_or_else(|| TaskResult {
            task_iri: task_iri.to_string(),
            status: "failed".to_string(),
            summary: "All PDCA cycles exhausted without success".to_string(),
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

        let mut experience_section = String::from("\n## Historical Experience Reference (Similar Tasks)\n");
        experience_section.push_str("The following historical tasks are similar to the current task, for reference only:\n\n");

        for (i, (iri, w2h, score)) in similar.iter().enumerate() {
            experience_section.push_str(&format!(
                "### Similar Task {} (Similarity: {:.0}%)\n",
                i + 1,
                score * 100.0
            ));
            experience_section.push_str(&format!("- **What**: {}\n", w2h.what));
            experience_section.push_str(&format!("- **Why**: {}\n", w2h.why.description));
            if let Some(ref how) = w2h.how {
                if let Some(ref steps) = how.required_steps {
                    experience_section.push_str(&format!("- **Execution Steps**: {}\n", steps));
                }
            }
            experience_section.push_str(&format!("- **Source**: {}\n\n", iri));
        }

        experience_section.push_str("**Note**: Historical experience is for reference only. Please adjust based on the actual current task.\n");
        experience_section
    }

}

mod actions;

use self::actions::{get_action_handler, parse_or_repair_json};

#[cfg(test)]
mod tests;
