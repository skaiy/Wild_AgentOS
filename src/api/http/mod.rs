use std::collections::HashMap;
use std::sync::Arc;
use std::convert::Infallible;

use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_stream::{Stream, StreamExt};
use futures::stream;
use tracing::info;

use crate::config::settings::Settings;
use crate::core::core_types::SemanticCore;
use crate::core::event_bus::EventBus;
use crate::core::execution_event::ExecutionEventEmitter;
use crate::core::execution_event::{ExecutionEvent, ExecutionEventKind};
use crate::core::sa::SupervisorAgent;
use crate::core::agent_runner::AgentRunner;
use crate::gateway::unified_gateway::UnifiedGateway;
use crate::memory::l0_store::L0Store;
use crate::memory::memory_manager::MemoryManager;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::scheduler::MemoryScheduler;
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::unified_graph::UnifiedGraphStore;
use crate::knowledge_graph::rdf_mapper::RdfMapper;
use crate::knowledge_graph::store::KnowledgeGraphStore;
use crate::knowledge_graph::types::{EdgeDef, LLMExtractionOutput, NodeDef};
use crate::templates::template_engine::TemplateEngine;
use crate::tools::skill_registry::SkillRegistry;
use crate::tools::tool_guard::{GuardAuditEntry, GUARD_AUDIT_LOG};

use std::sync::OnceLock;
use std::sync::RwLock;

static RAW_SKILL_JSONLD: OnceLock<RwLock<HashMap<String, Value>>> = OnceLock::new();

fn raw_skill_jsonld() -> &'static RwLock<HashMap<String, Value>> {
    RAW_SKILL_JSONLD.get_or_init(|| RwLock::new(HashMap::new()))
}

fn store_raw_jsonld(iri: &str, doc: Value) {
    if let Ok(mut guard) = raw_skill_jsonld().write() {
        guard.insert(iri.to_string(), doc);
    }
}

fn get_raw_jsonld(iri: &str) -> Option<Value> {
    raw_skill_jsonld().read().ok().and_then(|g| g.get(iri).cloned())
}

pub struct AppState {
    pub core: Arc<SemanticCore>,
    pub kg_store: Arc<oxigraph::store::Store>,
    /// Shared gateway for creating SupervisorAgent instances in HTTP stream handler
    pub gateway: Arc<UnifiedGateway>,
    /// App settings (gateway config, agent params, etc.)
    pub settings: Settings,
    /// L0 store shared with agent runner
    pub l0: Arc<L0Store>,
    /// Memory manager shared with agent runner
    pub memory_manager: Arc<tokio::sync::Mutex<MemoryManager>>,
    /// Template engine shared with agent runner
    pub templates: Arc<TemplateEngine>,
    /// Memory scheduler
    pub scheduler: Arc<MemoryScheduler>,
    /// Prefetch engine
    pub prefetch: Arc<PrefetchEngine>,
    /// Unified graph store
    pub unified_graph: Arc<UnifiedGraphStore>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Deserialize)]
pub struct TaskRequest {
    pub user_input: String,
}

#[derive(Deserialize)]
pub struct NodeWriteRequest {
    pub task_iri: String,
    pub json_ld: String,
    pub created_by: Option<String>,
}

#[derive(Deserialize)]
pub struct ProjectionRequest {
    pub task_iri: String,
    pub frame_name: Option<String>,
    pub params: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct StreamTaskRequest {
    pub prompt: String,
    pub task_iri: Option<String>,
    pub include_thought: Option<bool>,
    pub include_tool_calls: Option<bool>,
}

#[derive(Deserialize)]
pub struct RealtimeStatusRequest {
    pub task_iri: String,
}

#[derive(Deserialize)]
pub struct KgImportRequest {
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
    pub graph: String,
    #[serde(default = "default_true")]
    pub clear_before: bool,
}

fn default_true() -> bool { true }

#[derive(Deserialize)]
pub struct KgQueryRequest {
    pub sparql: String,
    pub named_graph: Option<String>,
}

#[derive(Serialize)]
pub struct StreamEventResponse {
    pub event_type: String,
    pub data: Value,
}

pub fn build_router(
    core: Arc<SemanticCore>,
    kg_store: Arc<oxigraph::store::Store>,
    gateway: Arc<UnifiedGateway>,
    settings: Settings,
    l0: Arc<L0Store>,
    memory_manager: Arc<tokio::sync::Mutex<MemoryManager>>,
    templates: Arc<TemplateEngine>,
    scheduler: Arc<MemoryScheduler>,
    prefetch: Arc<PrefetchEngine>,
    unified_graph: Arc<UnifiedGraphStore>,
) -> Router {
    // Load skills from the skills/ directory (if it exists next to the binary/config)
    load_skills_from_dir(&core.skills, &settings);

    let state = Arc::new(AppState {
        core,
        kg_store,
        gateway,
        settings,
        l0,
        memory_manager,
        templates,
        scheduler,
        prefetch,
        unified_graph,
    });

    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/tasks", post(create_task_handler))
        .route("/api/v1/tasks/:task_iri", get(get_task_handler))
        .route("/api/v1/tasks/stream", post(stream_task_handler))
        .route("/api/v1/tasks/:task_iri/status", get(get_realtime_status_handler))
        .route("/api/v1/tasks/:task_iri/details", get(get_execution_details_handler))
        .route("/api/v1/nodes", post(write_node_handler))
        .route("/api/v1/nodes/:node_iri", get(read_node_handler))
        .route("/api/v1/projections", post(get_projection_handler))
        .route("/api/v1/events", post(emit_event_handler))
        .route("/api/v1/batch/events", get(stream_batch_events_handler))
        .route("/api/v1/skills", get(list_skills_handler))
        .route("/api/v1/guard/audit", get(guard_audit_handler))
        .route("/api/v1/guard/stats", get(guard_stats_handler))
        .route("/api/v1/kg/import", post(kg_import_handler))
        .route("/api/v1/kg/query", post(kg_query_handler))
        .with_state(state)
}

/// Scan skill directories for JSON-LD definitions and register them.
/// Looks in: (1) AGENT_OS_SKILLS_DIR env var, (2) `skills/` relative to CWD,
/// (3) `../skills/` relative to CWD (supports agentos/skills/ when CWD is agentos/gliding_horse).
fn load_skills_from_dir(skills: &Arc<SkillRegistry>, settings: &Settings) {
    let mut dirs_to_scan: Vec<std::path::PathBuf> = Vec::new();

    // 1. Environment variable override
    if let Ok(skills_dir) = std::env::var("AGENT_OS_SKILLS_DIR") {
        let p = std::path::PathBuf::from(&skills_dir);
        if p.is_dir() {
            dirs_to_scan.push(p);
        }
    }

    // 2. CWD/skills/ (e.g. when running from gliding_horse root)
    let cwd_skills = std::path::Path::new("skills");
    if cwd_skills.is_dir() {
        dirs_to_scan.push(cwd_skills.to_path_buf());
    }

    // 3. Parent/skills/ (e.g. agentos/skills/ when CWD = agentos/gliding_horse)
    let parent_skills = std::path::Path::new("../skills");
    if parent_skills.is_dir() {
        dirs_to_scan.push(parent_skills.to_path_buf());
    }

    if dirs_to_scan.is_empty() {
        info!("No skills/ directory found, skipping skill loading");
        return;
    }

    for skills_dir in &dirs_to_scan {
        if let Ok(entries) = std::fs::read_dir(skills_dir) {
            for entry in entries.flatten() {
                let jsonld = entry.path().join("skill.jsonld");
                if jsonld.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&jsonld) {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                            if let Some(skill) = parse_compact_jsonld_to_skill_meta(&parsed) {
                                info!(name = %skill.name, iri = %skill.skill_iri, "Loaded skill from {}", jsonld.display());
                                store_raw_jsonld(&skill.skill_iri, parsed);
                                skills.register_skill(skill);
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = settings; // used for future parametric skill loading
}

/// Parse a compact JSON-LD skill file (like skills/test_skill/skill.jsonld) into SkillMeta.
fn parse_compact_jsonld_to_skill_meta(json: &Value) -> Option<crate::tools::skill_registry::SkillMeta> {
    use crate::tools::skill_registry::SkillMeta;
    let skill_iri = json.get("@id")?.as_str()?.to_string();
    let name = json.get("schema:name")
        .or_else(|| json.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let description = json.get("schema:description")
        .or_else(|| json.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = json.get("skill:version")
        .or_else(|| json.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0")
        .to_string();
    let category = json.get("skill:category")
        .or_else(|| json.get("category"))
        .and_then(|v| v.as_str())
        .unwrap_or("general")
        .to_string();
    let tags: Vec<String> = json.get("skill:tags")
        .or_else(|| json.get("tags"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|t| t.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let maturity = json.get("skill:maturity")
        .or_else(|| json.get("maturity"))
        .and_then(|v| v.as_str())
        .unwrap_or("stable")
        .to_string();
    let skill_types: Vec<String> = json.get("@type")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|t| t.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(SkillMeta {
        skill_iri,
        name,
        description,
        version,
        category,
        security_level: "normal".to_string(),
        allowed_roles: vec!["DA".to_string()],
        input_schema: serde_json::json!({"type": "object", "properties": {"prompt": {"type": "string", "description": "自然语言排程意图"}}}),
        output_schema: serde_json::json!({"type": "object", "properties": {"result": {"type": "string"}, "assignments": {"type": "array"}}}),
        compiled_template: "{}".to_string(),
        signature: None,
        signature_algorithm: None,
        input_mapping: HashMap::new(),
        output_mapping: HashMap::new(),
        skill_types,
    })
}

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({
        "l2_nodes": state.core.blackboard.node_count(),
        "l2_bytes": state.core.blackboard.total_bytes(),
        "events": state.core.events.event_count(),
        "subscribers": state.core.events.subscriber_count(),
        "skills": state.core.skills.skill_count(),
        "checkpoints": state.core.checkpoints.checkpoint_count(),
    }))
}

async fn guard_audit_handler() -> impl IntoResponse {
    let log = GUARD_AUDIT_LOG.read();
    let entries: Vec<GuardAuditEntry> = log.clone();
    Json(json!({
        "total": entries.len(),
        "entries": entries,
    }))
}

async fn guard_stats_handler() -> impl IntoResponse {
    let log = GUARD_AUDIT_LOG.read();
    let total = log.len();
    if total == 0 {
        return Json(json!({
            "total_checks": 0,
            "passed_checks": 0,
            "failed_checks": 0,
            "pass_rate": 1.0,
        }));
    }
    let passed = log.iter().filter(|e| e.validation_passed).count();
    Json(json!({
        "total_checks": total,
        "passed_checks": passed,
        "failed_checks": total - passed,
        "pass_rate": passed as f64 / total as f64,
    }))
}

async fn create_task_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TaskRequest>,
) -> impl IntoResponse {
    match state.core.init_task(&req.user_input, None, None).await {
        Ok(task_iri) => (
            StatusCode::CREATED,
            Json(json!({"task_iri": task_iri, "status": "created"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn get_task_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(task_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.core.read_node(&task_iri).await {
        Ok(Some(node)) => Json(json!({
            "task_iri": task_iri,
            "found": true,
            "node": node,
        })),
        Ok(None) => Json(json!({
            "task_iri": task_iri,
            "found": false,
        })),
        Err(e) => Json(json!({
            "task_iri": task_iri,
            "error": e.to_string(),
        })),
    }
}

async fn stream_task_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StreamTaskRequest>,
) -> impl IntoResponse {
    let task_iri = req.task_iri.unwrap_or_else(|| {
        format!("iri://stream/{}", uuid::Uuid::new_v4().hyphenated())
    });

    let event_bus = state.core.events.clone();
    let task_iri_clone = task_iri.clone();
    let mut rx = event_bus.subscribe();

    // --- Spawn agent execution using shared components (same event_bus) ---
    let gateway = state.gateway.clone();
    let skills = state.core.skills.clone();
    let blackboard = state.core.blackboard.clone();
    let l0 = state.l0.clone();
    let memory_manager = state.memory_manager.clone();
    let templates = state.templates.clone();
    let scheduler = state.scheduler.clone();
    let prefetch = state.prefetch.clone();
    let ug_store = state.unified_graph.store();
    let settings = state.settings.clone();
    let prompt = req.prompt.clone();
    let task_iri_exec = task_iri.clone();
    let event_bus_exec = event_bus.clone();
    let include_thought = req.include_thought.unwrap_or(true);
    let include_tool_calls = req.include_tool_calls.unwrap_or(true);

    tokio::spawn(async move {
        let mut runner = AgentRunner::new(
            gateway,
            skills.clone(),
            blackboard.clone(),
            l0,
            memory_manager,
            templates.clone(),
            settings.agents.clone(),
        )
        .with_scheduler(scheduler.clone())
        .with_prefetch_engine(prefetch.clone())
        .with_unified_graph_store(ug_store);

        // Wire up unified KG store for tool executor
        {
            let ug = runner.unified_graph_store.clone();
            if let Some(ug) = ug {
                let mut executor = runner.tool_executor.write().expect("tool_executor RwLock poisoned");
                executor.set_unified_kg_store(ug);
            }
        }

        // Register remote MCP server tools from AGENT_OS_MCP_SERVERS env var.
        // Comma-separated URLs (e.g. "http://localhost:8900/mcp").
        // Each server is connected (tools/list) and its tools registered
        // in the ToolExecutor so the Agent can call them during ReAct.
        // Retries connect with exponential backoff so a transient BFF startup
        // race does not silently fall back to built-in tools only.
        if let Ok(mcp_urls) = std::env::var("AGENT_OS_MCP_SERVERS") {
            let mut mcp_client = crate::tools::mcp_client::McpClient::new();
            for url in mcp_urls.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                let server_name = format!("bff_mcp");
                mcp_client.register_server(&server_name, url);
                let mut connect_result: Option<Vec<crate::tools::mcp_client::McpTool>> = None;
                let mut last_err: Option<String> = None;
                for attempt in 1u32..=5 {
                    match mcp_client.connect(&server_name).await {
                        Ok(tools) if !tools.is_empty() => {
                            connect_result = Some(tools);
                            break;
                        }
                        Ok(tools) => {
                            last_err = Some(format!(
                                "tools/list returned 0 tools (attempt {})",
                                attempt
                            ));
                            // No tools returned: still treat as transient and retry.
                            tracing::warn!(
                                server = %server_name,
                                attempt,
                                "MCP server returned 0 tools, will retry"
                            );
                            let _ = tools;
                        }
                        Err(e) => {
                            last_err = Some(e.to_string());
                            tracing::warn!(
                                server = %server_name,
                                attempt,
                                error = %e,
                                "MCP connect attempt failed, will retry"
                            );
                        }
                    }
                    let backoff_ms = 200u64.saturating_mul(1u64 << (attempt - 1));
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                }
                match connect_result {
                    Some(tools) => {
                        tracing::info!(server = %server_name, tools = tools.len(), "MCP server connected, registering tools");
                        let mut executor = runner.tool_executor.write().expect("tool_executor RwLock");
                        for tool in &tools {
                            let tool_name = format!("mcp__{}__{}", server_name, tool.name);
                            let desc = tool.description.clone().unwrap_or_default();
                            let schema = tool.input_schema.clone().unwrap_or(serde_json::json!({}));
                            let url_c = url.to_string();
                            let raw_name = tool.name.clone();
                            let server_name_c = server_name.clone();
                            executor.register(
                                &tool_name,
                                &desc,
                                schema,
                                Arc::new(move |args: serde_json::Value| {
                                    let url = url_c.clone();
                                    let name = raw_name.clone();
                                    let srv = server_name_c.clone();
                                    Box::pin(async move {
                                        let client = reqwest::Client::new();
                                        let req_body = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": 1,
                                            "method": "tools/call",
                                            "params": {
                                                "name": name,
                                                "arguments": args,
                                            }
                                        });
                                        let resp = client.post(&url)
                                            .json(&req_body)
                                            .timeout(std::time::Duration::from_secs(120))
                                            .send().await
                                            .map_err(|e| format!("MCP call to {} failed: {}", srv, e))?;
                                        let body: serde_json::Value = resp.json().await
                                            .map_err(|e| format!("MCP response parse error: {}", e))?;
                                        if let Some(error) = body.get("error") {
                                            Err(format!("MCP error from {}: {}", srv, error))
                                        } else {
                                            Ok(body.get("result").cloned().unwrap_or(serde_json::json!({})))
                                        }
                                    })
                                }),
                                &["DA"],
                            );
                        }
                        // Also register tools with their original names (without prefix)
                        // so the LLM can call them by their natural names like "get_tasks"
                        for tool in &tools {
                            let desc = tool.description.clone().unwrap_or_default();
                            let schema = tool.input_schema.clone().unwrap_or(serde_json::json!({}));
                            let url_c = url.to_string();
                            let raw_name_for_register = tool.name.clone();
                            let raw_name_for_closure = tool.name.clone();
                            let server_name_c = server_name.clone();
                            executor.register(
                                &raw_name_for_register,
                                &desc,
                                schema,
                                Arc::new(move |args: serde_json::Value| {
                                    let url = url_c.clone();
                                    let name = raw_name_for_closure.clone();
                                    let srv = server_name_c.clone();
                                    Box::pin(async move {
                                        let client = reqwest::Client::new();
                                        let req_body = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": 1,
                                            "method": "tools/call",
                                            "params": {
                                                "name": name,
                                                "arguments": args,
                                            }
                                        });
                                        let resp = client.post(&url)
                                            .json(&req_body)
                                            .timeout(std::time::Duration::from_secs(120))
                                            .send().await
                                            .map_err(|e| format!("MCP call to {} failed: {}", srv, e))?;
                                        let body: serde_json::Value = resp.json().await
                                            .map_err(|e| format!("MCP response parse error: {}", e))?;
                                        if let Some(error) = body.get("error") {
                                            Err(format!("MCP error from {}: {}", srv, error))
                                        } else {
                                            Ok(body.get("result").cloned().unwrap_or(serde_json::json!({})))
                                        }
                                    })
                                }),
                                &["DA"],
                            );
                        }
                    }
                    None => {
                        tracing::warn!(
                            server = %server_name,
                            error = ?last_err,
                            "Failed to connect MCP server after retries (non-fatal, continuing)"
                        );
                    }
                }
            }
        }

        let runner = Arc::new(runner);

        let mut sa = SupervisorAgent::new(
            runner,
            templates,
            skills,
            event_bus_exec.clone(), // Same event_bus as subscriber
            settings.agents.max_iterations,
        );
        sa = sa.with_memory(
            Some(blackboard),
            Some(prefetch),
            Some(scheduler),
        );

        let emitter = ExecutionEventEmitter::with_options(
            &task_iri_exec,
            None,
            Some(event_bus_exec),
            include_thought,
            include_tool_calls,
        );

        emitter.emit_phase_change("idle", "plan", "PA", "Task started");

        match sa.process_task(&prompt, &task_iri_exec).await {
            Ok(result) => {
                emitter.emit_completion(&result.status, &result.summary, result.output.clone());
            }
            Err(e) => {
                emitter.emit_error("ExecutionError", &e.to_string(), "SA", false);
                emitter.emit_completion("failed", &e.to_string(), None);
            }
        }
    });

    // --- Stream events from the shared event_bus as SSE ---
    let stream = async_stream::stream! {
        yield Ok::<axum::response::sse::Event, std::convert::Infallible>(Event::default().event("task_started").data(json!({
            "task_iri": task_iri_clone,
            "status": "started"
        }).to_string()));

        loop {
            match rx.recv().await {
                Ok(event) => {
                    if event.task_iri != task_iri_clone {
                        continue;
                    }

                    if let Some(sse_event) = convert_event_to_sse(&event) {
                        yield Ok(sse_event);
                    }

                    if event.event_type == "EXECUTION_COMPLETE" || event.event_type == "TASK_FAILED" {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn get_realtime_status_handler(
    State(_state): State<Arc<AppState>>,
    axum::extract::Path(task_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    Json(json!({
        "task_iri": task_iri,
        "status": "running",
        "current_phase": "do",
        "current_agent": {
            "id": "da_001",
            "role": "DA",
            "status": "running",
            "turn": 1
        },
        "progress": {
            "completed_steps": 1,
            "total_steps": 4,
            "percentage": 25
        }
    }))
}

async fn get_execution_details_handler(
    State(_state): State<Arc<AppState>>,
    axum::extract::Path(task_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    Json(json!({
        "task_iri": task_iri,
        "status": "running",
        "current_phase": "do",
        "plan": {
            "plan_id": "plan_001",
            "description": "执行任务",
            "steps": []
        },
        "steps": [],
        "agent_sessions": [],
        "stats": {
            "total_turns": 0,
            "total_tool_calls": 0,
            "total_tokens": 0
        }
    }))
}

async fn write_node_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeWriteRequest>,
) -> impl IntoResponse {
    match state
        .core
        .write_node(&req.task_iri, &req.json_ld, None, req.created_by.as_deref())
        .await
    {
        Ok(node_iri) => (
            StatusCode::CREATED,
            Json(json!({"node_iri": node_iri, "accepted": true})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"accepted": false, "error": e.to_string()})),
        ),
    }
}

async fn get_projection_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProjectionRequest>,
) -> impl IntoResponse {
    let frame = req.frame_name.unwrap_or_else(|| "reference_only".to_string());
    let params = req.params.unwrap_or_default();
    match state.core.projection.project(&req.task_iri, &frame, params).await {
        Ok(projection) => Json(json!({
            "projection": serde_json::from_str::<Value>(&projection).ok(),
            "frame": frame,
            "task_iri": req.task_iri,
        })),
        Err(e) => Json(json!({"error": e.to_string(), "task_iri": req.task_iri})),
    }
}

async fn read_node_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(node_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.core.read_node(&node_iri).await {
        Ok(Some(node)) => Json(json!({
            "found": true,
            "json_ld": node.json_ld,
        })),
        Ok(None) => Json(json!({"found": false})),
        Err(e) => Json(json!({"found": false, "error": e.to_string()})),
    }
}

async fn emit_event_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let task_iri = payload.get("task_iri").and_then(|v| v.as_str()).unwrap_or("unknown");
    let event_type = payload.get("event_type").and_then(|v| v.as_str()).unwrap_or("CUSTOM");
    let source = payload.get("source").and_then(|v| v.as_str()).unwrap_or("http_api");
    let event_id = state.core.emit_event(task_iri, event_type, source, &payload.to_string()).await;
    Json(json!({"event_id": event_id, "status": "emitted"}))
}

async fn list_skills_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let skills = state.core.skills.list_all_skills();
    let enriched: Vec<Value> = skills
        .into_iter()
        .map(|skill| {
            let mut value = serde_json::to_value(&skill).unwrap_or_else(|_| json!({}));
            if let Some(raw) = get_raw_jsonld(&skill.skill_iri) {
                if let Value::Object(ref mut map) = value {
                    if let Value::Object(raw_map) = raw {
                        for (k, v) in raw_map {
                            map.entry(k).or_insert(v);
                        }
                    }
                }
            }
            value
        })
        .collect();
    Json(json!({
        "count": enriched.len(),
        "skills": enriched,
    }))
}

async fn stream_batch_events_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let event_bus = state.core.events.clone();
    let mut rx = event_bus.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if !event.event_type.starts_with("BATCH_") {
                        continue;
                    }
                    let payload: Value =
                        serde_json::from_str(&event.payload).unwrap_or(Value::Null);
                    let data = json!({
                        "channel": "batch",
                        "event_type": event.event_type,
                        "source": event.source_agent_iri,
                        "task_iri": event.task_iri,
                        "timestamp": event.timestamp.to_rfc3339(),
                        "payload": payload,
                    });
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("batch")
                            .data(data.to_string()),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Expand short namespace prefixes to absolute IRIs for Oxigraph.
/// e.g. "aps:Bench" → "http://aps.local/ontology/Bench"
///      "graph:aps/benches" → "http://aps.local/graph/benches"
///      "rdfs:subClassOf" → "http://www.w3.org/2000/01/rdf-schema#subClassOf"
fn expand_iri(s: &str) -> String {
    if s.contains('/') && (s.starts_with("http://") || s.starts_with("https://")) {
        return s.to_string();
    }
    if let Some(rest) = s.strip_prefix("aps:") {
        format!("http://aps.local/ontology/{}", rest)
    } else if let Some(rest) = s.strip_prefix("graph:aps/") {
        format!("http://aps.local/graph/{}", rest)
    } else if let Some(rest) = s.strip_prefix("rdfs:") {
        format!("http://www.w3.org/2000/01/rdf-schema#{}", rest)
    } else if let Some(rest) = s.strip_prefix("rdf:") {
        format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#{}", rest)
    } else {
        s.to_string()
    }
}

fn expand_extraction(mut extraction: LLMExtractionOutput) -> LLMExtractionOutput {
    for node in &mut extraction.nodes {
        node.node_type = expand_iri(&node.node_type);
    }
    for edge in &mut extraction.edges {
        edge.relation = expand_iri(&edge.relation);
    }
    extraction
}

async fn kg_import_handler(
    State(state): State<Arc<AppState>>,
    Json(mut req): Json<KgImportRequest>,
) -> impl IntoResponse {
    let store = state.kg_store.clone();
    let graph_iri = expand_iri(&req.graph);

    if req.clear_before {
        let clear = format!("DELETE WHERE {{ GRAPH <{}> {{ ?s ?p ?o . }} }}", graph_iri);
        if let Err(e) = store.update(&clear) {
            tracing::warn!(graph = %graph_iri, "KG clear skipped: {}", e);
        }
    }

    let extraction = expand_extraction(LLMExtractionOutput {
        nodes: req.nodes,
        edges: req.edges,
    });
    let result = RdfMapper::map_extraction(&extraction, &graph_iri);

    let kg = match KnowledgeGraphStore::with_shared_store(store) {
        Ok(kg) => kg,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e})),
            )
        }
    };

    match kg.write_quads(&result.quads, &graph_iri) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "entity_count": result.entity_count,
                "relation_count": result.relation_count,
                "quad_count": result.quads.len(),
                "graph": req.graph,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        ),
    }
}

async fn kg_query_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<KgQueryRequest>,
) -> impl IntoResponse {
    let store = state.kg_store.clone();
    let kg = match KnowledgeGraphStore::with_shared_store(store) {
        Ok(kg) => kg,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e})),
            )
        }
    };

    let named_graph = req.named_graph.as_deref().map(|g| expand_iri(g));
    match kg.query_sparql(&req.sparql, named_graph.as_deref()) {
        Ok(results) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": results,
                "count": results.len(),
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        ),
    }
}

fn convert_event_to_sse(event: &crate::core::event_bus::Event) -> Option<Event> {
    use crate::core::event_bus::EventType;

    let event_type = EventType::from_str(&event.event_type);
    let (event_name, data) = match event_type {
        EventType::PlanStarted => (
            "phase_change",
            json!({
                "from_phase": "idle",
                "to_phase": "plan",
                "agent_role": "PA"
            }),
        ),
        EventType::PlanCompleted => (
            "phase_change",
            json!({
                "from_phase": "plan",
                "to_phase": "do",
                "agent_role": "PA"
            }),
        ),
        EventType::DoStarted => (
            "phase_change",
            json!({
                "from_phase": "plan",
                "to_phase": "do",
                "agent_role": "DA"
            }),
        ),
        EventType::DoCompleted => (
            "phase_change",
            json!({
                "from_phase": "do",
                "to_phase": "check",
                "agent_role": "DA"
            }),
        ),
        EventType::CheckStarted => (
            "phase_change",
            json!({
                "from_phase": "do",
                "to_phase": "check",
                "agent_role": "CA"
            }),
        ),
        EventType::CheckCompleted => (
            "phase_change",
            json!({
                "from_phase": "check",
                "to_phase": "act",
                "agent_role": "CA"
            }),
        ),
        EventType::ActStarted => (
            "phase_change",
            json!({
                "from_phase": "check",
                "to_phase": "act",
                "agent_role": "AA"
            }),
        ),
        EventType::ActCompleted => (
            "phase_change",
            json!({
                "from_phase": "act",
                "to_phase": "completed",
                "agent_role": "AA"
            }),
        ),
        EventType::AgentStarted => (
            "agent_status",
            json!({
                "agent_id": event.source_agent_iri,
                "status": "running"
            }),
        ),
        EventType::AgentCompleted => (
            "agent_status",
            json!({
                "agent_id": event.source_agent_iri,
                "status": "completed"
            }),
        ),
        EventType::AgentError => (
            "error",
            json!({
                "agent_id": event.source_agent_iri,
                "message": event.payload
            }),
        ),
        EventType::TaskCompleted => {
            // Memory subsystem lifecycle — not the agent's final completion.
            // The structured completion comes via EXECUTION_COMPLETE.
            (
                "agent_status",
                json!({
                    "agent_id": "system",
                    "status": "task_completed",
                    "detail": event.payload,
                }),
            )
        }
        EventType::TaskFailed => {
            (
                "agent_status",
                json!({
                    "agent_id": "system",
                    "status": "task_failed",
                    "detail": event.payload,
                }),
            )
        }
        // --- Phase / Agent lifecycle events (from ExecutionEventEmitter) ---
        EventType::Custom(ref name) if name == "PHASE_CHANGE" => {
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let pc = payload.get("event").and_then(|e| e.get("PhaseChange"));
            (
                "phase",
                json!({
                    "from_phase": pc.and_then(|p| p.get("from_phase")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "to_phase": pc.and_then(|p| p.get("to_phase")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "agent_role": pc.and_then(|p| p.get("agent_role")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "reason": pc.and_then(|p| p.get("reason")).and_then(|v| v.as_str()).unwrap_or(""),
                }),
            )
        }
        EventType::Custom(ref name) if name == "AGENT_STATUS" => {
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let as_ = payload.get("event").and_then(|e| e.get("AgentStatus"));
            (
                "agent_status",
                json!({
                    "agent_id": as_.and_then(|s| s.get("agent_id")).and_then(|v| v.as_str()).unwrap_or(&event.source_agent_iri),
                    "role": as_.and_then(|s| s.get("role")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "status": as_.and_then(|s| s.get("status")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "turn": as_.and_then(|s| s.get("turn")).and_then(|v| v.as_u64()).unwrap_or(0),
                    "iteration": as_.and_then(|s| s.get("iteration")).and_then(|v| v.as_u64()).unwrap_or(0),
                }),
            )
        }
        // --- Thought/Reasoning events (from SA/AgentRunner ReAct loop) ---
        EventType::Custom(ref name) if name == "THOUGHT" => {
            // Payload is ExecutionEvent { event: { Thought: { thought, action, … } } }
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let thought_obj = payload.get("event").and_then(|e| e.get("Thought"));
            let content = thought_obj
                .and_then(|t| t.get("thought"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let agent_id = thought_obj
                .and_then(|t| t.get("agent_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(&event.source_agent_iri);
            let action = thought_obj
                .and_then(|t| t.get("action"))
                .and_then(|v| v.as_str())
                .unwrap_or("continue");
            (
                "reasoning",
                json!({
                    "content": content,
                    "agent_id": agent_id,
                    "action": action,
                }),
            )
        }
        EventType::Custom(ref name) if name == "TOOL_CALL" => {
            // Payload is ExecutionEvent { event: { ToolCall: { tool_name, arguments_json, … } } }
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let tc = payload.get("event").and_then(|e| e.get("ToolCall"));
            let tool_name = tc
                .and_then(|t| t.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let call_id = tc
                .and_then(|t| t.get("call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args_raw = tc
                .and_then(|t| t.get("arguments_json"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let args: Value = serde_json::from_str(args_raw).unwrap_or(Value::Null);
            let agent_id = tc
                .and_then(|t| t.get("agent_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(&event.source_agent_iri);
            (
                "tool_call",
                json!({
                    "tool": tool_name,
                    "call_id": call_id,
                    "agent_id": agent_id,
                    "arguments": args,
                }),
            )
        }
        EventType::Custom(ref name) if name == "TOOL_RESULT" => {
            // Payload is ExecutionEvent { event: { ToolResult: { tool_name, result, … } } }
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let tr = payload.get("event").and_then(|e| e.get("ToolResult"));
            let tool_name = tr
                .and_then(|t| t.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let call_id = tr
                .and_then(|t| t.get("call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result_raw = tr
                .and_then(|t| t.get("result"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result_val: Value = serde_json::from_str(result_raw).unwrap_or(Value::Null);
            let success = tr
                .and_then(|t| t.get("success"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let result_size = tr
                .and_then(|t| t.get("result_size_bytes"))
                .and_then(|v| v.as_u64())
                .unwrap_or(result_raw.len() as u64);
            let agent_id = tr
                .and_then(|t| t.get("agent_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(&event.source_agent_iri);
            // Inline a preview of the routed/truncated result alongside the
            // structured `result` field so SSE consumers can still inspect
            // tool output even when the router redirects the full payload to
            // a micro-tool / KG node and the parsed JSON would otherwise be
            // null. Preview is the first 256 chars; top-level keys are
            // included when the raw string happens to parse as JSON.
            let result_preview: String = result_raw.chars().take(256).collect();
            let result_top_keys: Vec<String> = match &result_val {
                Value::Object(map) => map.keys().cloned().collect(),
                Value::Array(arr) => arr
                    .first()
                    .and_then(|first| first.as_object())
                    .map(|obj| obj.keys().cloned().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            (
                "tool_call",
                json!({
                    "tool": tool_name,
                    "call_id": call_id,
                    "agent_id": agent_id,
                    "result": result_val,
                    "result_preview": result_preview,
                    "result_top_keys": result_top_keys,
                    "result_size_bytes": result_size,
                    "success": success,
                    "is_result": true,
                }),
            )
        }
        EventType::Custom(ref name) if name == "EXECUTION_COMPLETE" => {
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let comp = payload.get("event").and_then(|e| e.get("Completion"));
            let status = comp
                .and_then(|c| c.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("success");
            let summary = comp
                .and_then(|c| c.get("summary"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let total_turns = comp
                .and_then(|c| c.get("total_turns"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let total_tool_calls = comp
                .and_then(|c| c.get("total_tool_calls"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let total_tokens = comp
                .and_then(|c| c.get("total_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (
                "completion",
                json!({
                    "status": status,
                    "summary": summary,
                    "total_turns": total_turns,
                    "total_tool_calls": total_tool_calls,
                    "total_tokens": total_tokens,
                }),
            )
        }
        EventType::Custom(ref name) if name == "EXECUTION_ERROR" => {
            let payload: Value = serde_json::from_str(&event.payload)
                .unwrap_or_default();
            let err = payload.get("event").and_then(|e| e.get("Error"));
            (
                "error",
                json!({
                    "agent_id": err.and_then(|e| e.get("agent_id")).and_then(|v| v.as_str()).unwrap_or(&event.source_agent_iri),
                    "error_type": err.and_then(|e| e.get("error_type")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "message": err.and_then(|e| e.get("message")).and_then(|v| v.as_str()).unwrap_or(&event.payload),
                }),
            )
        }
        _ => return None,
    };

    Some(
        Event::default()
            .event(event_name)
            .data(data.to_string())
    )
}
