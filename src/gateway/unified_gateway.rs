use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::config::settings::GatewaySettings;
use crate::llm::stream_processor::MessageStream;
use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallPayload>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// 消息内容:纯文本或多部件(VL 多模态)。`#[serde(untagged)]` 先试 Text,
/// 使旧 JSON `"content":"..."` 仍反序列化为 Text;新 VL 数组反序列化为 Parts。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

impl Default for ChatContent {
    fn default() -> Self {
        ChatContent::Text(String::new())
    }
}

impl ChatContent {
    pub fn text<S: Into<String>>(s: S) -> Self {
        ChatContent::Text(s.into())
    }
    /// 取纯文本(Parts 时拼接所有 text 部件),供校验/回退/日志使用。
    pub fn as_text(&self) -> String {
        match self {
            ChatContent::Text(s) => s.clone(),
            ChatContent::Parts(ps) => ps
                .iter()
                .filter_map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
    /// 取可变文本引用;若当前为 Parts 则先折叠为等价纯文本,便于就地增删(如系统提示拼接)。
    pub fn as_text_mut(&mut self) -> &mut String {
        if let ChatContent::Parts(_) = self {
            let flattened = self.as_text();
            *self = ChatContent::Text(flattened);
        }
        match self {
            ChatContent::Text(s) => s,
            _ => unreachable!("just normalized to Text above"),
        }
    }
    pub fn image(url: impl Into<String>) -> ContentPart {
        ContentPart {
            kind: "image_url".into(),
            text: None,
            image_url: Some(ImageUrl { url: url.into() }),
        }
    }
    pub fn part_text(t: impl Into<String>) -> ContentPart {
        ContentPart {
            kind: "text".into(),
            text: Some(t.into()),
            image_url: None,
        }
    }
    /// 提取所有 image_url 部件的 URL(Text 变体返回空),供 VL 透传。
    pub fn image_urls(&self) -> Vec<String> {
        match self {
            ChatContent::Text(_) => Vec::new(),
            ChatContent::Parts(ps) => ps
                .iter()
                .filter_map(|p| p.image_url.as_ref().map(|u| u.url.clone()))
                .collect(),
        }
    }
}

impl From<String> for ChatContent {
    fn from(s: String) -> Self {
        ChatContent::Text(s)
    }
}

impl From<&str> for ChatContent {
    fn from(s: &str) -> Self {
        ChatContent::Text(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallPayload {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: Option<String>,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ResponseToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 单个 provider 的运行时端点信息(由 models 注册表灌入,支持热更新)。
#[derive(Clone, Default)]
pub struct ProviderRuntime {
    pub base_url: String,
    pub api_key: String,
    pub timeout_seconds: u64,
}

pub struct UnifiedGateway {
    base_url: RwLock<String>,
    api_key: RwLock<String>,
    client: Client,
    model_mapping: RwLock<HashMap<String, String>>,
    default_model: RwLock<String>,
    timeout_seconds: u64,
    max_retries: u32,
    retry_base_ms: u64,
    /// provider_id → 运行时端点(base_url/api_key/timeout)。
    providers: RwLock<HashMap<String, ProviderRuntime>>,
    /// resource.model → provider_id;命中则按该 provider 解析端点,否则回退单网关。
    model_provider: RwLock<HashMap<String, String>>,
}

impl UnifiedGateway {
    pub fn new(settings: &GatewaySettings) -> Result<Self, CoreError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(settings.timeout_seconds))
            // 显式 User-Agent：部分网关前置 WAF 会拦截空 UA 的请求（返回 403）。
            .user_agent("curl/8.1.2")
            .build()
            .map_err(|e| CoreError::Internal {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self {
            base_url: RwLock::new(settings.base_url.trim_end_matches('/').to_string()),
            api_key: RwLock::new(settings.api_key.clone()),
            client,
            model_mapping: RwLock::new(settings.model_mapping.clone()),
            default_model: RwLock::new(settings.default_model.clone()),
            timeout_seconds: settings.timeout_seconds,
            max_retries: settings.max_retries,
            retry_base_ms: 500,
            providers: RwLock::new(HashMap::new()),
            model_provider: RwLock::new(HashMap::new()),
        })
    }

    pub fn default_model(&self) -> String {
        self.default_model.read().unwrap().clone()
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<ChatCompletionResponse, CoreError> {
        let model = self.get_model("default");
        let sanitized = Self::sanitize_tool_messages(messages);
        self.chat_with_model(&model, sanitized).await
    }

    pub async fn chat_with_model(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<ChatCompletionResponse, CoreError> {
        let sanitized = Self::sanitize_tool_messages(messages);
        let (base, key) = self.resolve_endpoint(model);
        let url = format!("{}/v1/chat/completions", base);
        let body = serde_json::json!({
            "model": model,
            "messages": sanitized,
        });
        self.send_request(&url, &key, body).await
    }

    pub async fn chat_with_params(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<&str>,
    ) -> Result<ChatCompletionResponse, CoreError> {
        let messages = Self::sanitize_tool_messages(messages);
        // Pre-validate messages: check for empty content that might cause 400 errors
        for (i, msg) in messages.iter().enumerate() {
            if msg.content.as_text().trim().is_empty() && msg.role != "assistant" {
                warn!(
                    msg_idx = i, role = %msg.role,
                    "Message has empty content — this may cause 400 errors from the LLM API"
                );
            }
        }

        let (base, key) = self.resolve_endpoint(model);
        let url = format!("{}/v1/chat/completions", base);
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
        });
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(tokens) = max_tokens {
            body["max_tokens"] = serde_json::json!(tokens);
        }
        if let Some(t) = tools {
            body["tools"] = serde_json::json!(t);
            body["tool_choice"] = serde_json::json!(tool_choice.unwrap_or("auto"));
        }
        self.send_request(&url, &key, body).await
    }

    async fn send_request(
        &self,
        url: &str,
        api_key: &str,
        body: Value,
    ) -> Result<ChatCompletionResponse, CoreError> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let backoff = Duration::from_millis(self.retry_base_ms * u64::pow(2, attempt - 1));
                tokio::time::sleep(backoff).await;
                debug!(attempt, "Retrying LLM API call");
            }

            let req_body = body.clone();
            let req = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&req_body);

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let response_text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                warn!(error = %e, "Failed to read LLM response body");
                                last_error = Some(CoreError::Internal {
                                    message: format!("Failed to read response body: {}", e),
                                });
                                continue;
                            }
                        };
                        match serde_json::from_str::<ChatCompletionResponse>(&response_text) {
                            Ok(result) => {
                                info!(
                                    model = %body["model"],
                                    usage = ?result.usage.as_ref().map(|u| u.total_tokens),
                                    "LLM API call successful"
                                );
                                return Ok(result);
                            }
                            Err(e) => {
                                warn!(error = %e, response_len = response_text.len(), "Failed to parse LLM response");
                                last_error = Some(CoreError::Internal {
                                    message: format!("Failed to parse LLM response: {} (response length: {})", e, response_text.len()),
                                });
                            }
                        }
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        // Embed a preview of the request body into the error message
                        // for debugging 4xx errors directly from the TUI / result display.
                        let req_body_str = serde_json::to_string_pretty(&req_body)
                            .unwrap_or_default();
                        let req_preview: String = req_body_str
                            .chars()
                            .take(8000)
                            .collect();
                        warn!(status = %status, body = %text, req_preview = %req_preview, "LLM API error");
                        last_error = Some(CoreError::Internal {
                            message: format!("LLM API error ({}): {}\nrequest_preview(8k)={}", status, text, req_preview),
                        });
                        if status.is_client_error() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "LLM API request failed");
                    last_error = Some(CoreError::Internal {
                        message: format!("LLM API request failed: {}", e),
                    });
                }
            }
        }

        Err(last_error.unwrap_or_else(|| CoreError::Internal {
            message: "LLM API call failed after all retries".to_string(),
        }))
    }

    pub fn set_base_url(&self, url: String) {
        *self.base_url.write().unwrap() = crate::config::settings::normalize_api_base(&url);
    }

    pub fn set_api_key(&self, key: String) {
        *self.api_key.write().unwrap() = key;
    }

    pub fn set_default_model(&self, model: String) {
        *self.default_model.write().unwrap() = model.clone();
        self.model_mapping.write().unwrap().insert("default".to_string(), model);
    }

    pub fn set_model_mapping(&self, task_type: String, model: String) {
        self.model_mapping.write().unwrap().insert(task_type, model);
    }

    /// 灌入 provider 运行时注册表(整体替换),支持 models 段热更新。
    pub fn set_provider_registry(&self, provs: HashMap<String, ProviderRuntime>) {
        *self.providers.write().unwrap() = provs;
    }

    /// 灌入 model→provider 映射(整体替换),支持 models 段热更新。
    pub fn set_model_provider_mapping(&self, map: HashMap<String, String>) {
        *self.model_provider.write().unwrap() = map;
    }

    /// 按 model 解析目标端点:命中 model→provider 且其 base_url 非空则用该 provider 的
    /// base_url/api_key;否则回退单网关(向后兼容,未配置 models 或未命中时行为不变)。
    fn resolve_endpoint(&self, model: &str) -> (String, String) {
        if let Some(pid) = self.model_provider.read().unwrap().get(model).cloned() {
            if let Some(p) = self.providers.read().unwrap().get(&pid) {
                if !p.base_url.is_empty() {
                    return (crate::config::settings::normalize_api_base(&p.base_url), p.api_key.clone());
                }
            }
        }
        (
            self.base_url.read().unwrap().clone(),
            self.api_key.read().unwrap().clone(),
        )
    }

    pub fn get_model(&self, task_type: &str) -> String {
        let mapping = self.model_mapping.read().unwrap();
        mapping
            .get(task_type)
            .or_else(|| mapping.get("default"))
            .cloned()
            .unwrap_or_else(|| self.default_model.read().unwrap().clone())
    }

    pub async fn health_check(&self) -> Result<bool, CoreError> {
        let url = format!("{}/v1/models", self.base_url.read().unwrap());
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key.read().unwrap()))
            .send()
            .await
        {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Sanitize messages to avoid OpenAI/DeepSeek API error:
    /// "Messages with role 'tool' must be a response to a preceding message with 'tool_calls'"
    fn sanitize_tool_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        crate::core::context_compressor::ContextWindowManager::remove_orphaned_tool_messages(messages)
    }

    pub fn supports_native_reasoning(&self, model: &str) -> bool {
        let model_lower = model.to_lowercase();
        
        if model_lower.contains("deepseek-r1")
            || model_lower.contains("deepseek-reasoning") {
            return true;
        }
        
        if model_lower.starts_with("o1-") 
            || model_lower.starts_with("o3-")
            || model_lower.starts_with("o1")
            || model_lower.starts_with("o3") {
            return true;
        }
        
        if model_lower.contains("claude") && model_lower.contains("extended-thinking") {
            return true;
        }
        
        if model_lower.contains("gemini") && model_lower.contains("thinking") {
            return true;
        }
        
        false
    }

    pub async fn stream_chat_with_params(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<&str>,
    ) -> Result<MessageStream, CoreError> {
        let (base, key) = self.resolve_endpoint(model);
        let url = format!("{}/v1/chat/completions", base);
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(tokens) = max_tokens {
            body["max_tokens"] = serde_json::json!(tokens);
        }
        if let Some(t) = tools {
            body["tools"] = serde_json::json!(t);
            body["tool_choice"] = serde_json::json!(tool_choice.unwrap_or("auto"));
        }

        self.send_stream_request(&url, &key, body).await
    }

    async fn send_stream_request(
        &self,
        url: &str,
        api_key: &str,
        body: Value,
    ) -> Result<MessageStream, CoreError> {
        let req = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body);

        let response = req.send().await.map_err(|e| CoreError::Internal {
            message: format!("Stream request failed: {}", e),
        })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(CoreError::Internal {
                message: format!("Stream API error ({}): {}", status, text),
            });
        }

        info!(model = %body["model"], "Stream request started");
        Ok(MessageStream::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        let settings = GatewaySettings {
            base_url: "http://localhost:3000".to_string(),
            api_key: "sk-test".to_string(),
            default_model: "deepseek-v4-flash".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            model_mapping: HashMap::from([
                ("planning".to_string(), "deepseek-v4-pro".to_string()),
                ("default".to_string(), "deepseek-v4-flash".to_string()),
            ]),
        };

        let gateway = UnifiedGateway::new(&settings).unwrap();
        assert_eq!(gateway.get_model("planning"), "deepseek-v4-pro");
        assert_eq!(gateway.get_model("unknown"), "deepseek-v4-flash");
    }

    #[test]
    fn test_runtime_updates() {
        let settings = GatewaySettings {
            base_url: "http://localhost:3000".to_string(),
            api_key: "sk-test".to_string(),
            default_model: "deepseek-v4-flash".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            model_mapping: HashMap::from([("default".to_string(), "deepseek-v4-flash".to_string())]),
        };

        let gateway = UnifiedGateway::new(&settings).unwrap();

        // test updating model at runtime
        gateway.set_default_model("deepseek-v4-pro".to_string());
        assert_eq!(gateway.get_model("default"), "deepseek-v4-pro");

        // test updating API key at runtime
        gateway.set_api_key("sk-new-key".to_string());
        assert_eq!(*gateway.api_key.read().unwrap(), "sk-new-key");

        // test updating base URL at runtime
        gateway.set_base_url("https://api.new-endpoint.com".to_string());
        assert_eq!(*gateway.base_url.read().unwrap(), "https://api.new-endpoint.com");
    }

    fn test_gateway() -> UnifiedGateway {
        let settings = GatewaySettings {
            base_url: "http://fallback:3000".to_string(),
            api_key: "sk-fallback".to_string(),
            default_model: "deepseek-v4-flash".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            model_mapping: HashMap::from([("default".to_string(), "deepseek-v4-flash".to_string())]),
        };
        UnifiedGateway::new(&settings).unwrap()
    }

    #[test]
    fn test_chat_content_untagged_backcompat() {
        // 旧 JSON:content 为字符串 → Text
        let m: ChatMessage =
            serde_json::from_str(r#"{"role":"user","content":"hi"}"#).unwrap();
        assert!(matches!(m.content, ChatContent::Text(ref s) if s == "hi"));

        // 新 VL JSON:content 为数组 → Parts
        let vl = r#"{"role":"user","content":[{"type":"text","text":"看图"},{"type":"image_url","image_url":{"url":"http://x/1.png"}}]}"#;
        let m2: ChatMessage = serde_json::from_str(vl).unwrap();
        match &m2.content {
            ChatContent::Parts(ps) => {
                assert_eq!(ps.len(), 2);
                assert_eq!(ps[0].kind, "text");
                assert_eq!(ps[1].kind, "image_url");
                assert_eq!(ps[1].image_url.as_ref().unwrap().url, "http://x/1.png");
            }
            _ => panic!("expected Parts"),
        }
    }

    #[test]
    fn test_chat_content_as_text_and_serialize() {
        let parts = ChatContent::Parts(vec![
            ChatContent::part_text("第一段"),
            ChatContent::image("http://x/1.png"),
            ChatContent::part_text("第二段"),
        ]);
        assert_eq!(parts.as_text(), "第一段\n第二段");
        // image_urls 仅提取 image_url 部件;Text 变体返回空。
        assert_eq!(parts.image_urls(), vec!["http://x/1.png".to_string()]);
        assert!(ChatContent::text("hi").image_urls().is_empty());

        // Text 序列化为字符串(untagged),保持旧线格式。
        let txt = ChatContent::text("hi");
        assert_eq!(serde_json::to_value(&txt).unwrap(), serde_json::json!("hi"));
    }

    #[test]
    fn test_resolve_endpoint_hit_and_fallback() {
        let gw = test_gateway();
        // 未配置 models → 回退单网关
        let (b, k) = gw.resolve_endpoint("deepseek-v4-flash");
        assert_eq!(b, "http://fallback:3000");
        assert_eq!(k, "sk-fallback");

        // 配置 provider + 映射 → 命中
        gw.set_provider_registry(HashMap::from([(
            "prov-vl".to_string(),
            ProviderRuntime {
                base_url: "https://vl.example.com/".to_string(),
                api_key: "sk-vl".to_string(),
                timeout_seconds: 60,
            },
        )]));
        gw.set_model_provider_mapping(HashMap::from([(
            "qwen-vl-max".to_string(),
            "prov-vl".to_string(),
        )]));
        let (b2, k2) = gw.resolve_endpoint("qwen-vl-max");
        assert_eq!(b2, "https://vl.example.com"); // 尾斜杠被裁剪
        assert_eq!(k2, "sk-vl");
        // 未命中的 model 仍回退
        let (b3, _) = gw.resolve_endpoint("deepseek-v4-flash");
        assert_eq!(b3, "http://fallback:3000");
    }
}
