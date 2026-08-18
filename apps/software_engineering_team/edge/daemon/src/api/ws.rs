use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

pub async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    let welcome = serde_json::json!({
        "type": "connected",
        "message": "AgentOS Daemon connected"
    });

    if socket.send(Message::Text(welcome.to_string())).await.is_err() {
        return;
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            if val.get("type").and_then(|t| t.as_str()) == Some("pong") {
                                continue;
                            }
                        }

                        let echo = serde_json::json!({
                            "type": "echo",
                            "data": text
                        });

                        if socket.send(Message::Text(echo.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
            _ = interval.tick() => {
                let ping = serde_json::json!({
                    "type": "ping"
                });

                if socket.send(Message::Text(ping.to_string())).await.is_err() {
                    break;
                }
            }
        }
    }

    tracing::info!("websocket connection closed");
}