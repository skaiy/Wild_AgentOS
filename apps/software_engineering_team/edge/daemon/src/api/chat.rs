use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::runner::ChatMessage;
use crate::grpc::KernelClient;
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub session_id: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub session_id: String,
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    let api_key = req.api_key.unwrap_or_else(|| state.config.llm.api_key.clone());
    let base_url = req.base_url.unwrap_or_else(|| state.config.llm.base_url.clone());
    let model = req.model.unwrap_or_else(|| state.config.llm.model.clone());

    let build_context = || {
        let mut context = String::new();
        for msg in &req.messages {
            let role_label = match msg.role.as_str() {
                "assistant" => "助手",
                "system" => "系统",
                _ => "用户",
            };
            context.push_str(&format!("{}: {}\n", role_label, msg.content));
        }
        context
    };

    // Try kernel gRPC directly (SA pipeline)
    if state.config.kernel.is_enabled() {
        let context = build_context();
        if let Some(content) =
            try_kernel_chat(&state, &context, &session_id, &api_key, &base_url, &model).await
        {
            return Json(ChatResponse {
                content,
                session_id,
            });
        }
    }

    // Fallback: direct LLM call
    let runner = crate::agent::runner::AgentRunner::new(api_key, base_url, model);

    match runner.chat(req.messages).await {
        Ok(content) => Json(ChatResponse { content, session_id }),
        Err(e) => Json(ChatResponse {
            content: format!("error: {}", e),
            session_id,
        }),
    }
}

async fn try_kernel_chat(
    state: &Arc<AppState>,
    context: &str,
    session_id: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Option<String> {
    let mut kernel_lock = state.kernel.lock().await;

    if let Some(ref client) = *kernel_lock {
        match client
            .chat_stream(
                context.to_string(),
                format!("iri://chat/{}", session_id),
                api_key.to_string(),
                base_url.to_string(),
                model.to_string(),
            )
            .await
        {
            Ok(content) => {
                tracing::debug!("Kernel SA pipeline succeeded (cached connection)");
                return Some(content);
            }
            Err(e) => {
                tracing::warn!("Cached kernel connection failed: {}. Reconnecting...", e);
                *kernel_lock = None;
            }
        }
    }

    match KernelClient::connect(&state.config.kernel.target).await {
        Ok(new_client) => {
            let result = new_client
                .chat_stream(
                    context.to_string(),
                    format!("iri://chat/{}", session_id),
                    api_key.to_string(),
                    base_url.to_string(),
                    model.to_string(),
                )
                .await;

            match result {
                Ok(content) => {
                    tracing::info!(
                        "Kernel SA pipeline succeeded ({} chars via gRPC)",
                        content.len()
                    );
                    *kernel_lock = Some(new_client);
                    Some(content)
                }
                Err(e) => {
                    tracing::warn!("Kernel gRPC chat failed: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to kernel gRPC: {}", e);
            None
        }
    }
}