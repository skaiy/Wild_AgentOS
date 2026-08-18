use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimResponse {
    pub task_id: String,
    pub stage_id: String,
    pub task_def: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableTask {
    pub task_id: String,
    pub project_id: String,
    pub stage_id: String,
    pub stage_type: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct CallbackPayload {
    stage_id: String,
    status: String,
    output: serde_json::Value,
    summary: String,
}

pub struct CenterClient {
    center_url: String,
    auth_token: String,
    client: reqwest::Client,
}

impl CenterClient {
    pub fn new(center_url: String, auth_token: String) -> Self {
        let client = reqwest::Client::new();
        Self {
            center_url,
            auth_token,
            client,
        }
    }

    pub async fn heartbeat(&self, agent_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/agents/heartbeat", self.center_url);
        let payload = serde_json::json!({ "agent_id": agent_id });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.auth_token)
            .json(&payload)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .context("failed to send heartbeat")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("heartbeat failed with status {status}: {body}");
        }

        Ok(())
    }

    pub async fn claim_task(
        &self,
        agent_id: &str,
        task_id: &str,
    ) -> anyhow::Result<ClaimResponse> {
        let url = format!("{}/api/v1/tasks/{task_id}/claim", self.center_url);
        let payload = serde_json::json!({ "agent_id": agent_id });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.auth_token)
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("failed to send claim request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("claim task failed with status {status}: {body}");
        }

        let claim_resp: ClaimResponse = resp
            .json()
            .await
            .context("failed to parse claim response")?;

        Ok(claim_resp)
    }

    pub async fn send_callback(
        &self,
        task_id: &str,
        stage_id: &str,
        status: &str,
        output: &serde_json::Value,
        summary: &str,
    ) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/tasks/{task_id}/callback", self.center_url);
        let payload = CallbackPayload {
            stage_id: stage_id.to_string(),
            status: status.to_string(),
            output: output.clone(),
            summary: summary.to_string(),
        };

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.auth_token)
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("failed to send callback")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("callback failed with status {status}: {body}");
        }

        Ok(())
    }

    pub async fn get_available_tasks(&self) -> anyhow::Result<Vec<AvailableTask>> {
        let url = format!("{}/api/v1/tasks/available", self.center_url);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.auth_token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("failed to fetch available tasks")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("get available tasks failed with status {status}: {body}");
        }

        let tasks: Vec<AvailableTask> = resp
            .json()
            .await
            .context("failed to parse available tasks")?;

        Ok(tasks)
    }
}