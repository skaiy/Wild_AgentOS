use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tracing::{debug, info, instrument, warn};

use crate::config::settings::AgentSettings;
use crate::core::agent_instance::{AgentInstance, AgentRole, AgentStatus};
use crate::core::system_prompt::{
    SystemPromptBuilder, SystemPromptRegion, build_constitution_prompt,
};
use crate::methodology::integration::MethodologyPromptInjector;
use crate::methodology::{
    evolution::{EvolutionEngine, EvolutionEngineHandle},
    gate::{MethodologyGate, MethodologyGateHandle},
    MethodologyRegistry,
};
use crate::core::constitution::ConstitutionRegistry;
use crate::gateway::unified_gateway::{ChatMessage, UnifiedGateway};
use crate::jsonld::{generate_iri, validate_jsonld_node, JsonLdContext, JsonLdNode};
use crate::memory::l0_store::L0Store;
use crate::memory::l1_session::L1Session;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_manager::MemoryManager;
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::core::context_compressor::{ContextWindowManager, ToolResultCompressor};
use crate::templates::template_engine::TemplateEngine;
use crate::tools::hooks::{HookContext, HookManager, HookPoint, HookResult};
use crate::tools::sharing::{ContextInjector, Permission, ShareType, SharingProtocol};
use crate::tools::tool_guard::ToolGuard;
use crate::tools::skill_registry::SkillRegistry;
use crate::core::execution_event::{ExecutionEvent, ExecutionEventKind};
use crate::tools::tool_executor::ToolExecutor;
use crate::root_cause::RootCauseEngine;
use crate::CoreError;

mod execution;
mod prompt;
mod utils;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReActPhase {
    Thought,
    Action,
    Observation,
}

const LLM_RESPONSE_FORMAT_WITH_THOUGHT: &str = r#"
返回 JSON: {"thought": "...", "content": "...", "summary": "...", "action": "tool_call|finish|continue", "emphasis": []}
- thought: 思考过程
- summary: ≤50字摘要
- action: tool_call(调用工具) / finish(任务完成) / continue(继续思考)
- emphasis: 识别的重要约束（数组）

示例:
{"thought": "需要创建文件", "content": "创建 calculator.py", "summary": "创建主文件", "action": "tool_call", "emphasis": []}
"#;

const LLM_RESPONSE_FORMAT_NO_THOUGHT: &str = r#"
返回 JSON: {"content": "...", "summary": "...", "action": "tool_call|finish|continue", "emphasis": []}
- summary: ≤50字摘要
- action: tool_call(调用工具) / finish(任务完成) / continue(继续思考)
- emphasis: 识别的重要约束（数组）

示例:
{"content": "查看文件内容", "summary": "读取文件", "action": "tool_call", "emphasis": []}
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
    /// 从 checkpoint 恢复的历史消息，用于 resume 模式
    pub resumed_messages: Option<Vec<ChatMessage>>,
    /// 从 checkpoint 恢复的 turn 计数
    pub resumed_turn_count: u32,
    /// 从 checkpoint 恢复的 tool call 计数
    pub resumed_tool_count: u32,
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
            resumed_messages: None,
            resumed_turn_count: 0,
            resumed_tool_count: 0,
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

    /// 设置从 checkpoint 恢复的历史消息（resume 模式）
    pub fn with_resumed_messages(mut self, messages: Vec<ChatMessage>, turn_count: u32, tool_count: u32) -> Self {
        self.resumed_messages = Some(messages);
        self.resumed_turn_count = turn_count;
        self.resumed_tool_count = tool_count;
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
            resumed_messages: None,
            resumed_turn_count: 0,
            resumed_tool_count: 0,
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
    pub tracked_actions: Vec<crate::core::tracked_action::TrackedAction>,
    pub archive_iri: Option<String>,
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
    pub scheduler: Option<Arc<MemoryScheduler>>,
    pub prefetch_engine: Option<Arc<PrefetchEngine>>,
    pub unified_graph_store: Option<Arc<oxigraph::store::Store>>,
    pub tool_controller: Option<crate::core::tool_controller::ToolController>,
    pub total_prompt_tokens: Arc<AtomicU64>,
    pub total_completion_tokens: Arc<AtomicU64>,
    pub tool_result_compressor: Option<Arc<std::sync::Mutex<ToolResultCompressor>>>,
    pub context_window_manager: Option<Arc<std::sync::Mutex<ContextWindowManager>>>,
    pub prompt_loader: Option<Arc<crate::core::prompt_loader::PromptLoader>>,
    pub methodology_gate: Option<MethodologyGateHandle>,
    pub root_cause_engine: Option<Arc<RootCauseEngine>>,
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
        let hook_manager = Arc::new(HookManager::new());
        ToolGuard::new().register_hooks(&hook_manager);

        // Initialize MethodologyGate with constitution bindings + EvolutionEngine
        let methodology_gate = {
            let registry = MethodologyRegistry::new();
            let mut gate = MethodologyGate::new(registry);
            gate.register_constitution_bindings(&ConstitutionRegistry::new());
            let evolution = EvolutionEngineHandle::new(EvolutionEngine::new());
            let handle = MethodologyGateHandle::new(gate).with_evolution(evolution);
            handle.register_hooks(&hook_manager);
            Some(handle)
        };

        // Conditionally initialize RootCauseEngine (lightweight, always-on by default)
        let root_cause_engine = {
            let engine = Arc::new(RootCauseEngine::default());
            engine.register_hooks(&hook_manager, "agent");
            Some(engine)
        };

        let mut runner = Self {
            gateway,
            skills,
            blackboard,
            l0_store,
            memory_manager,
            templates,
            tool_executor: {
                let mut exe = ToolExecutor::new();
                exe.set_projection_engine(projection.clone());
                Arc::new(std::sync::RwLock::new(exe))
            },
            agent_settings,
            hook_manager,
            projection,
            sharing,
            emphasis_config: None,
            event_bus: None,
            scheduler: None,
            prefetch_engine: None,
            unified_graph_store: None,
            tool_controller: None,
            total_prompt_tokens: Arc::new(AtomicU64::new(0)),
            total_completion_tokens: Arc::new(AtomicU64::new(0)),
            tool_result_compressor: None,
            context_window_manager: None,
            prompt_loader: None,
            methodology_gate,
            root_cause_engine,
        };
        runner.init_context_compressors();
        runner
    }

    fn init_context_compressors(&mut self) {
        use crate::config::settings::{ContextWindowSettings, ToolResultCompressorSettings};
        let trc_settings = ToolResultCompressorSettings::default();
        if trc_settings.enabled {
            self.tool_result_compressor = Some(Arc::new(std::sync::Mutex::new(
                ToolResultCompressor::new(&trc_settings),
            )));
        }
        let cwm_settings = ContextWindowSettings::default();
        if cwm_settings.max_messages > 0 {
            self.context_window_manager = Some(Arc::new(std::sync::Mutex::new(
                ContextWindowManager::new(&cwm_settings),
            )));
        }
    }

    pub fn with_scheduler(mut self, scheduler: Arc<MemoryScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    pub fn with_prefetch_engine(mut self, prefetch_engine: Arc<PrefetchEngine>) -> Self {
        self.prefetch_engine = Some(prefetch_engine);
        self
    }

    pub fn with_unified_graph_store(mut self, store: Arc<oxigraph::store::Store>) -> Self {
        if let Some(ref gate) = self.methodology_gate {
            let g = gate.inner();
            let guard = g.read();
            let kg = match crate::knowledge_graph::store::KnowledgeGraphStore::with_shared_store(store.clone()) {
                Err(e) => {
                    warn!("Failed to create KG for methodology seed: {}", e);
                    self.unified_graph_store = Some(store);
                    return self;
                }
                Ok(kg) => kg,
            };
            for m in guard.registry().all() {
                let quads = m.to_kg_quads();
                if let Err(e) = kg.write_quads(&quads, "graph:methodology") {
                    warn!("Failed to seed methodology {} into KG: {}", m.id, e);
                }
            }
            info!("Seeded {} methodology definitions into knowledge graph", guard.registry().all().len());
        }
        self.unified_graph_store = Some(store);
        self
    }

    pub fn with_tool_controller(mut self, tc: crate::core::tool_controller::ToolController) -> Self {
        self.tool_controller = Some(tc);
        self
    }

    pub fn with_emphasis_config(mut self, config: crate::config::settings::EmphasisConfig) -> Self {
        self.emphasis_config = Some(config);
        self
    }

    pub fn with_prompt_loader(mut self, loader: crate::core::prompt_loader::PromptLoader) -> Self {
        self.prompt_loader = Some(Arc::new(loader));
        self
    }

    pub fn with_hook_manager(mut self, hook_manager: HookManager) -> Self {
        self.hook_manager = Arc::new(hook_manager);
        self
    }

    /// Load ToolGuard rules from a JSON config file.
    /// The guard is registered into the hook_manager on the next `execute` call.
    /// Default rules are used for categories not specified in the file.
    pub fn with_tool_guard_config<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        match ToolGuard::from_json(path) {
            Ok(guard) => {
                guard.register_hooks(&self.hook_manager);
            }
            Err(e) => {
                warn!("Failed to load ToolGuard config: {}, using defaults", e);
                ToolGuard::new().register_hooks(&self.hook_manager);
            }
        }
        self
    }

    pub fn set_event_bus(&mut self, event_bus: Arc<crate::core::event_bus::EventBus>) {
        self.event_bus = Some(event_bus);
    }


}

#[cfg(test)]
mod tests;
