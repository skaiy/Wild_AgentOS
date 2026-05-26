use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

use crate::core::core_types::SemanticCore;

/// Shared application state
pub struct AppState {
    pub core: Arc<SemanticCore>,
}

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Task request body
#[derive(Deserialize)]
pub struct TaskRequest {
    pub user_input: String,
}

/// Node write request
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

/// Build the HTTP router
pub fn build_router(core: Arc<SemanticCore>) -> Router {
    let state = Arc::new(AppState { core });

    Router::new()
        // Health
        .route("/health", get(health_handler))
        // Metrics
        .route("/metrics", get(metrics_handler))
        // Tasks
        .route("/api/v1/tasks", post(create_task_handler))
        .route("/api/v1/tasks/:task_iri", get(get_task_handler))
        // Nodes
        .route("/api/v1/nodes", post(write_node_handler))
        .route("/api/v1/nodes/:node_iri", get(read_node_handler))
        // Projections
        .route("/api/v1/projections", post(get_projection_handler))
        // Events
        .route("/api/v1/events", post(emit_event_handler))
        // Skills
        .route("/api/v1/skills", get(list_skills_handler))
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
    Json(json!({
        "count": skills.len(),
        "skills": skills,
    }))
}
