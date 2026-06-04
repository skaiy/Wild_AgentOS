use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use glidinghorse::config::AgentSettings;
use glidinghorse::core::agent_runner::TaskResult;
use glidinghorse::core::event_bus::{EventBus, Event};
use glidinghorse::core::sa::SupervisorAgent;
use glidinghorse::gateway::UnifiedGateway;
use glidinghorse::memory::l0_store::L0Store;
use glidinghorse::memory::l2_blackboard::Blackboard;
use glidinghorse::memory::l3_projection::ProjectionEngine;
use glidinghorse::memory::memory_manager::MemoryManager;
use glidinghorse::templates::template_engine::TemplateEngine;
use glidinghorse::tools::skill_registry::SkillRegistry;
use glidinghorse::CoreConfig;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tracing::info;

use crate::config::CliConfig;

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub event_type: String,
    pub source: String,
    pub payload: String,
}

pub struct CodeCliEngine {
    sa: SupervisorAgent,
    event_bus: Arc<EventBus>,
    config: CliConfig,
    _temp_dir: TempDir,
    l2_bb: Arc<Blackboard>,
    proj: Arc<ProjectionEngine>,
    mm: Arc<tokio::sync::Mutex<MemoryManager>>,
    l0: Arc<L0Store>,
    prompt_tokens: Arc<AtomicU64>,
    completion_tokens: Arc<AtomicU64>,
}

impl CodeCliEngine {
    pub fn new(mut config: CliConfig) -> anyhow::Result<Self> {
        // Set the process working directory to the configured workspace so that
        // agent_os tool handlers (execute_file_read/write/edit, execute_bash, …)
        // resolve relative paths against the correct root. Without this they
        // default to std::env::current_dir() which may be anything.
        let workspace_abs = std::path::Path::new(&config.workspace)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&config.workspace));
        // Store canonicalized path so engine.workspace() returns the real absolute path
        config.workspace = workspace_abs.to_string_lossy().to_string();
        std::env::set_current_dir(&workspace_abs)
            .map_err(|e| anyhow::anyhow!("无法切换到工作目录 '{}': {}", workspace_abs.display(), e))?;

        let gateway = Arc::new(UnifiedGateway::new(&config.gateway)?);
        let dir = tempfile::TempDir::new()?;

        let l0_path = config.data_dir.as_ref()
            .map(|d| {
                let _ = std::fs::create_dir_all(d);
                d.clone()
            })
            .unwrap_or_else(|| dir.path().join("l0").to_string_lossy().to_string());

        let l0 = Arc::new(
            L0Store::new(&l0_path)
                .map_err(|e| anyhow::anyhow!("L0Store 创建失败: {}", e))?,
        );
        let l2 = Arc::new(
            Blackboard::new()
                .map_err(|e| anyhow::anyhow!("Blackboard 创建失败: {}", e))?,
        );
        let proj = Arc::new(ProjectionEngine::new(l2.clone(), 500));
        let core_config = CoreConfig::default();
        let mm = Arc::new(tokio::sync::Mutex::new(MemoryManager::new(
            l0.clone(),
            l2.clone(),
            proj.clone(),
            core_config,
        )));
        let mm_for_runner = mm.clone();

        let templates_dir = dir.path().join("templates");
        std::fs::create_dir_all(&templates_dir)?;
        let tmpl = Arc::new(
            TemplateEngine::new(&templates_dir)
                .map_err(|e| anyhow::anyhow!("TemplateEngine 创建失败: {}", e))?,
        );

        let skills = Arc::new(SkillRegistry::new());
        let agent_settings = AgentSettings::default();

        let runner = Arc::new(glidinghorse::core::agent_runner::AgentRunner::new(
            gateway,
            skills.clone(),
            l2.clone(),
            l0.clone(),
            mm_for_runner,
            tmpl.clone(),
            agent_settings,
        ).with_prompt_loader(glidinghorse::core::prompt_loader::PromptLoader::new(
            Default::default(),
            tmpl.clone(),
        )));

        let event_bus = Arc::new(EventBus::new(100));

        let l2_bb = l2.clone();
        let sa = SupervisorAgent::new(
            runner,
            tmpl,
            skills,
            event_bus.clone(),
            config.max_iterations,
        )
        .with_memory(Some(l2), None, None);

        let (prompt_tokens, completion_tokens) = sa.token_usage_arcs();

        info!(
            model = %config.model,
            workspace = %config.workspace,
            max_iterations = config.max_iterations,
            "Code CLI 引擎初始化完成"
        );

        Ok(Self {
            sa,
            event_bus,
            config,
            _temp_dir: dir,
            l2_bb,
            proj,
            mm,
            l0: l0.clone(),
            prompt_tokens,
            completion_tokens,
        })
    }

    pub fn rebuild(&mut self) -> anyhow::Result<()> {
        *self = Self::new(self.config.clone())?;
        Ok(())
    }

    pub fn rebuild_with_model(&mut self, model: String) -> anyhow::Result<()> {
        self.config = self.config.clone_with_model(model);
        *self = Self::new(self.config.clone())?;
        Ok(())
    }

    pub fn rebuild_with_api_key(&mut self, api_key: String) -> anyhow::Result<()> {
        self.config = self.config.clone_with_api_key(api_key);
        *self = Self::new(self.config.clone())?;
        Ok(())
    }

    pub fn rebuild_with_api_url(&mut self, api_url: String) -> anyhow::Result<()> {
        self.config = self.config.clone_with_api_url(api_url);
        *self = Self::new(self.config.clone())?;
        Ok(())
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub fn api_key(&self) -> &str {
        &self.config.gateway.api_key
    }

    pub fn api_url(&self) -> &str {
        &self.config.gateway.base_url
    }

    pub fn workspace(&self) -> &str {
        &self.config.workspace
    }

    pub fn max_iterations(&self) -> u32 {
        self.config.max_iterations
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_bus.subscribe()
    }

    pub async fn process_task(&mut self, user_input: &str) -> anyhow::Result<(String, TaskResult)> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let task_iri = format!("iri://task/{}", task_id);

        let result = self.sa.process_task(user_input, &task_iri).await?;

        info!(
            task_iri = %task_iri,
            status = %result.status,
            turn_count = result.turn_count,
            tool_call_count = result.tool_call_count,
            "任务处理完成"
        );

        Ok((task_iri, result))
    }

    /// Returns a clone of the internal EventBus (for supplementary input / event monitoring).
    pub fn event_bus(&self) -> Arc<EventBus> {
        self.event_bus.clone()
    }

    /// Blackboard reference (lock-free node count reads).
    pub fn l2_bb(&self) -> Arc<Blackboard> {
        self.l2_bb.clone()
    }

    /// ProjectionEngine reference (std RwLock for cache_stats, safe from sync context).
    pub fn proj(&self) -> Arc<ProjectionEngine> {
        self.proj.clone()
    }

    /// MemoryManager Arc (for lock-free L1 session count reads via atomic).
    pub fn mm(&self) -> Arc<tokio::sync::Mutex<MemoryManager>> {
        self.mm.clone()
    }

    /// L0Store reference (for checkpoint loading during resume).
    pub fn l0(&self) -> Arc<L0Store> {
        self.l0.clone()
    }

    /// Token counter Arcs (lock-free reads from TUI).
    pub fn token_arcs(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (self.prompt_tokens.clone(), self.completion_tokens.clone())
    }

    /// Query memory subsystem usage counts: (L1_session_count, L2_node_count, L3_projection_count)
    ///
    /// All reads are lock-free or use independent locks (not the engine lock),
    /// so this can be called from the UI thread without blocking.
    pub fn memory_stats(&self) -> (u64, u64, u64) {
        let l2 = self.l2_bb.node_count();
        let l3 = self.proj.cache_stats().total_views as u64;
        let l1 = self.sa.try_l1_session_count().unwrap_or(0);
        (l1, l2, l3)
    }

    pub async fn list_checkpoints(&self) -> anyhow::Result<Vec<glidinghorse::core::checkpoint::CheckpointData>> {
        let prefix = "iri://checkpoint/";
        let entries = self.l0.scan_iri_prefix(prefix, 100)?;
        let mut results: Vec<glidinghorse::core::checkpoint::CheckpointData> = entries
            .iter()
            .filter_map(|e| serde_json::from_str(&e.content).ok())
            .collect();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        results.truncate(20);
        Ok(results)
    }

    pub async fn resume_task(&mut self, task_iri: &str) -> anyhow::Result<TaskResult> {
        let cm = glidinghorse::core::checkpoint::CheckpointManager::with_persistence(self.l0.clone());
        let cp = cm.restore_latest(task_iri)?
            .ok_or_else(|| anyhow::anyhow!("没有找到 task_iri={} 的 checkpoint", task_iri))?;

        let _agent_state: serde_json::Value = serde_json::from_str(&cp.agent_state_json)?;

        let resume_input = format!(
            "继续执行之前中断的任务。上次进度: {}\n\n请从上次中断处继续。",
            cp.name
        );
        self.process_task_with_iri(&resume_input, task_iri).await
    }

    /// 从 checkpoint 恢复任务，包含完整的历史上下文消息
    pub async fn resume_task_with_messages(&mut self, task_iri: &str, resumed_messages: Vec<glidinghorse::gateway::unified_gateway::ChatMessage>) -> anyhow::Result<TaskResult> {
        let resume_input = "继续执行之前中断的任务。请从上次中断处继续。".to_string();
        self.process_task_with_iri_and_messages(&resume_input, task_iri, Some(resumed_messages)).await
    }

    /// Process a task with an externally-generated task IRI so the caller
    /// can emit supplementary input events during execution.
    pub async fn process_task_with_iri(&mut self, user_input: &str, task_iri: &str) -> anyhow::Result<TaskResult> {
        self.process_task_with_iri_and_messages(user_input, task_iri, None).await
    }

    /// Process a task with optional resumed messages (for checkpoint resume)
    pub async fn process_task_with_iri_and_messages(
        &mut self,
        user_input: &str,
        task_iri: &str,
        resumed_messages: Option<Vec<glidinghorse::gateway::unified_gateway::ChatMessage>>,
    ) -> anyhow::Result<TaskResult> {
        use glidinghorse::core::agent_runner::TaskContext;

        let ctx = TaskContext::new(task_iri, user_input, self.config.max_iterations)
            .with_original_task(user_input);
        let ctx = if let Some(msgs) = resumed_messages {
            // 从历史消息推算 turn/tool 计数
            let turn_count = msgs.iter().filter(|m| m.role == "assistant").count() as u32;
            let tool_count = msgs.iter().filter(|m| m.role == "tool" || m.tool_call_id.is_some()).count() as u32;
            ctx.with_resumed_messages(msgs, turn_count, tool_count)
        } else {
            ctx
        };

        let result = self.sa.process_task_with_context(user_input, task_iri, ctx).await?;

        info!(
            task_iri = %task_iri,
            status = %result.status,
            turn_count = result.turn_count,
            tool_call_count = result.tool_call_count,
            "任务处理完成"
        );

        Ok(result)
    }
}
