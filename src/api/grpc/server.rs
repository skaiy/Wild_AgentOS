use std::sync::Arc;
use std::pin::Pin;

use tonic::{Request, Response, Status};
use tokio_stream::{Stream, StreamExt};
use tokio::sync::mpsc;

use crate::core::sa::SupervisorAgent;
use crate::core::agent_runner::AgentRunner;
use crate::core::event_bus::EventBus;
use crate::core::checkpoint::CheckpointManager;
use crate::gateway::unified_gateway::UnifiedGateway;
use crate::memory::consistency_engine::ConsistencyEngine;
use crate::memory::l0_store::L0Store;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_bus::MemoryBus;
use crate::memory::memory_manager::MemoryManager;
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::perception::proactive_engine::ProactiveEngine;
use crate::templates::template_engine::TemplateEngine;
use crate::tools::skill_registry::SkillRegistry;
use crate::config::settings::Settings;
use crate::CoreConfig;

pub mod seapp {
    tonic::include_proto!("seapp");
}

use seapp::*;

pub struct AgentOSService {
    settings: Settings,
    gateway: Arc<UnifiedGateway>,
    l0: Arc<L0Store>,
    blackboard: Arc<Blackboard>,
    projection: Arc<ProjectionEngine>,
    memory_manager: Arc<tokio::sync::Mutex<MemoryManager>>,
    skills: Arc<SkillRegistry>,
    templates: Arc<TemplateEngine>,
    event_bus: Arc<EventBus>,
    checkpoints: Arc<CheckpointManager>,
    scheduler: Arc<MemoryScheduler>,
    prefetch: Arc<PrefetchEngine>,
}

impl AgentOSService {
    pub fn new(settings: Settings) -> Result<Self, String> {
        let gateway = Arc::new(
            UnifiedGateway::new(&settings.gateway)
                .map_err(|e| format!("Gateway init failed: {}", e))?
        );

        let l0 = Arc::new(
            L0Store::new(&settings.memory.l0.path)
                .map_err(|e| format!("L0 init failed: {}", e))?
        );
        let blackboard = Arc::new(
            Blackboard::new().map_err(|e| format!("L2 init failed: {}", e))?
        );
        let projection = Arc::new(ProjectionEngine::new(blackboard.clone(), settings.memory.l3.max_size));
        let skills = Arc::new(SkillRegistry::new());
        let templates = Arc::new(
            TemplateEngine::new(std::path::Path::new(
                settings.agents.template_path.as_deref().unwrap_or("src/templates/templates")
            ))
            .unwrap_or_else(|_| TemplateEngine::new(std::path::Path::new("/nonexistent")).unwrap())
        );
        let event_bus = Arc::new(EventBus::new(settings.agents.event_bus_capacity));

        let memory_bus = Arc::new(MemoryBus::new(event_bus.clone()));
        let consistency = Arc::new(ConsistencyEngine::new(
            memory_bus.clone(), l0.clone(), blackboard.clone(), projection.clone(),
        ));
        let scheduler = Arc::new(MemoryScheduler::new(
            l0.clone(), blackboard.clone(), projection.clone(), consistency.clone(), memory_bus.clone(),
        ));
        let prefetch = Arc::new(PrefetchEngine::new(
            memory_bus.clone(), blackboard.clone(), projection.clone(),
        ));

        let memory_manager = Arc::new(tokio::sync::Mutex::new(
            MemoryManager::with_scheduler(l0.clone(), blackboard.clone(), projection.clone(), CoreConfig::default(), scheduler.clone()),
        ));

        let checkpoints = Arc::new(CheckpointManager::new());

        let eb_checkpoint = event_bus.clone();
        let cp_clone = checkpoints.clone();
        eb_checkpoint.spawn_consumer(
            vec!["CYCLE_STARTED".to_string(), "CYCLE_COMPLETED".to_string()],
            move |event| {
                let cp = cp_clone.clone();
                async move {
                    match event.event_type.as_str() {
                        "CYCLE_STARTED" => {
                            let id = cp.create(&event.task_iri, &format!("cycle:{}", event.task_iri), "{}", "{}", "{}", &[]);
                            tracing::debug!(checkpoint_id = ?id, "Checkpoint created for cycle start");
                        }
                        "CYCLE_COMPLETED" => {
                            let _ = cp.restore(&event.task_iri);
                            tracing::debug!("Checkpoint restored for cycle completion");
                        }
                        _ => {}
                    }
                }
            },
        );

        let eb_5w2h = event_bus.clone();
        eb_5w2h.spawn_consumer(
            vec!["DEADLINE_APPROACHING".to_string(), "BUDGET_EXCEEDED".to_string()],
            move |event| {
                let et = event.event_type.clone();
                async move {
                    tracing::warn!(
                        event_type = %et,
                        task_iri = %event.task_iri,
                        "5W2H 约束告警消费: 需要关注"
                    );
                }
            },
        );

        Ok(Self {
            settings,
            gateway,
            l0,
            blackboard,
            projection,
            memory_manager,
            skills,
            templates,
            event_bus,
            checkpoints,
            scheduler,
            prefetch,
        })
    }

    fn create_sa(&self, settings: &Settings) -> SupervisorAgent {
        let runner = Arc::new(AgentRunner::new(
            self.gateway.clone(),
            self.skills.clone(),
            self.blackboard.clone(),
            self.l0.clone(),
            self.memory_manager.clone(),
            self.templates.clone(),
            settings.agents.clone(),
        ));

        let mut sa = SupervisorAgent::new(
            runner,
            self.templates.clone(),
            self.skills.clone(),
            self.event_bus.clone(),
            settings.agents.max_iterations,
        );

        sa = sa.with_memory(Some(self.blackboard.clone()), Some(self.prefetch.clone()), Some(self.scheduler.clone()));
        sa
    }

    fn apply_request_settings(&self, req: &impl RequestSettings) -> Settings {
        let mut settings = self.settings.clone();
        req.apply_to(&mut settings);
        settings
    }
}

trait RequestSettings {
    fn apply_to(&self, settings: &mut Settings);
}

impl AgentOSService {
    /// 统一的用户补充输入管道
    /// 接收用户的补充输入，通过 EventBus 传递给运行中的 SA 进行分类和动作映射
    pub async fn send_supplementary_input(
        &self,
        task_iri: &str,
        content: &str,
    ) {
        tracing::info!(task_iri = %task_iri, "收到用户补充输入");
        self.event_bus.emit(
            task_iri,
            "USER_SUPPLEMENTARY_INPUT",
            "external",
            content,
        ).await;
    }
}

impl RequestSettings for ExecuteStageRequest {
    fn apply_to(&self, settings: &mut Settings) {
        if !self.llm_api_key.is_empty() {
            settings.gateway.api_key = self.llm_api_key.clone();
        }
        if !self.llm_base_url.is_empty() {
            settings.gateway.base_url = self.llm_base_url.clone();
        }
        if !self.llm_model.is_empty() {
            settings.gateway.default_model = self.llm_model.clone();
            settings.gateway.model_mapping.insert("default".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("planning".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("execution".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("analysis".to_string(), self.llm_model.clone());
        }
    }
}

impl RequestSettings for ChatStreamRequest {
    fn apply_to(&self, settings: &mut Settings) {
        if !self.llm_api_key.is_empty() {
            settings.gateway.api_key = self.llm_api_key.clone();
        }
        if !self.llm_base_url.is_empty() {
            settings.gateway.base_url = self.llm_base_url.clone();
        }
        if !self.llm_model.is_empty() {
            settings.gateway.default_model = self.llm_model.clone();
            settings.gateway.model_mapping.insert("default".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("planning".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("execution".to_string(), self.llm_model.clone());
            settings.gateway.model_mapping.insert("analysis".to_string(), self.llm_model.clone());
        }
    }
}

#[tonic::async_trait]
impl seapp::se_kernel_service_server::SeKernelService for AgentOSService {
    type ChatStreamStream = Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, Status>> + Send>>;

    async fn execute_stage(
        &self,
        request: Request<ExecuteStageRequest>,
    ) -> Result<Response<ExecuteStageResponse>, Status> {
        let req = request.into_inner();
        let settings = self.apply_request_settings(&req);

        let mut sa = self.create_sa(&settings);

        let task_iri = if req.task_iri.is_empty() {
            format!("iri://stage/{}", req.stage_id)
        } else {
            req.task_iri
        };

        let result = sa.process_task(&req.prompt, &task_iri).await
            .map_err(|e| Status::internal(format!("SA execution failed: {}", e)))?;

        let output_bytes = match &result.output {
            Some(v) => serde_json::to_vec(v).unwrap_or_default(),
            None => Vec::new(),
        };

        Ok(Response::new(ExecuteStageResponse {
            status: result.status.clone(),
            summary: result.summary.clone(),
            output_json: output_bytes,
            output_iri: task_iri,
            artifacts: vec![],
            errors: result.errors.clone(),
        }))
    }

    async fn chat_stream(
        &self,
        request: Request<ChatStreamRequest>,
    ) -> Result<Response<Self::ChatStreamStream>, Status> {
        let req = request.into_inner();
        let settings = self.apply_request_settings(&req);

        let (tx, rx) = mpsc::channel::<Result<ChatStreamChunk, Status>>(64);

        let mut sa = self.create_sa(&settings);

        let task_iri = if req.task_iri.is_empty() {
            format!("iri://chat/{}", uuid::Uuid::new_v4().hyphenated())
        } else {
            req.task_iri.clone()
        };

        let _ = tx.send(Ok(ChatStreamChunk {
            content: String::new(),
            done: false,
            status: "processing".to_string(),
        })).await;

        match sa.process_task(&req.prompt, &task_iri).await {
            Ok(result) => {
                let content = extract_content(&result);

                let chunk_size = 20;
                let chars: Vec<char> = content.chars().collect();
                for chunk in chars.chunks(chunk_size) {
                    let chunk_str: String = chunk.iter().collect();
                    if tx.send(Ok(ChatStreamChunk {
                        content: chunk_str,
                        done: false,
                        status: "streaming".to_string(),
                    })).await.is_err() {
                        return Ok(Response::new(Box::pin(
                            tokio_stream::wrappers::ReceiverStream::new(rx)
                        )));
                    }
                }

                let _ = tx.send(Ok(ChatStreamChunk {
                    content: String::new(),
                    done: true,
                    status: result.status.clone(),
                })).await;
            }
            Err(e) => {
                let _ = tx.send(Ok(ChatStreamChunk {
                    content: format!("Error: {}", e),
                    done: true,
                    status: "error".to_string(),
                })).await;
            }
        }

        let output = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output)))
    }

    async fn validate_contract(
        &self,
        _request: Request<ValidateContractRequest>,
    ) -> Result<Response<ValidateContractResponse>, Status> {
        Ok(Response::new(ValidateContractResponse {
            valid: true,
            violations: vec![],
        }))
    }

    async fn flatten_to_frontend(
        &self,
        _request: Request<FlattenRequest>,
    ) -> Result<Response<FlattenResponse>, Status> {
        Ok(Response::new(FlattenResponse {
            frontend_json: "{}".to_string(),
        }))
    }

    async fn submit_human_approval(
        &self,
        _request: Request<SubmitApprovalRequest>,
    ) -> Result<Response<SubmitApprovalResponse>, Status> {
        Ok(Response::new(SubmitApprovalResponse {
            success: true,
            message: "ok".to_string(),
        }))
    }
}

fn extract_content(result: &crate::core::agent_runner::TaskResult) -> String {
    if let Some(ref output) = result.output {
        match output {
            serde_json::Value::String(s) => {
                let cleaned = clean_content(s);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(content) = map.get("content").and_then(|v| v.as_str()) {
                    let cleaned = clean_content(content);
                    if !cleaned.is_empty() {
                        return cleaned;
                    }
                }
                if let Some(summary) = map.get("summary").and_then(|v| v.as_str()) {
                    let cleaned = clean_content(summary);
                    if !cleaned.is_empty() {
                        return cleaned;
                    }
                }
            }
            _ => {}
        }
        if let Some(formatted) = serde_json::to_string_pretty(output).ok() {
            return formatted;
        }
    }

    if !result.summary.is_empty() {
        return clean_content(&result.summary);
    }

    "No content returned".to_string()
}

fn clean_content(text: &str) -> String {
    let re = regex::Regex::new(r#"\{[^}]*"thought"[^}]*\}"#).ok();
    let cleaned = re.map(|r| r.replace_all(text, "").to_string()).unwrap_or_else(|| text.to_string());
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { text.to_string() } else { cleaned }
}
