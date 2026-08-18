use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::sync::client::CenterClient;

pub struct HeartbeatRunner {
    client: Arc<CenterClient>,
    agent_id: String,
    interval: Duration,
    cancel_token: CancellationToken,
}

impl HeartbeatRunner {
    pub fn new(client: Arc<CenterClient>, agent_id: String, interval_secs: u64) -> Self {
        Self {
            client,
            agent_id,
            interval: Duration::from_secs(interval_secs),
            cancel_token: CancellationToken::new(),
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let client = Arc::clone(&self.client);
        let agent_id = self.agent_id.clone();
        let interval = self.interval;
        let cancel_token = self.cancel_token.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        tracing::info!("heartbeat stopped for agent {}", agent_id);
                        break;
                    }
                    _ = tokio::time::sleep(interval) => {
                        match client.heartbeat(&agent_id).await {
                            Ok(()) => {
                                tracing::debug!("heartbeat sent for agent {}", agent_id);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "heartbeat failed for agent {}: {:?}",
                                    agent_id,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.cancel_token.cancel();
    }
}