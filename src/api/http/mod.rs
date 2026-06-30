use std::collections::HashMap;
use std::sync::Arc;
use std::convert::Infallible;

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::core_types::SemanticCore;
use crate::gateway::unified_gateway::{ChatMessage, UnifiedGateway};
use crate::knowledge_graph::rdf_mapper::RdfMapper;
use crate::knowledge_graph::store::KnowledgeGraphStore;
use crate::knowledge_graph::types::{EdgeDef, LLMExtractionOutput, NodeDef};
use crate::tools::tool_guard::{GuardAuditEntry, GUARD_AUDIT_LOG};
use crate::tools::prompt_registry::{PromptRegistry, PromptVersion};
use crate::tools::skill_registry::SkillMeta;

pub mod iam;
use iam::UserIdentity;

pub struct AppState {
    pub core: Arc<SemanticCore>,
    pub gateway: Arc<UnifiedGateway>,
    pub kg_store: Arc<oxigraph::store::Store>,
    /// 已脱敏的运行期配置快照（不含 api_key 明文），支持前端 PUT 写回并持久化到
    /// data/config_override.json（重启后由 Settings::load() 读取生效）。
    pub config_info: Arc<tokio::sync::RwLock<Value>>,
    /// 批处理 Agent 列表（静态，来自启动配置）。
    pub agents_info: Value,
    /// MCP 服务器注册表（运行期动态写入）。
    pub mcp_servers: Arc<tokio::sync::RwLock<Vec<Value>>>,
    /// 用户态 Agent 注册表（运行期可增删改，持久化到 data/agents.json）。
    pub user_agents: Arc<tokio::sync::RwLock<Vec<Value>>>,
    /// Prompt/模型灰度版本注册表（G6'）。
    pub prompts: Arc<PromptRegistry>,
    /// 知识库分类注册表（运行期可增删改，持久化到 data/kb_categories.json）。
    pub kb_categories: Arc<tokio::sync::RwLock<Vec<Value>>>,
    /// 知识库注册表（向量/图，运行期可增删，持久化到 data/knowledge_bases.json）。
    pub knowledge_bases: Arc<tokio::sync::RwLock<Vec<Value>>>,
}

/// 持久化数据目录；可由 AGENTOS_DATA_DIR 覆盖（便于测试隔离），缺省为 "data"。
fn data_dir() -> std::path::PathBuf {
    std::env::var("AGENTOS_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data"))
}

/// 用户态 Agent 的持久化文件路径。
fn agents_store_path() -> std::path::PathBuf {
    data_dir().join("agents.json")
}

/// 启动时从磁盘加载用户态 Agent；文件不存在或解析失败时返回空列表。
fn load_user_agents() -> Vec<Value> {
    match std::fs::read_to_string(agents_store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 将用户态 Agent 持久化到磁盘（pretty JSON）。
fn save_user_agents(agents: &[Value]) -> std::io::Result<()> {
    let path = agents_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(agents).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(&path, content)
}

/// MCP 服务器注册表的持久化文件路径。
fn mcp_servers_store_path() -> std::path::PathBuf {
    data_dir().join("mcp_servers.json")
}

/// 启动时从磁盘加载已注册的 MCP 服务器；文件不存在或解析失败时返回空列表。
fn load_mcp_servers() -> Vec<Value> {
    match std::fs::read_to_string(mcp_servers_store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 将 MCP 服务器注册表持久化到磁盘（pretty JSON）。
fn save_mcp_servers(servers: &[Value]) -> std::io::Result<()> {
    let path = mcp_servers_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(servers).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(&path, content)
}

/// 用户态注册技能的持久化文件路径（仅 POST 注册的技能，不含启动播种的默认技能）。
fn skills_store_path() -> std::path::PathBuf {
    data_dir().join("skills.json")
}

/// 启动时从磁盘加载用户态注册的技能；文件不存在或解析失败时返回空列表。
fn load_user_skills() -> Vec<SkillMeta> {
    match std::fs::read_to_string(skills_store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 以 skill_iri 为主键 upsert 一条用户态技能并持久化（pretty JSON）。
fn save_user_skill(skill: &SkillMeta) -> std::io::Result<()> {
    let mut skills = load_user_skills();
    match skills.iter_mut().find(|s| s.skill_iri == skill.skill_iri) {
        Some(existing) => *existing = skill.clone(),
        None => skills.push(skill.clone()),
    }
    let path = skills_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&skills).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(&path, content)
}

/// Prompt 版本注册表的持久化文件路径。
fn prompts_store_path() -> std::path::PathBuf {
    data_dir().join("prompts.json")
}

/// 知识库分类的持久化文件路径。
fn kb_categories_store_path() -> std::path::PathBuf {
    data_dir().join("kb_categories.json")
}

/// 启动时从磁盘加载知识库分类；文件不存在或解析失败时返回空列表。
fn load_kb_categories() -> Vec<Value> {
    match std::fs::read_to_string(kb_categories_store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 将知识库分类持久化到磁盘（pretty JSON）。
fn save_kb_categories(categories: &[Value]) -> std::io::Result<()> {
    let path = kb_categories_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(categories).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(&path, content)
}

/// 知识库注册表的持久化文件路径。
fn knowledge_bases_store_path() -> std::path::PathBuf {
    data_dir().join("knowledge_bases.json")
}

/// 启动时从磁盘加载知识库；文件不存在或解析失败时返回空列表。
fn load_knowledge_bases() -> Vec<Value> {
    match std::fs::read_to_string(knowledge_bases_store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 将知识库持久化到磁盘（pretty JSON）。
fn save_knowledge_bases(bases: &[Value]) -> std::io::Result<()> {
    let path = knowledge_bases_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(bases).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(&path, content)
}

/// 运行期配置覆盖文件路径；由 PUT /api/v1/config 写入，启动时被 Settings::load() 作为
/// 高于 config.yaml 的来源读取。路径与 Settings::load 中的 "data/config_override" 保持一致。
fn config_override_path() -> std::path::PathBuf {
    std::path::PathBuf::from("data/config_override.json")
}

/// 将网关配置持久化到运行期覆盖文件，重启后由 Settings::load() 生效。
/// 将持久化所有字段（包括 api_key），保留覆盖文件其余段落。
fn save_config_override(patch: &Value) -> std::io::Result<()> {
    let path = config_override_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .unwrap_or_else(|| json!({}));

    if let Some(gateway_patch) = patch.get("gateway").and_then(|v| v.as_object()) {
        let mut clean = gateway_patch.clone();
        // api_key_configured 仅用于前端展示，不是 GatewaySettings 字段。
        clean.remove("api_key_configured");

        if let Some(obj) = root.as_object_mut() {
            let existing_gateway = obj.entry("gateway").or_insert(json!({}));
            if let Some(existing_gw_obj) = existing_gateway.as_object_mut() {
                for (k, v) in clean {
                    existing_gw_obj.insert(k, v);
                }
            }
        }
    }
    let content = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&path, content)
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Deserialize)]
pub struct TaskRequest {
    pub user_input: String,
    /// 用户态标识，用于会话隔离（可选，缺省为匿名）。
    pub user_id: Option<String>,
    /// 会话标识，用于多轮上下文隔离（可选）。
    pub session_id: Option<String>,
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
    gateway: Arc<UnifiedGateway>,
    kg_store: Arc<oxigraph::store::Store>,
    config_info: Value,
    agents_info: Value,
) -> Router {
    // 启动时加载用户态注册的技能并重新注册到内存技能表（默认技能由 SemanticCore 播种）。
    for skill in load_user_skills() {
        core.skills.register_skill(skill);
    }

    let state = Arc::new(AppState {
        core,
        gateway,
        kg_store,
        config_info: Arc::new(tokio::sync::RwLock::new(config_info)),
        agents_info,
        mcp_servers: Arc::new(tokio::sync::RwLock::new(load_mcp_servers())),
        user_agents: Arc::new(tokio::sync::RwLock::new(load_user_agents())),
        prompts: Arc::new(PromptRegistry::load(prompts_store_path())),
        kb_categories: Arc::new(tokio::sync::RwLock::new(load_kb_categories())),
        knowledge_bases: Arc::new(tokio::sync::RwLock::new(load_knowledge_bases())),
    });

    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/config", get(config_handler).put(update_config_handler))
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
        .route("/api/v1/skills", get(list_skills_handler).post(register_skill_handler))
        .route("/api/v1/guard/audit", get(guard_audit_handler))
        .route("/api/v1/guard/stats", get(guard_stats_handler))
        .route("/api/v1/kg/import", post(kg_import_handler))
        .route("/api/v1/kg/query", post(kg_query_handler))
        // ── 本体层（Ontology Layer）只读元数据 ──
        .route("/api/v1/knowledge-packs", get(list_knowledge_packs_handler))
        .route("/api/v1/ontology/types", get(ontology_types_handler))
        .route("/api/v1/ontology/actions/:id/invoke", post(invoke_action_handler))
        // ── 知识库分类管理 CRUD ──
        .route("/api/v1/kb/categories", get(list_kb_categories_handler).post(create_kb_category_handler))
        .route("/api/v1/kb/categories/:id", put(update_kb_category_handler).delete(delete_kb_category_handler))
        // ── 知识库（向量/图）管理 ──
        .route("/api/v1/kb/bases", get(list_knowledge_bases_handler).post(create_knowledge_base_handler))
        .route("/api/v1/kb/bases/:id", delete(delete_knowledge_base_handler))
        .route("/api/v1/agents", get(list_agents_handler).post(create_agent_handler))
        .route("/api/v1/agents/:id", put(update_agent_handler).delete(delete_agent_handler))
        .route("/api/v1/agents/:id/graph", post(bind_agent_graph_handler))
        .route("/api/v1/agents/:id/chat", post(agent_chat_handler))
        .route("/api/v1/mcp/servers", get(list_mcp_servers_handler).post(register_mcp_server_handler))
        // ── G6' Prompt/模型灰度版本管理 ──
        .route("/api/v1/prompts", get(list_prompts_handler).post(create_prompt_handler))
        .route("/api/v1/prompts/resolve", get(resolve_prompt_handler))
        .route("/api/v1/prompts/:id/activate", post(activate_prompt_handler))
        .route("/api/v1/prompts/:id/canary", put(canary_prompt_handler))
        .route("/api/v1/prompts/:id", delete(delete_prompt_handler))
        .with_state(state)
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

async fn config_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let info = state.config_info.read().await.clone();
    Json(info)
}

/// PUT /api/v1/config — 更新运行期配置并持久化到 data/config_override.json（重启后由 Settings 生效）
/// Body: { "gateway": { "base_url": "...", "api_key": "...", "default_model": "...", ... } }
async fn update_config_handler(
    State(state): State<Arc<AppState>>,
    Json(patch): Json<Value>,
) -> impl IntoResponse {
    // 1. 运行时更新 Gateway 服务
    if let Some(gw_patch) = patch.get("gateway").and_then(|v| v.as_object()) {
        if let Some(base_url) = gw_patch.get("base_url").and_then(|v| v.as_str()) {
            state.gateway.set_base_url(base_url.to_string());
        }
        if let Some(api_key) = gw_patch.get("api_key").and_then(|v| v.as_str()) {
            state.gateway.set_api_key(api_key.to_string());
        }
        if let Some(default_model) = gw_patch.get("default_model").and_then(|v| v.as_str()) {
            state.gateway.set_default_model(default_model.to_string());
        }
        if let Some(mapping) = gw_patch.get("model_mapping").and_then(|v| v.as_object()) {
            for (k, v) in mapping {
                if let Some(m) = v.as_str() {
                    state.gateway.set_model_mapping(k.clone(), m.to_string());
                }
            }
        }
    }

    // 2. 持久化网关配置（包含 api_key）
    let persisted = save_config_override(&patch).is_ok();

    // 3. 更新已脱敏的运行期快照供前端展示
    {
        let mut info = state.config_info.write().await;
        if let Some(gw_patch) = patch.get("gateway").and_then(|v| v.as_object()) {
            if let Some(obj) = info.as_object_mut() {
                let gateway = obj.entry("gateway").or_insert(json!({}));
                if let Some(gateway_obj) = gateway.as_object_mut() {
                    for (k, v) in gw_patch {
                        if k == "api_key" {
                            gateway_obj.insert(
                                "api_key_configured".into(),
                                json!(!v.as_str().unwrap_or("").is_empty()),
                            );
                        } else {
                            gateway_obj.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
    }

    let final_info = state.config_info.read().await.clone();
    Json(json!({
        "status": "ok",
        "message": if persisted {
            "配置已更新并持久化生效。"
        } else {
            "配置已在运行时更新，但持久化失败。"
        },
        "persisted": persisted,
        "config": final_info,
    }))
}

/// GET /api/v1/agents — 返回批处理 Agent（静态）与用户态 Agent（持久化）合并列表
async fn list_agents_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut agents: Vec<Value> = state
        .agents_info
        .get("agents")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let batch_count = agents.len();
    let user_agents = state.user_agents.read().await.clone();
    let user_count = user_agents.len();
    agents.extend(user_agents);
    Json(json!({
        "count": agents.len(),
        "batch_count": batch_count,
        "user_count": user_count,
        "agents": agents,
    }))
}

#[derive(Deserialize)]
pub struct AgentCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub business_domain: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    pub knowledge_graph: Option<String>,
    pub enabled: Option<bool>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

/// POST /api/v1/agents — 创建用户态 Agent 并持久化
async fn create_agent_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentCreateRequest>,
) -> impl IntoResponse {
    let agent = json!({
        "id": uuid::Uuid::new_v4().hyphenated().to_string(),
        "name": req.name,
        "description": req.description.unwrap_or_default(),
        "business_domain": req.business_domain.unwrap_or_default(),
        "skills": req.skills,
        "knowledge_graph": req.knowledge_graph.unwrap_or_default(),
        "enabled": req.enabled.unwrap_or(true),
        "icon": req.icon.unwrap_or_else(|| "Bot".to_string()),
        "color": req.color.unwrap_or_else(|| "bg-blue-500".to_string()),
        "source": "user",
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    let id = agent["id"].as_str().unwrap_or("").to_string();
    let mut guard = state.user_agents.write().await;
    guard.push(agent.clone());
    let _ = save_user_agents(&guard);
    (StatusCode::CREATED, Json(json!({ "id": id, "status": "created", "agent": agent })))
}

/// PUT /api/v1/agents/:id — 更新用户态 Agent 并持久化
async fn update_agent_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(patch): Json<Value>,
) -> impl IntoResponse {
    let mut guard = state.user_agents.write().await;
    let found = guard
        .iter_mut()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
    match found {
        Some(agent) => {
            if let (Some(obj), Some(patch_obj)) = (agent.as_object_mut(), patch.as_object()) {
                for (k, v) in patch_obj {
                    if k == "id" || k == "source" || k == "created_at" {
                        continue;
                    }
                    obj.insert(k.clone(), v.clone());
                }
                obj.insert("updated_at".into(), json!(chrono::Utc::now().to_rfc3339()));
            }
            let updated = agent.clone();
            let _ = save_user_agents(&guard);
            (StatusCode::OK, Json(json!({ "status": "updated", "agent": updated })))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found", "id": id })),
        ),
    }
}

/// DELETE /api/v1/agents/:id — 删除用户态 Agent 并持久化
async fn delete_agent_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut guard = state.user_agents.write().await;
    let before = guard.len();
    guard.retain(|a| a.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    if guard.len() == before {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found", "id": id })),
        );
    }
    let _ = save_user_agents(&guard);
    (StatusCode::OK, Json(json!({ "status": "deleted", "id": id })))
}

#[derive(Deserialize)]
pub struct AgentChatRequest {
    pub message: String,
}

/// 仅保留 ASCII 字母/数字/下划线组成、长度≥3 且包含数字或下划线（或长度≥4）的片段，
/// 作为故障码检索 token；可有效命中 APP_w009 / P0A80 等代码而排除普通停用词。
fn extract_code_tokens(message: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if cur.len() >= 3 {
            let has_digit = cur.chars().any(|c| c.is_ascii_digit());
            if has_digit || cur.contains('_') || cur.len() >= 4 {
                out.push(cur.to_lowercase());
            }
        }
        cur.clear();
    };
    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut tokens);
        }
    }
    flush(&mut cur, &mut tokens);
    tokens.dedup();
    tokens
}

/// 将用户问题中的品牌别名映射为图谱中的品牌 label（如 特斯拉→Tesla）。
fn extract_brand_labels(message: &str) -> Vec<String> {
    let lower = message.to_lowercase();
    let mut out = Vec::new();
    let table: [(&[&str], &str); 6] = [
        (&["特斯拉", "tesla"], "Tesla"),
        (&["比亚迪", "byd"], "比亚迪"),
        (&["蔚来", "nio"], "蔚来"),
        (&["小鹏", "xpeng"], "小鹏"),
        (&["理想", "li auto", "lixiang"], "理想"),
        (&["问界", "aito"], "问界"),
    ];
    for (aliases, label) in table {
        if aliases.iter().any(|a| message.contains(*a) || lower.contains(&a.to_lowercase())) {
            out.push(label.to_string());
        }
    }
    out
}

const ONT_FAULT: &str = "http://aps.local/ontology/FaultCode";
const ONT_BRAND_REL: &str = "http://aps.local/ontology/belongsToBrand";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const META: &str = "https://agentos.ontology/meta";

/// 构造检索 FaultCode 的 SPARQL；filter_expr 为已组装好的 FILTER 条件表达式。
fn build_fault_query(filter_expr: &str, limit: usize) -> String {
    format!(
        "SELECT ?code ?label ?meaning ?can_drive ?repair ?models ?brand WHERE {{ \
            ?n a <{f}> . \
            ?n <{m}/code> ?code . \
            OPTIONAL {{ ?n <{rl}> ?label }} \
            OPTIONAL {{ ?n <{m}/meaning> ?meaning }} \
            OPTIONAL {{ ?n <{m}/can_drive> ?can_drive }} \
            OPTIONAL {{ ?n <{m}/repair> ?repair }} \
            OPTIONAL {{ ?n <{m}/models> ?models }} \
            OPTIONAL {{ ?n <{br}> ?bn . ?bn <{rl}> ?brand }} \
            FILTER( {flt} ) \
        }} LIMIT {lim}",
        f = ONT_FAULT, m = META, rl = RDFS_LABEL, br = ONT_BRAND_REL,
        flt = filter_expr, lim = limit,
    )
}

fn trunc(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n { t.to_string() } else { t.chars().take(n).collect::<String>() + "…" }
}

/// POST /api/v1/agents/:id/chat — 基于该 Agent 绑定知识图谱的检索增强问答（RAG）。
/// 流程：定位 Agent → 抽取故障码/品牌 token → SPARQL 检索 FaultCode 事实 →
/// 组装为上下文交由 LLM 网关生成简体中文回答 → 返回 answer 与 sources。
async fn agent_chat_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<AgentChatRequest>,
) -> impl IntoResponse {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "message 不能为空" })));
    }

    // 1. 定位 Agent（用户态优先，其次批处理静态）。
    let agent = {
        let guard = state.user_agents.read().await;
        guard
            .iter()
            .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
            .cloned()
            .or_else(|| {
                state
                    .agents_info
                    .get("agents")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        arr.iter()
                            .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
                            .cloned()
                    })
            })
    };
    let agent = match agent {
        Some(a) => a,
        None => {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": "agent not found", "id": id })))
        }
    };
    let agent_name = agent.get("name").and_then(|v| v.as_str()).unwrap_or("维修助手").to_string();
    let graph = agent
        .get("knowledge_graph")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| expand_iri(s));

    // 2. 知识图谱检索（仅当绑定了图谱时）。
    let mut rows: Vec<Value> = Vec::new();
    if let Some(graph_iri) = graph.as_deref() {
        let kg = state.kg_store.clone();
        if let Ok(store) = KnowledgeGraphStore::with_shared_store(kg) {
            let codes = extract_code_tokens(&message);
            let brands = extract_brand_labels(&message);
            // 优先按故障码精确片段命中。
            if !codes.is_empty() {
                let conds: Vec<String> = codes
                    .iter()
                    .map(|t| format!("CONTAINS(LCASE(STR(?code)), \"{}\")", t))
                    .collect();
                let q = build_fault_query(&conds.join(" || "), 6);
                rows = store.query_sparql(&q, Some(graph_iri)).unwrap_or_default();
            }
            // 故障码无命中时，按品牌召回若干代表性故障码。
            if rows.is_empty() && !brands.is_empty() {
                let conds: Vec<String> = brands
                    .iter()
                    .map(|b| format!("CONTAINS(STR(?brand), \"{}\")", b.replace('"', "")))
                    .collect();
                let q = format!(
                    "SELECT ?code ?label ?meaning ?can_drive ?repair ?models ?brand WHERE {{ \
                        ?n a <{f}> . \
                        ?n <{m}/code> ?code . \
                        ?n <{br}> ?bn . ?bn <{rl}> ?brand . \
                        OPTIONAL {{ ?n <{rl}> ?label }} \
                        OPTIONAL {{ ?n <{m}/meaning> ?meaning }} \
                        OPTIONAL {{ ?n <{m}/can_drive> ?can_drive }} \
                        OPTIONAL {{ ?n <{m}/repair> ?repair }} \
                        OPTIONAL {{ ?n <{m}/models> ?models }} \
                        FILTER( {flt} ) \
                    }} LIMIT 6",
                    f = ONT_FAULT, m = META, rl = RDFS_LABEL, br = ONT_BRAND_REL,
                    flt = conds.join(" || "),
                );
                rows = store.query_sparql(&q, Some(graph_iri)).unwrap_or_default();
            }
        }
    }

    // 3. 组装检索事实上下文。
    let get = |row: &Value, k: &str| row.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut facts = String::new();
    let mut sources: Vec<Value> = Vec::new();
    for row in &rows {
        let code = get(row, "?code");
        let label = get(row, "?label");
        let brand = get(row, "?brand");
        facts.push_str(&format!(
            "- 故障码 {code}（{brand}）：{label}\n  含义：{}\n  能否行驶：{}\n  维修建议：{}\n  适用车型：{}\n",
            trunc(&get(row, "?meaning"), 300),
            trunc(&get(row, "?can_drive"), 200),
            trunc(&get(row, "?repair"), 300),
            trunc(&get(row, "?models"), 160),
        ));
        sources.push(json!({ "code": code, "label": label, "brand": brand }));
    }

    // 4. 构造提示并调用 LLM 网关。
    let sys = format!(
        "你是「{agent_name}」，一名专业的新能源汽车故障诊断与维修助手。请严格依据下方“知识图谱检索结果”，\
用简体中文回答用户问题：解释故障含义、是否可继续行驶、维修建议与适用车型。\
若检索结果为空或不足以支撑回答，请如实说明并给出通用排查建议，切勿编造具体故障码信息。\
回答需专业、严谨、条理清晰。"
    );
    let user_content = if facts.is_empty() {
        format!("【知识图谱检索结果】\n（未检索到相关故障码记录）\n\n【用户问题】\n{message}")
    } else {
        format!("【知识图谱检索结果】\n{facts}\n【用户问题】\n{message}")
    };
    let messages = vec![
        ChatMessage { role: "system".into(), content: sys, name: None, tool_calls: None, tool_call_id: None, reasoning_content: None },
        ChatMessage { role: "user".into(), content: user_content, name: None, tool_calls: None, tool_call_id: None, reasoning_content: None },
    ];

    match state.gateway.chat(messages).await {
        Ok(resp) => {
            let answer = resp
                .choices
                .first()
                .and_then(|c| c.message.content.clone())
                .unwrap_or_default();
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "answer": answer,
                    "grounded": !rows.is_empty(),
                    "sources": sources,
                    "retrieved": rows.len(),
                    "model": state.gateway.default_model(),
                })),
            )
        }
        Err(e) => {
            // 网关失败但已检索到事实时，回退为基于图谱的确定性回答，保证可用性。
            if let Some(row) = rows.first() {
                let fallback = format!(
                    "【基于知识图谱的检索结果】\n故障码 {}（{}）：{}\n含义：{}\n能否行驶：{}\n维修建议：{}\n适用车型：{}",
                    get(row, "?code"), get(row, "?brand"), get(row, "?label"),
                    get(row, "?meaning"), get(row, "?can_drive"), get(row, "?repair"), get(row, "?models"),
                );
                (
                    StatusCode::OK,
                    Json(json!({
                        "status": "degraded",
                        "answer": fallback,
                        "grounded": true,
                        "sources": sources,
                        "retrieved": rows.len(),
                        "warning": format!("LLM 网关不可用，已回退为图谱直出：{}", e),
                    })),
                )
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("LLM 网关调用失败：{}", e) })),
                )
            }
        }
    }
}

/// GET /api/v1/mcp/servers — 返回已注册的 MCP 服务器
async fn list_mcp_servers_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let servers = state.mcp_servers.read().await.clone();
    Json(json!({ "count": servers.len(), "servers": servers }))
}

#[derive(Deserialize)]
pub struct McpServerRegisterRequest {
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
    pub protocol: Option<String>,
}

/// POST /api/v1/mcp/servers — 注册新的 MCP 服务器
async fn register_mcp_server_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<McpServerRegisterRequest>,
) -> impl IntoResponse {
    let server = json!({
        "id": uuid::Uuid::new_v4().hyphenated().to_string(),
        "name": req.name,
        "description": req.description.unwrap_or_default(),
        "endpoint": req.endpoint,
        "protocol": req.protocol.unwrap_or_else(|| "sse".to_string()),
        "status": "active",
    });
    let id = server["id"].as_str().unwrap_or("").to_string();
    let mut guard = state.mcp_servers.write().await;
    guard.push(server);
    let _ = save_mcp_servers(&guard);
    (StatusCode::CREATED, Json(json!({ "id": id, "status": "registered" })))
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
    match state
        .core
        .init_task(
            &req.user_input,
            None,
            None,
            req.user_id.as_deref(),
            req.session_id.as_deref(),
        )
        .await
    {
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

                    if event.event_type == "TASK_COMPLETED" || event.event_type == "TASK_FAILED" {
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
    State(state): State<Arc<AppState>>,
    axum::extract::Path(task_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Read the task node from L2 blackboard; return 404 when not found.
    match state.core.blackboard.read_node(&task_iri) {
        Ok(Some(node)) => {
            // Parse json_ld to extract runtime status fields if present.
            let parsed: Value = serde_json::from_str(&node.json_ld).unwrap_or(Value::Null);
            let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("queued");
            let phase = parsed.get("current_phase").and_then(|v| v.as_str()).unwrap_or("unknown");
            (
                axum::http::StatusCode::OK,
                Json(json!({
                    "task_iri": task_iri,
                    "status": status,
                    "current_phase": phase,
                    "node_type": node.node_type,
                    "tags": node.tags,
                    "created_at": node.created_at,
                    "dirty": node.dirty,
                })),
            ).into_response()
        }
        _ => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found", "task_iri": task_iri })),
        ).into_response(),
    }
}

async fn get_execution_details_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(task_iri): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Read the task node from L2 blackboard; return 404 when not found.
    match state.core.blackboard.read_node(&task_iri) {
        Ok(Some(node)) => {
            let parsed: Value = serde_json::from_str(&node.json_ld).unwrap_or(Value::Null);
            let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("queued");
            let phase = parsed.get("current_phase").and_then(|v| v.as_str()).unwrap_or("unknown");
            let turn = parsed.get("turn").and_then(|v| v.as_i64()).unwrap_or(0);
            // Collect child nodes for this task.
            let child_nodes = state.core.blackboard.get_task_nodes(&task_iri);
            (
                axum::http::StatusCode::OK,
                Json(json!({
                    "task_iri": task_iri,
                    "status": status,
                    "current_phase": phase,
                    "node_type": node.node_type,
                    "tags": node.tags,
                    "created_at": node.created_at,
                    "child_nodes": child_nodes,
                    "steps": [],
                    "agent_sessions": [],
                    "stats": {
                        "total_turns": turn,
                        "total_tool_calls": 0,
                        "total_tokens": 0,
                    },
                })),
            ).into_response()
        }
        _ => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found", "task_iri": task_iri })),
        ).into_response(),
    }
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
    let trusted_key_count = state.core.skills.trusted_key_count();
    let enriched: Vec<Value> = skills.iter().map(|s| {
        let status = state.core.skills.verify_skill_signature(s);
        let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("signature_status".into(), json!(status.as_str()));
        }
        v
    }).collect();
    Json(json!({
        "count": enriched.len(),
        "trusted_key_count": trusted_key_count,
        "skills": enriched,
    }))
}

/// POST /api/v1/skills — 注册新技能（G7：仅 DA 角色可用）
async fn register_skill_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    Json(skill): Json<SkillMeta>,
) -> impl IntoResponse {
    // G7：严格模式下要求 DA 角色
    if let Err(e) = identity.require_role("DA") {
        return e.into_response();
    }
    if skill.skill_iri.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "status": "error", "error": "skill_iri 不能为空"
        }))).into_response();
    }
    let sig_status = state.core.skills.verify_skill_signature(&skill);
    use crate::tools::skill_registry::SignatureStatus;
    if sig_status == SignatureStatus::Invalid {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "status": "error", "error": "签名校验失败，技能被拒绝注册",
            "signature_status": "invalid",
        }))).into_response();
    }
    let iri = skill.skill_iri.clone();
    let _ = save_user_skill(&skill);
    state.core.skills.register_skill(skill);
    (StatusCode::CREATED, Json(json!({
        "status": "ok",
        "skill_iri": iri,
        "signature_status": sig_status.as_str(),
        "registered_by": identity.user_id,
        "tenant_id": identity.tenant_id,
    }))).into_response()
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
    Json(req): Json<KgImportRequest>,
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

// ─── 知识库分类管理 CRUD ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct KbCategoryCreateRequest {
    pub name: String,
    pub description: Option<String>,
}

/// GET /api/v1/knowledge-packs — 返回系统内置的知识包清单（含本体统计摘要）
///
/// 本体层只读元数据：每个知识包封装独立的 RDF 命名图与向量命名空间，支持 Agent 挂载与包间隔离。
async fn list_knowledge_packs_handler() -> impl IntoResponse {
    let packs = crate::knowledge_graph::ontology_layer::knowledge_packs();
    Json(json!({ "count": packs.len(), "knowledge_packs": packs }))
}

/// GET /api/v1/ontology/types — 返回新能源车维修域本体定义（对象/链接/动作/函数）
///
/// 语义层（ObjectType/LinkType）+ 动力层（ActionType/FunctionDef）的完整元模型。
async fn ontology_types_handler() -> impl IntoResponse {
    let ont = crate::knowledge_graph::ontology_layer::ev_repair_ontology();
    Json(json!({
        "domain": ont.domain,
        "counts": {
            "object_types": ont.object_types.len(),
            "link_types": ont.link_types.len(),
            "action_types": ont.action_types.len(),
            "functions": ont.functions.len(),
        },
        "object_types": ont.object_types,
        "link_types": ont.link_types,
        "action_types": ont.action_types,
        "functions": ont.functions,
    }))
}

// ─── 动力层执行器（ActionType invoke）──────────────────────────────────
//
// 让知识图谱从"只读"变为"可写可执行"：依据 ActionType 做参数校验 + 前置条件检查，
// 再把 side-effect 以 SPARQL 写回新能源车维修知识包的命名图（graph:pack/ev-repair）。

/// 新能源车维修知识包的命名图（写回隔离单元）。
const EV_PACK_GRAPH: &str = "graph:pack/ev-repair";
const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";

/// 本体实例 IRI：https://agentos.ontology/ev/{ObjectType}/{key}
fn ev_instance_iri(obj_type: &str, key: &str) -> String {
    format!("https://agentos.ontology/ev/{}/{}", obj_type, iri_safe(key))
}
/// 对象类型 / 链接类型 IRI（与 ontology_layer 的 ev() 一致）。
fn ev_term_iri(name: &str) -> String {
    format!("https://agentos.ontology/ev/{}", name)
}
/// 属性谓词 IRI。
fn ev_prop_iri(name: &str) -> String {
    format!("https://agentos.ontology/ev/prop/{}", name)
}
/// 主键值转 IRI 安全片段。
fn iri_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\') { '_' } else { c })
        .collect()
}

/// 文本字面量项（含转义引号）。
fn lit(s: &str) -> String { format!("\"{}\"", sparql_literal(s)) }
/// 十进制数值字面量项。
fn lit_decimal(n: f64) -> String { format!("\"{}\"^^<{}>", n, XSD_DECIMAL) }

/// 属性 upsert：先删旧值再写新值（idempotent）。obj 为完整对象项。
fn upsert_prop_stmts(subject: &str, prop: &str, obj: &str) -> Vec<String> {
    vec![
        format!("DELETE WHERE {{ GRAPH <{g}> {{ <{s}> <{p}> ?old }} }}", g = EV_PACK_GRAPH, s = subject, p = prop),
        format!("INSERT DATA {{ GRAPH <{g}> {{ <{s}> <{p}> {o} }} }}", g = EV_PACK_GRAPH, s = subject, p = prop, o = obj),
    ]
}

/// 命名图内对象是否存在（前置条件检查）。
fn ev_object_exists(kg: &KnowledgeGraphStore, iri: &str) -> bool {
    let q = format!(
        "SELECT ?o WHERE {{ GRAPH <{g}> {{ <{iri}> ?p ?o }} }} LIMIT 1",
        g = EV_PACK_GRAPH, iri = iri,
    );
    kg.query_sparql(&q, None).map(|r| !r.is_empty()).unwrap_or(false)
}

fn p_str(params: &serde_json::Map<String, Value>, name: &str) -> Option<String> {
    match params.get(name) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}
fn p_num(params: &serde_json::Map<String, Value>, name: &str) -> Option<f64> {
    match params.get(name) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct ActionInvokeRequest {
    /// applies_to 对象实例的主键值（动作作用的目标对象）。
    #[serde(default)]
    pub target: Option<String>,
    /// 动作参数（name → value）。
    #[serde(default)]
    pub params: serde_json::Map<String, Value>,
    /// 仅校验并返回将执行的 SPARQL，不真正写回。
    #[serde(default)]
    pub dry_run: bool,
}

/// POST /api/v1/ontology/actions/:id/invoke — 动力层执行器
async fn invoke_action_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(action_id): axum::extract::Path<String>,
    Json(req): Json<ActionInvokeRequest>,
) -> impl IntoResponse {
    let ont = crate::knowledge_graph::ontology_layer::ev_repair_ontology();
    let action = match ont.action_types.iter().find(|a| a.id == action_id) {
        Some(a) => a.clone(),
        None => return (StatusCode::NOT_FOUND, Json(json!({ "error": format!("未知动作类型: {}", action_id) }))),
    };

    // 1. 参数校验：必填项存在且非空。
    let missing: Vec<String> = action
        .parameters
        .iter()
        .filter(|p| p.required && p_str(&req.params, &p.name).is_none())
        .map(|p| p.name.clone())
        .collect();
    if !missing.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "缺少必填参数", "missing": missing })));
    }

    let kg = match KnowledgeGraphStore::with_shared_store(state.kg_store.clone()) {
        Ok(kg) => kg,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))),
    };

    // 2. 前置条件 + 3. 组装 side-effect 写回 SPARQL（按动作分派）。
    let now = chrono::Utc::now().to_rfc3339();
    let (statements, result_meta) = match build_action_effects(&action_id, &req, &kg, &now) {
        Ok(v) => v,
        Err((code, msg)) => return (code, Json(json!({ "error": msg }))),
    };

    if req.dry_run {
        return (StatusCode::OK, Json(json!({
            "status": "dry_run",
            "action": action_id,
            "graph": EV_PACK_GRAPH,
            "sparql": statements,
            "result": result_meta,
        })));
    }

    // 4. 执行写回命名图。
    for stmt in &statements {
        if let Err(e) = state.kg_store.update(stmt) {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("写回失败: {e}"), "sparql": stmt })));
        }
    }
    let _ = state.kg_store.flush();

    (StatusCode::OK, Json(json!({
        "status": "ok",
        "action": action_id,
        "graph": EV_PACK_GRAPH,
        "applied": statements.len(),
        "result": result_meta,
    })))
}

/// 按动作类型组装前置条件校验 + side-effect 写回 SPARQL 语句序列。
fn build_action_effects(
    action_id: &str,
    req: &ActionInvokeRequest,
    kg: &KnowledgeGraphStore,
    now: &str,
) -> Result<(Vec<String>, Value), (StatusCode, String)> {
    let g = EV_PACK_GRAPH;
    let bad = |m: String| (StatusCode::BAD_REQUEST, m);
    match action_id {
        // 依据已确诊故障码为车辆创建维修工单，并建立 forVehicle / diagnoses 链接。
        "GenerateRepairOrder" => {
            let fault_code = req.target.clone().ok_or_else(|| bad("缺少 target（故障码主键）".into()))?;
            let vin = p_str(&req.params, "vehicle_vin").unwrap();
            let vehicle_iri = ev_instance_iri("Vehicle", &vin);
            if !ev_object_exists(kg, &vehicle_iri) {
                return Err(bad(format!("前置条件不满足：车辆VIN不存在于图谱 ({vin})")));
            }
            let fault_iri = ev_instance_iri("FaultCode", &fault_code);
            if !ev_object_exists(kg, &fault_iri) {
                return Err(bad(format!("前置条件不满足：故障码未确诊/不存在 ({fault_code})")));
            }
            let order_id = format!("RO-{}", uuid::Uuid::new_v4().hyphenated());
            let order_iri = ev_instance_iri("RepairOrder", &order_id);
            let mut triples = vec![
                format!("<{o}> a <{c}>", o = order_iri, c = ev_term_iri("RepairOrder")),
                format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("order_id"), v = lit(&order_id)),
                format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("vehicle_vin"), v = lit(&vin)),
                format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("fault_code"), v = lit(&fault_code)),
                format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("status"), v = lit("待处理")),
                format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("created_at"), v = lit(now)),
                format!("<{o}> <{l}> <{veh}>", o = order_iri, l = ev_term_iri("forVehicle"), veh = vehicle_iri),
                format!("<{o}> <{l}> <{f}>", o = order_iri, l = ev_term_iri("diagnoses"), f = fault_iri),
                format!("<{o}> <{lbl}> {v}", o = order_iri, lbl = RDFS_LABEL, v = lit(&order_id)),
            ];
            if let Some(a) = p_str(&req.params, "assigned_to") {
                triples.push(format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("assigned_to"), v = lit(&a)));
            }
            if let Some(c) = p_num(&req.params, "estimated_cost") {
                triples.push(format!("<{o}> <{p}> {v}", o = order_iri, p = ev_prop_iri("estimated_cost"), v = lit_decimal(c)));
            }
            let stmt = format!("INSERT DATA {{ GRAPH <{g}> {{ {t} . }} }}", g = g, t = triples.join(" .\n"));
            Ok((vec![stmt], json!({ "order_id": order_id, "order_iri": order_iri, "vehicle": vehicle_iri, "fault_code": fault_iri })))
        }
        // 检测后写回电池 SOH（0-100），并记录更新时间。
        "UpdateBatterySoh" => {
            let battery_id = p_str(&req.params, "battery_id").unwrap();
            let soh = p_num(&req.params, "soh").ok_or_else(|| bad("soh 必须为数值".into()))?;
            if !(0.0..=100.0).contains(&soh) {
                return Err(bad("前置条件不满足：SOH 取值需在 0-100".into()));
            }
            let bat_iri = ev_instance_iri("Battery", &battery_id);
            if !ev_object_exists(kg, &bat_iri) {
                return Err(bad(format!("前置条件不满足：电池对象不存在 ({battery_id})")));
            }
            let mut stmts = upsert_prop_stmts(&bat_iri, &ev_prop_iri("soh"), &lit_decimal(soh));
            stmts.extend(upsert_prop_stmts(&bat_iri, &ev_prop_iri("soh_updated_at"), &lit(now)));
            Ok((stmts, json!({ "battery": bat_iri, "soh": soh })))
        }
        // 对存在批次性缺陷的车型打召回标记。
        "MarkRecall" => {
            let model_id = p_str(&req.params, "model_id").unwrap();
            let reason = p_str(&req.params, "recall_reason").unwrap();
            let model_iri = ev_instance_iri("VehicleModel", &model_id);
            if !ev_object_exists(kg, &model_iri) {
                return Err(bad(format!("前置条件不满足：车型对象不存在 ({model_id})")));
            }
            let mut stmts = upsert_prop_stmts(&model_iri, &ev_prop_iri("recalled"), &lit("true"));
            stmts.extend(upsert_prop_stmts(&model_iri, &ev_prop_iri("recall_reason"), &lit(&reason)));
            stmts.extend(upsert_prop_stmts(&model_iri, &ev_prop_iri("recall_marked_at"), &lit(now)));
            Ok((stmts, json!({ "model": model_iri, "recalled": true, "recall_reason": reason })))
        }
        // 将一次诊断沉淀为 FAQ，挂接到对应故障码。
        "AppendFaq" => {
            let code = req.target.clone().or_else(|| p_str(&req.params, "code")).ok_or_else(|| bad("缺少 target/code（故障码主键）".into()))?;
            let question = p_str(&req.params, "question").unwrap();
            let answer = p_str(&req.params, "answer").unwrap();
            let fault_iri = ev_instance_iri("FaultCode", &code);
            if !ev_object_exists(kg, &fault_iri) {
                return Err(bad(format!("前置条件不满足：故障码对象不存在 ({code})")));
            }
            let faq_id = format!("FAQ-{}", uuid::Uuid::new_v4().hyphenated());
            let faq_iri = ev_instance_iri("FAQ", &faq_id);
            let triples = vec![
                format!("<{f}> a <{c}>", f = faq_iri, c = ev_term_iri("FAQ")),
                format!("<{f}> <{p}> {o}", f = faq_iri, p = ev_prop_iri("faq_id"), o = lit(&faq_id)),
                format!("<{f}> <{p}> {o}", f = faq_iri, p = ev_prop_iri("question"), o = lit(&question)),
                format!("<{f}> <{p}> {o}", f = faq_iri, p = ev_prop_iri("answer"), o = lit(&answer)),
                format!("<{f}> <{lbl}> {o}", f = faq_iri, lbl = RDFS_LABEL, o = lit(&question)),
                format!("<{fc}> <{l}> <{f}>", fc = fault_iri, l = ev_term_iri("relatedFaq"), f = faq_iri),
            ];
            let stmt = format!("INSERT DATA {{ GRAPH <{g}> {{ {t} . }} }}", g = g, t = triples.join(" .\n"));
            Ok((vec![stmt], json!({ "faq_id": faq_id, "faq_iri": faq_iri, "fault_code": fault_iri })))
        }
        _ => Err((StatusCode::NOT_FOUND, format!("动作 {action_id} 暂未实现执行器"))),
    }
}

/// GET /api/v1/kb/categories — 返回全部知识库分类
async fn list_kb_categories_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let categories = state.kb_categories.read().await.clone();
    Json(json!({ "count": categories.len(), "categories": categories }))
}

/// POST /api/v1/kb/categories — 创建知识库分类并持久化
async fn create_kb_category_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<KbCategoryCreateRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name 不能为空" })));
    }
    let category = json!({
        "id": uuid::Uuid::new_v4().hyphenated().to_string(),
        "name": req.name,
        "description": req.description.unwrap_or_default(),
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    let id = category["id"].as_str().unwrap_or("").to_string();
    let mut guard = state.kb_categories.write().await;
    guard.push(category.clone());
    let _ = save_kb_categories(&guard);
    (StatusCode::CREATED, Json(json!({ "id": id, "status": "created", "category": category })))
}

/// PUT /api/v1/kb/categories/:id — 更新知识库分类并持久化
async fn update_kb_category_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(patch): Json<Value>,
) -> impl IntoResponse {
    let mut guard = state.kb_categories.write().await;
    let found = guard
        .iter_mut()
        .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
    match found {
        Some(category) => {
            if let (Some(obj), Some(patch_obj)) = (category.as_object_mut(), patch.as_object()) {
                for (k, v) in patch_obj {
                    if k == "id" || k == "created_at" {
                        continue;
                    }
                    obj.insert(k.clone(), v.clone());
                }
                obj.insert("updated_at".into(), json!(chrono::Utc::now().to_rfc3339()));
            }
            let updated = category.clone();
            let _ = save_kb_categories(&guard);
            (StatusCode::OK, Json(json!({ "status": "updated", "category": updated })))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "category not found", "id": id })),
        ),
    }
}

/// DELETE /api/v1/kb/categories/:id — 删除知识库分类并持久化
async fn delete_kb_category_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut guard = state.kb_categories.write().await;
    let before = guard.len();
    guard.retain(|c| c.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    if guard.len() == before {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "category not found", "id": id })),
        );
    }
    let _ = save_kb_categories(&guard);
    (StatusCode::OK, Json(json!({ "status": "deleted", "id": id })))
}

// ─── 知识库（向量/图）管理 ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct KnowledgeBaseCreateRequest {
    pub name: String,
    pub description: Option<String>,
    /// "vector" | "graph"
    pub kb_type: String,
    pub category_id: Option<String>,
}

/// SPARQL 字面量转义。
fn sparql_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// GET /api/v1/kb/bases — 返回全部知识库
async fn list_knowledge_bases_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let bases = state.knowledge_bases.read().await.clone();
    Json(json!({ "count": bases.len(), "bases": bases }))
}

/// POST /api/v1/kb/bases — 创建知识库（向量/图），图类型在 oxigraph 落盘命名图元数据
async fn create_knowledge_base_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    Json(req): Json<KnowledgeBaseCreateRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name 不能为空" })));
    }
    if req.kb_type != "vector" && req.kb_type != "graph" {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "kb_type 必须为 vector 或 graph" })));
    }
    // 校验分类存在（若指定）
    if let Some(cat_id) = req.category_id.as_deref() {
        let exists = state
            .kb_categories
            .read()
            .await
            .iter()
            .any(|c| c.get("id").and_then(|v| v.as_str()) == Some(cat_id));
        if !exists {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "category_id 不存在", "category_id": cat_id })));
        }
    }

    let kb_id = uuid::Uuid::new_v4().hyphenated().to_string();
    // 图类型：租户隔离命名图 + 落盘元数据三元组（gliding 底座）
    let graph_iri = if req.kb_type == "graph" {
        let iri = format!("tenant:{}/kb/{}", identity.tenant_id, kb_id);
        let insert = format!(
            "INSERT DATA {{ GRAPH <{g}> {{ <{g}> <https://agentos.ontology/meta/kbName> \"{n}\" . <{g}> <https://agentos.ontology/meta/kbType> \"graph\" }} }}",
            g = iri,
            n = sparql_literal(&req.name),
        );
        if let Err(e) = state.kg_store.update(&insert) {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("命名图初始化失败: {e}") })));
        }
        let _ = state.kg_store.flush();
        Some(iri)
    } else {
        None
    };

    let kb = json!({
        "id": kb_id,
        "name": req.name,
        "description": req.description.unwrap_or_default(),
        "kb_type": req.kb_type,
        "category_id": req.category_id.unwrap_or_default(),
        "graph": graph_iri.clone().unwrap_or_default(),
        "tenant_id": identity.tenant_id,
        "created_by": identity.user_id,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    let mut guard = state.knowledge_bases.write().await;
    guard.push(kb.clone());
    let _ = save_knowledge_bases(&guard);
    (StatusCode::CREATED, Json(json!({ "id": kb["id"], "status": "created", "base": kb })))
}

/// DELETE /api/v1/kb/bases/:id — 删除知识库并持久化（图类型同时清空命名图）
async fn delete_knowledge_base_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut guard = state.knowledge_bases.write().await;
    let removed = guard
        .iter()
        .find(|b| b.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        .cloned();
    let before = guard.len();
    guard.retain(|b| b.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    if guard.len() == before {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "knowledge base not found", "id": id })),
        );
    }
    let _ = save_knowledge_bases(&guard);
    // 图类型：清空命名图三元组
    if let Some(b) = removed {
        if let Some(g) = b.get("graph").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let clear = format!("DELETE WHERE {{ GRAPH <{g}> {{ ?s ?p ?o . }} }}");
            if let Err(e) = state.kg_store.update(&clear) {
                tracing::warn!(graph = %g, "KB graph clear skipped: {}", e);
            }
            let _ = state.kg_store.flush();
        }
    }
    (StatusCode::OK, Json(json!({ "status": "deleted", "id": id })))
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
        EventType::TaskCompleted => (
            "completion",
            json!({
                "status": "success",
                "summary": event.payload
            }),
        ),
        EventType::TaskFailed => (
            "completion",
            json!({
                "status": "failed",
                "summary": event.payload
            }),
        ),
        _ => return None,
    };

    Some(
        Event::default()
            .event(event_name)
            .data(data.to_string())
    )
}

// ─── G5：知识库可视化绑定 ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BindGraphRequest {
    /// KG 命名图名称，如 "aps/benches"，后端自动加前缀 tenant_graph(tenant_id, graph)。
    pub graph: String,
    pub description: Option<String>,
}

/// POST /api/v1/agents/:id/graph — 绑定智能体与知识图谱命名图
async fn bind_agent_graph_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<BindGraphRequest>,
) -> impl IntoResponse {
    if req.graph.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "graph 不能为空"
        }))).into_response();
    }
    // 租户隔离前缀
    let full_graph = format!("tenant:{}/{}", identity.tenant_id, req.graph);
    let patch = json!({
        "knowledge_graph": full_graph,
        "knowledge_graph_description": req.description.unwrap_or_default(),
    });
    let mut guard = state.user_agents.write().await;
    let found = guard.iter_mut()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
    match found {
        Some(agent) => {
            if let (Some(obj), Some(patch_obj)) = (agent.as_object_mut(), patch.as_object()) {
                for (k, v) in patch_obj {
                    obj.insert(k.clone(), v.clone());
                }
                obj.insert("updated_at".into(), json!(chrono::Utc::now().to_rfc3339()));
                obj.insert("graph_bound_by".into(), json!(identity.user_id.clone()));
            }
            let updated = agent.clone();
            let _ = save_user_agents(&guard);
            (StatusCode::OK, Json(json!({
                "status": "bound",
                "agent_id": id,
                "graph": full_graph,
                "agent": updated,
                "bound_by": identity.user_id,
            }))).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "agent not found", "id": id }))).into_response(),
    }
}

// ─── G6'：Prompt/模型灰度版本管理 ────────────────────────────────────────────

/// GET /api/v1/prompts — 列举所有 Prompt 版本
async fn list_prompts_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let versions = state.prompts.list_versions();
    let active_id = state.prompts.active_id();
    Json(json!({
        "count": versions.len(),
        "active_id": active_id,
        "versions": versions,
    }))
}

/// POST /api/v1/prompts — 创建新版本（G7：仅 DA 角色）
async fn create_prompt_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    Json(body): Json<PromptVersion>,
) -> impl IntoResponse {
    if let Err(e) = identity.require_role("DA") {
        return e.into_response();
    }
    let id = state.prompts.add_version(body);
    (StatusCode::CREATED, Json(json!({ "status": "created", "id": id }))).into_response()
}

/// POST /api/v1/prompts/:id/activate — 激活指定版本
async fn activate_prompt_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = identity.require_role("DA") {
        return e.into_response();
    }
    if state.prompts.activate(&id) {
        Json(json!({ "status": "activated", "id": id })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "版本不存在", "id": id }))).into_response()
    }
}

#[derive(Deserialize)]
pub struct CanaryRequest {
    pub percent: u8,
    #[serde(default)]
    pub tenant_ids: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// PUT /api/v1/prompts/:id/canary — 设置灰度规则
async fn canary_prompt_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<CanaryRequest>,
) -> impl IntoResponse {
    if let Err(e) = identity.require_role("DA") {
        return e.into_response();
    }
    if state.prompts.set_canary(&id, req.percent, req.tenant_ids, req.roles) {
        Json(json!({ "status": "ok", "id": id, "percent": req.percent })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "版本不存在", "id": id }))).into_response()
    }
}

/// DELETE /api/v1/prompts/:id — 删除版本
async fn delete_prompt_handler(
    State(state): State<Arc<AppState>>,
    identity: UserIdentity,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = identity.require_role("DA") {
        return e.into_response();
    }
    if state.prompts.delete_version(&id) {
        Json(json!({ "status": "deleted", "id": id })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "版本不存在", "id": id }))).into_response()
    }
}

/// GET /api/v1/prompts/resolve?tenant_id=&user_id=&role= — 灰度路由决策
async fn resolve_prompt_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let tenant_id = params.get("tenant_id").map(|s| s.as_str()).unwrap_or("default");
    let user_id   = params.get("user_id").map(|s| s.as_str()).unwrap_or("anonymous");
    let role      = params.get("role").map(|s| s.as_str()).unwrap_or("");
    match state.prompts.resolve(tenant_id, user_id, role) {
        Some(resolved) => Json(json!({ "status": "ok", "resolved": resolved })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "无可用 Prompt 版本（请先激活一个版本）" }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt; // oneshot

    /// 构造一个最小可用的 UnifiedGateway（不触网，仅满足 AppState 依赖）。
    fn test_gateway() -> UnifiedGateway {
        UnifiedGateway::new(&crate::config::GatewaySettings {
            base_url: "http://localhost".into(),
            api_key: String::new(),
            default_model: "test-model".into(),
            timeout_seconds: 30,
            max_retries: 1,
            model_mapping: std::collections::HashMap::new(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn test_config_handler_returns_sanitized_config() {
        // 构造一个包含 api_key 的测试配置
        let test_config = json!({
            "version": "0.1.0-test",
            "gateway": {
                "base_url": "https://api.example.com",
                "default_model": "test-model",
                "max_retries": 5,
                "timeout_seconds": 120,
                "model_mapping": {"default": "test-model"},
                "api_key_configured": true
            },
            "api": {
                "http_addr": "0.0.0.0:8080",
                "grpc_addr": "0.0.0.0:50051",
                "metrics_port": 9090
            },
            "memory": {"l1_max_messages": 50, "l2_max_node_size": 1024},
            "agents": {"max_iterations": 20, "max_parallel_agents": 5}
        });

        // 构造测试用 AppState (最小化依赖)
        use crate::core::core_types::{SemanticCore, CoreConfig};

        let tmp = std::env::temp_dir().join(format!("agentos_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let test_core_config = CoreConfig {
            max_node_size: 1024,
            max_projection_size: 2048,
            l0_storage_path: tmp.to_str().unwrap().to_string(),
            event_buffer_size: 10,
            enable_metrics: false,
            eviction_config: None,
        };
        let core = Arc::new(SemanticCore::new(test_core_config).unwrap());
        let kg_store = Arc::new(oxigraph::store::Store::new().unwrap());
        let gateway = Arc::new(test_gateway());

        let state = Arc::new(AppState {
            core,
            gateway,
            kg_store,
            config_info: Arc::new(tokio::sync::RwLock::new(test_config.clone())),
            agents_info: serde_json::json!({ "count": 0, "agents": [] }),
            mcp_servers: Arc::new(tokio::sync::RwLock::new(vec![])),
            user_agents: Arc::new(tokio::sync::RwLock::new(vec![])),
            prompts: Arc::new(PromptRegistry::new()),
            kb_categories: Arc::new(tokio::sync::RwLock::new(vec![])),
            knowledge_bases: Arc::new(tokio::sync::RwLock::new(vec![])),
        });

        // 构造 Router 并发起 GET /api/v1/config 请求
        let router = Router::new()
            .route("/api/v1/config", get(config_handler))
            .with_state(state);

        let req = axum::http::Request::builder()
            .uri("/api/v1/config")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // 读取 body 并解析 JSON
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let config_res: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // 验证关键字段存在（且无明文 api_key）
        assert_eq!(config_res["version"], "0.1.0-test");
        assert_eq!(config_res["gateway"]["base_url"], "https://api.example.com");
        assert_eq!(config_res["gateway"]["default_model"], "test-model");
        assert_eq!(config_res["gateway"]["api_key_configured"], true);
        assert!(config_res["gateway"]["api_key"].is_null() || !config_res["gateway"].as_object().unwrap().contains_key("api_key"));

        // 清理
        let _ = std::fs::remove_dir_all(tmp);
    }

    /// 端到端集成测试：电池维修助手 —— 导入(按租户隔离命名图) → 建体 → 多 Skill 注册(DA/匿名 403)
    /// → 绑定专用知识库 → 跨租户会话隔离查询 → 持久化落盘断言。
    #[tokio::test]
    async fn test_battery_assistant_e2e_tenant_isolation() {
        use crate::core::core_types::{CoreConfig, SemanticCore};
        use base64::{engine::general_purpose::STANDARD, Engine};

        // 隔离持久化目录 + 启用严格鉴权（验证匿名 403）
        let tmp = std::env::temp_dir().join(format!("agentos_e2e_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("AGENTOS_DATA_DIR", &tmp);
        std::env::set_var("AGENTOS_AUTH_STRICT", "true");

        let l0 = tmp.join("l0");
        std::fs::create_dir_all(&l0).unwrap();
        let core = Arc::new(
            SemanticCore::new(CoreConfig {
                max_node_size: 1024,
                max_projection_size: 2048,
                l0_storage_path: l0.to_str().unwrap().to_string(),
                event_buffer_size: 10,
                enable_metrics: false,
                eviction_config: None,
            })
            .unwrap(),
        );
        let kg_store = Arc::new(oxigraph::store::Store::new().unwrap());
        let gateway = Arc::new(test_gateway());
        let state = Arc::new(AppState {
            core,
            gateway,
            kg_store,
            config_info: Arc::new(tokio::sync::RwLock::new(json!({}))),
            agents_info: json!({ "count": 0, "agents": [] }),
            mcp_servers: Arc::new(tokio::sync::RwLock::new(vec![])),
            user_agents: Arc::new(tokio::sync::RwLock::new(vec![])),
            prompts: Arc::new(PromptRegistry::new()),
            kb_categories: Arc::new(tokio::sync::RwLock::new(vec![])),
            knowledge_bases: Arc::new(tokio::sync::RwLock::new(vec![])),
        });

        let router = Router::new()
            .route("/api/v1/kg/import", post(kg_import_handler))
            .route("/api/v1/kg/query", post(kg_query_handler))
            .route("/api/v1/agents", post(create_agent_handler))
            .route("/api/v1/skills", post(register_skill_handler))
            .route("/api/v1/agents/:id/graph", post(bind_agent_graph_handler))
            .with_state(state);

        async fn post_json(
            router: &Router,
            uri: &str,
            body: Value,
            ident: Option<&str>,
        ) -> (StatusCode, Value) {
            let mut b = axum::http::Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json");
            if let Some(id) = ident {
                b = b.header("x-identity", id);
            }
            let req = b.body(axum::body::Body::from(body.to_string())).unwrap();
            let resp = router.clone().oneshot(req).await.unwrap();
            let status = resp.status();
            let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            (status, v)
        }

        let id_of = |user: &str, tenant: &str| -> String {
            STANDARD.encode(
                json!({"user_id": user, "tenant_id": tenant, "roles": ["DA"]}).to_string(),
            )
        };
        let id_a = id_of("svc-tesla", "t-tesla");
        let id_b = id_of("svc-byd", "t-byd");

        let fault_node = |tenant: &str, code: &str, label: &str| -> Value {
            json!({
                "id": format!("dtc:{}:{}", tenant, code),
                "node_type": "aps:FaultCode",
                "label": label,
                "properties": {"code": code}
            })
        };
        let g_tesla = "tenant:t-tesla/kb/fault-codes";
        let g_byd = "tenant:t-byd/kb/fault-codes";

        // [1] 按租户导入隔离命名图
        let (st, _) = post_json(
            &router,
            "/api/v1/kg/import",
            json!({
                "graph": g_tesla, "clear_before": true,
                "nodes": [fault_node("t-tesla", "BMS_a067", "BMS_a067 — 高压电池需要维修"),
                          fault_node("t-tesla", "BMS_a068", "BMS_a068 — 电池需要维修")],
                "edges": []
            }),
            Some(&id_a),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = post_json(
            &router,
            "/api/v1/kg/import",
            json!({
                "graph": g_byd, "clear_before": true,
                "nodes": [fault_node("t-byd", "P0A80", "P0A80 — 动力电池热管理系统故障"),
                          fault_node("t-byd", "P0A1F", "P0A1F — 电池包电压异常偏高")],
                "edges": []
            }),
            Some(&id_b),
        )
        .await;
        assert_eq!(st, StatusCode::OK);

        // [2] 创建智能体
        let (st, agent) = post_json(
            &router,
            "/api/v1/agents",
            json!({
                "name": "新能源汽车电池维修助手",
                "description": "聚合多技能、绑定专用故障码知识库的工业级维修助手",
                "business_domain": "新能源汽车维修",
                "enabled": true
            }),
            Some(&id_a),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let agent_id = agent["id"].as_str().unwrap().to_string();
        assert!(!agent_id.is_empty());

        // [3] 注册多个 Skill（DA 角色）
        let skill = |iri: &str, name: &str| -> Value {
            json!({
                "skill_iri": iri, "name": name, "description": name,
                "version": "1.0.0", "category": "diagnostics", "security_level": "standard",
                "allowed_roles": ["DA"], "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}, "compiled_template": "{{x}}"
            })
        };
        for (iri, name) in [
            ("skill://battery/fault-code-lookup", "故障码检索"),
            ("skill://battery/repair-order-gen", "维修工单生成"),
            ("skill://battery/severity-triage", "故障严重度分级"),
        ] {
            let (st, _) = post_json(&router, "/api/v1/skills", skill(iri, name), Some(&id_a)).await;
            assert_eq!(st, StatusCode::CREATED, "skill {} should register", iri);
        }
        // 负向：严格模式下匿名注册应 403
        let (st, _) =
            post_json(&router, "/api/v1/skills", skill("skill://battery/anon", "匿名技能"), None)
                .await;
        assert_eq!(st, StatusCode::FORBIDDEN);

        // [4] 绑定专用知识库（自动注入租户前缀）
        let (st, bound) = post_json(
            &router,
            &format!("/api/v1/agents/{}/graph", agent_id),
            json!({"graph": "kb/fault-codes", "description": "故障码知识库"}),
            Some(&id_a),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(bound["graph"], g_tesla);
        assert_eq!(bound["bound_by"], "svc-tesla");

        // [5] 跨租户会话隔离
        let list = |g: &str| {
            json!({"sparql": format!(
                "SELECT ?label WHERE {{ GRAPH <{}> {{ ?s a <http://aps.local/ontology/FaultCode> ; <http://www.w3.org/2000/01/rdf-schema#label> ?label }} }}", g)})
        };
        let find = |g: &str, code: &str| {
            json!({"sparql": format!(
                "SELECT ?label WHERE {{ GRAPH <{}> {{ ?s a <http://aps.local/ontology/FaultCode> ; <https://agentos.ontology/meta/code> \"{}\" }} }}", g, code)})
        };

        let (st, a) = post_json(&router, "/api/v1/kg/query", list(g_tesla), Some(&id_a)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(a["count"], 2);
        let (_, b) = post_json(&router, "/api/v1/kg/query", list(g_byd), Some(&id_b)).await;
        assert_eq!(b["count"], 2);

        // 隔离：租户A 图中查 BYD 专有码 → 0
        let (_, x) = post_json(&router, "/api/v1/kg/query", find(g_tesla, "P0A80"), Some(&id_a)).await;
        assert_eq!(x["count"], 0, "cross-tenant leak: P0A80 must not appear in tesla graph");
        // 对照：租户B 图中查同码 → 1
        let (_, y) = post_json(&router, "/api/v1/kg/query", find(g_byd, "P0A80"), Some(&id_b)).await;
        assert_eq!(y["count"], 1);
        // 对照：租户A 图中查 Tesla 码 → 1
        let (_, z) = post_json(&router, "/api/v1/kg/query", find(g_tesla, "BMS_a067"), Some(&id_a)).await;
        assert_eq!(z["count"], 1);

        // [6] 持久化落盘断言
        let agents_disk = std::fs::read_to_string(tmp.join("agents.json")).unwrap();
        assert!(agents_disk.contains("新能源汽车电池维修助手"));
        let skills_disk = std::fs::read_to_string(tmp.join("skills.json")).unwrap();
        assert!(skills_disk.contains("skill://battery/fault-code-lookup"));

        // 清理
        std::env::remove_var("AGENTOS_DATA_DIR");
        std::env::remove_var("AGENTOS_AUTH_STRICT");
        let _ = std::fs::remove_dir_all(tmp);
    }
}

/// 动力层执行器（ActionType invoke）单测：参数/前置条件校验 + SPARQL 组装。
#[cfg(test)]
mod ontology_action_tests {
    use super::*;
    use oxigraph::store::Store;

    /// 预置车辆/故障码/电池/车型实例于 graph:pack/ev-repair。
    fn seeded_kg() -> KnowledgeGraphStore {
        let store = Arc::new(Store::new().unwrap());
        let seed = format!(
            "INSERT DATA {{ GRAPH <{g}> {{ \
             <{veh}> a <{vehc}> . \
             <{fault}> a <{faultc}> . \
             <{bat}> a <{batc}> . \
             <{model}> a <{modelc}> . \
             }} }}",
            g = EV_PACK_GRAPH,
            veh = ev_instance_iri("Vehicle", "LVIN123"),
            vehc = ev_term_iri("Vehicle"),
            fault = ev_instance_iri("FaultCode", "P0A80"),
            faultc = ev_term_iri("FaultCode"),
            bat = ev_instance_iri("Battery", "BAT-001"),
            batc = ev_term_iri("Battery"),
            model = ev_instance_iri("VehicleModel", "M-001"),
            modelc = ev_term_iri("VehicleModel"),
        );
        store.update(&seed).unwrap();
        KnowledgeGraphStore::with_shared_store(store).unwrap()
    }

    fn mk_req(target: Option<&str>, params: Value, dry_run: bool) -> ActionInvokeRequest {
        ActionInvokeRequest {
            target: target.map(|s| s.to_string()),
            params: params.as_object().cloned().unwrap_or_default(),
            dry_run,
        }
    }

    #[test]
    fn test_iri_safe_and_instance_iri() {
        assert_eq!(iri_safe("P0A80"), "P0A80");
        assert_eq!(iri_safe("a b"), "a_b");
        assert_eq!(ev_instance_iri("Vehicle", "X 1"), "https://agentos.ontology/ev/Vehicle/X_1");
        assert_eq!(ev_prop_iri("soh"), "https://agentos.ontology/ev/prop/soh");
    }

    #[test]
    fn test_generate_repair_order_ok() {
        let kg = seeded_kg();
        let r = mk_req(Some("P0A80"), json!({"vehicle_vin": "LVIN123", "assigned_to": "张工", "estimated_cost": 1200}), false);
        let (stmts, meta) = build_action_effects("GenerateRepairOrder", &r, &kg, "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(stmts.len(), 1);
        let s = &stmts[0];
        assert!(s.contains("RepairOrder"));
        assert!(s.contains("forVehicle"));
        assert!(s.contains("diagnoses"));
        assert!(s.contains("张工"));
        assert!(s.contains("1200"));
        assert!(meta["order_id"].as_str().unwrap().starts_with("RO-"));
    }

    #[test]
    fn test_generate_repair_order_missing_vehicle_precondition() {
        let kg = seeded_kg();
        let r = mk_req(Some("P0A80"), json!({"vehicle_vin": "UNKNOWN"}), false);
        let err = build_action_effects("GenerateRepairOrder", &r, &kg, "t").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("车辆VIN不存在"));
    }

    #[test]
    fn test_generate_repair_order_missing_target() {
        let kg = seeded_kg();
        let r = mk_req(None, json!({"vehicle_vin": "LVIN123"}), false);
        let err = build_action_effects("GenerateRepairOrder", &r, &kg, "t").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_update_battery_soh_ok_and_range() {
        let kg = seeded_kg();
        let ok = mk_req(None, json!({"battery_id": "BAT-001", "soh": 87.5}), false);
        let (stmts, meta) = build_action_effects("UpdateBatterySoh", &ok, &kg, "t").unwrap();
        assert_eq!(stmts.len(), 4); // soh upsert(2) + soh_updated_at upsert(2)
        assert!(stmts.iter().any(|s| s.contains("DELETE WHERE")));
        assert!(stmts.iter().any(|s| s.contains("87.5")));
        assert_eq!(meta["soh"], 87.5);

        let bad = mk_req(None, json!({"battery_id": "BAT-001", "soh": 150}), false);
        let err = build_action_effects("UpdateBatterySoh", &bad, &kg, "t").unwrap_err();
        assert!(err.1.contains("0-100"));
    }

    #[test]
    fn test_update_battery_soh_missing_battery() {
        let kg = seeded_kg();
        let r = mk_req(None, json!({"battery_id": "NOPE", "soh": 50}), false);
        let err = build_action_effects("UpdateBatterySoh", &r, &kg, "t").unwrap_err();
        assert!(err.1.contains("电池对象不存在"));
    }

    #[test]
    fn test_mark_recall_ok() {
        let kg = seeded_kg();
        let r = mk_req(None, json!({"model_id": "M-001", "recall_reason": "电池批次缺陷"}), false);
        let (stmts, meta) = build_action_effects("MarkRecall", &r, &kg, "t").unwrap();
        assert_eq!(stmts.len(), 6); // 三个属性各 upsert(2)
        assert!(stmts.iter().any(|s| s.contains("recalled")));
        assert!(stmts.iter().any(|s| s.contains("电池批次缺陷")));
        assert_eq!(meta["recalled"], true);
    }

    #[test]
    fn test_append_faq_ok_and_links_fault() {
        let kg = seeded_kg();
        let r = mk_req(Some("P0A80"), json!({"question": "报警怎么办？", "answer": "请尽快检修"}), false);
        let (stmts, meta) = build_action_effects("AppendFaq", &r, &kg, "t").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("relatedFaq"));
        assert!(stmts[0].contains("报警怎么办"));
        assert!(meta["faq_id"].as_str().unwrap().starts_with("FAQ-"));
    }

    #[test]
    fn test_append_faq_missing_fault_precondition() {
        let kg = seeded_kg();
        let r = mk_req(Some("NON_EXIST"), json!({"question": "q", "answer": "a"}), false);
        let err = build_action_effects("AppendFaq", &r, &kg, "t").unwrap_err();
        assert!(err.1.contains("故障码对象不存在"));
    }

    #[test]
    fn test_unknown_action() {
        let kg = seeded_kg();
        let r = mk_req(None, json!({}), false);
        let err = build_action_effects("NoSuchAction", &r, &kg, "t").unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }
}
