use anyhow::Context;
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub agent_id: String,
    pub token: String,
    pub token_expires_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegisterPayload {
    agent_id: String,
    capabilities: Vec<String>,
    user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    iat: usize,
}

pub struct AuthManager {
    center_url: String,
    secret: String,
}

impl AuthManager {
    pub fn new(center_url: String, secret: String) -> Self {
        Self { center_url, secret }
    }

    pub async fn register(
        &self,
        agent_id: &str,
        capabilities: Vec<String>,
        user_id: &str,
    ) -> anyhow::Result<RegisterResponse> {
        let url = format!("{}/api/v1/agents/register", self.center_url);
        let payload = RegisterPayload {
            agent_id: agent_id.to_string(),
            capabilities,
            user_id: user_id.to_string(),
        };

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("failed to send register request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("register failed with status {status}: {body}");
        }

        let register_resp: RegisterResponse = resp
            .json()
            .await
            .context("failed to parse register response")?;

        Ok(register_resp)
    }

    pub fn generate_jwt(&self, agent_id: &str) -> anyhow::Result<String> {
        let now = Utc::now();
        let exp = (now + chrono::Duration::hours(24)).timestamp() as usize;
        let iat = now.timestamp() as usize;

        let claims = Claims {
            sub: agent_id.to_string(),
            exp,
            iat,
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .context("failed to encode JWT")?;

        Ok(token)
    }

    pub fn validate_jwt(&self, token: &str) -> anyhow::Result<String> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .context("failed to validate JWT")?;

        Ok(token_data.claims.sub)
    }
}