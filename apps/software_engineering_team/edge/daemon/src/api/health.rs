use axum::Json;
use serde_json::Value;

pub async fn health_check() -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0"
    }))
}