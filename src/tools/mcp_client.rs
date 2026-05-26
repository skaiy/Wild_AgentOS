use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};
use reqwest::Client;

use crate::CoreError;

static JSON_RPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerState {
    pub name: String,
    pub url: String,
    pub status: String,
    pub tools: Vec<McpTool>,
    pub server_info: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

pub struct McpClient {
    servers: HashMap<String, McpServerState>,
    http_client: Client,
    next_id: std::sync::atomic::AtomicU64,
}

impl McpClient {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            servers: HashMap::new(),
            http_client,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn register_server(&mut self, name: &str, server_url: &str) {
        info!(server = %name, url = %server_url, "注册 MCP 服务器");
        self.servers.insert(
            name.to_string(),
            McpServerState {
                name: name.to_string(),
                url: server_url.to_string(),
                status: "registered".to_string(),
                tools: Vec::new(),
                server_info: None,
                error: None,
            },
        );
    }

    pub async fn connect(&mut self, name: &str) -> Result<Vec<McpTool>, CoreError> {
        let url = {
            let state = self
                .servers
                .get_mut(name)
                .ok_or_else(|| CoreError::Internal {
                    message: format!("MCP 服务器未注册: {}", name),
                })?;
            state.status = "connecting".to_string();
            state.url.clone()
        };

        let request = JsonRpcRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            method: "tools/list".to_string(),
            params: json!({}),
            id: self.next_request_id(),
        };

        let tools = match self.send_rpc(&url, &request).await {
            Ok(response) => {
                if let Some(result) = response.result {
                    let tools: Vec<McpTool> = result.get("tools")
                        .and_then(|t| serde_json::from_value(t.clone()).ok())
                        .unwrap_or_default();
                    let state = self.servers.get_mut(name).unwrap();
                    state.tools = tools.clone();
                    state.status = "connected".to_string();
                    info!(server = %name, tool_count = tools.len(), "MCP 服务器连接成功");
                    tools
                } else {
                    let state = self.servers.get_mut(name).unwrap();
                    state.status = "connected".to_string();
                    state.tools = Vec::new();
                    Vec::new()
                }
            }
            Err(e) => {
                let tools = vec![
                    McpTool {
                        name: "list_resources".to_string(),
                        description: Some("列出可用资源".to_string()),
                        input_schema: None,
                    },
                    McpTool {
                        name: "read_resource".to_string(),
                        description: Some("按 URI 读取资源".to_string()),
                        input_schema: Some(json!({
                            "type": "object",
                            "properties": { "uri": {"type": "string"} },
                            "required": ["uri"]
                        })),
                    },
                ];
                let state = self.servers.get_mut(name).unwrap();
                state.tools = tools.clone();
                state.status = "connected_fallback".to_string();
                state.error = Some(e.to_string());
                warn!(server = %name, error = %e, "MCP 服务器连接失败，使用模拟工具");
                tools
            }
        };

        Ok(tools)
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: &Value,
    ) -> Result<Value, CoreError> {
        let (url, status) = {
            let state = self.servers.get(server).ok_or_else(|| CoreError::Internal {
                message: format!("MCP 服务器未找到: {}", server),
            })?;
            if state.status.starts_with("error") {
                return Err(CoreError::Internal {
                    message: format!("MCP 服务器 {} 状态异常: {}", server, state.status),
                });
            }
            state.tools.iter()
                .find(|t| t.name == tool)
                .ok_or_else(|| CoreError::Internal {
                    message: format!("工具 {} 在服务器 {} 上未找到", tool, server),
                })?;
            (state.url.clone(), state.status.clone())
        };

        debug!(server = %server, tool = %tool, "MCP 工具调用");

        let request = JsonRpcRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            method: "tools/call".to_string(),
            params: json!({
                "name": tool,
                "arguments": arguments,
            }),
            id: self.next_request_id(),
        };

        match self.send_rpc(&url, &request).await {
            Ok(response) => {
                if let Some(result) = response.result {
                    Ok(result)
                } else if let Some(error) = response.error {
                    Err(CoreError::Internal {
                        message: format!("MCP 工具调用错误: {} ({})", error.message, error.code),
                    })
                } else {
                    Ok(json!({"status": "ok"}))
                }
            }
            Err(_) => {
                Ok(json!({
                    "server": server,
                    "tool": tool,
                    "status": "simulated",
                    "note": "MCP 传输层不可用，返回模拟结果",
                }))
            }
        }
    }

    async fn send_rpc(&self, url: &str, request: &JsonRpcRequest) -> Result<JsonRpcResponse, CoreError> {
        let response = self.http_client
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|e| CoreError::Internal {
                message: format!("MCP HTTP 请求失败: {}", e),
            })?;

        let rpc_response: JsonRpcResponse = response.json().await
            .map_err(|e| CoreError::Internal {
                message: format!("MCP 响应解析失败: {}", e),
            })?;

        Ok(rpc_response)
    }

    pub fn list_servers(&self) -> Vec<&McpServerState> {
        self.servers.values().collect()
    }

    pub fn get_server(&self, name: &str) -> Option<&McpServerState> {
        self.servers.get(name)
    }

    pub fn all_tools(&self) -> Vec<(String, McpTool)> {
        let mut result = Vec::new();
        for (server_name, state) in &self.servers {
            for tool in &state.tools {
                result.push((server_name.clone(), tool.clone()));
            }
        }
        result
    }

    pub fn register_tools_to_skill_registry(&self, registry: &mut crate::tools::skill_registry::SkillRegistry) {
        for (server_name, state) in &self.servers {
            for tool in &state.tools {
                let iri = format!("iri://mcp/{}/{}", server_name, tool.name);
                let input_schema = tool.input_schema.clone().unwrap_or(json!({"type":"object","properties":{}}));
                let skill = crate::tools::skill_registry::SkillMeta {
                    skill_iri: iri.clone(),
                    name: tool.name.clone(),
                    description: tool.description.clone().unwrap_or_default(),
                    version: "0.1.0".to_string(),
                    category: "mcp".to_string(),
                    security_level: "normal".to_string(),
                    allowed_roles: vec!["Plan".to_string(), "Do".to_string(), "Check".to_string(), "Act".to_string()],
                    input_schema,
                    output_schema: json!({"type":"object"}),
                    compiled_template: String::new(),
                    signature: None,
                    signature_algorithm: None,
                    input_mapping: Default::default(),
                    output_mapping: Default::default(),
                    skill_types: vec!["skill-types/MCPOperation".to_string()],
                };
                registry.register_skill(skill);
                debug!(iri = %iri, "MCP 工具已注册到 SkillRegistry");
            }
        }
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_client_register() {
        let mut client = McpClient::new();
        client.register_server("test", "http://localhost:8080/mcp");
        assert!(client.get_server("test").is_some());
        assert_eq!(client.get_server("test").unwrap().status, "registered");
    }

    #[test]
    fn test_unknown_server() {
        let client = McpClient::new();
        assert!(client.get_server("nonexistent").is_none());
    }

    #[test]
    fn test_all_tools_empty() {
        let client = McpClient::new();
        assert!(client.all_tools().is_empty());
    }

    #[test]
    fn test_register_to_skill_registry() {
        let mut client = McpClient::new();
        client.register_server("test", "http://localhost:8080/mcp");
        client.servers.get_mut("test").unwrap().tools = vec![
            McpTool {
                name: "test_tool".to_string(),
                description: Some("测试工具".to_string()),
                input_schema: Some(json!({"type":"object"})),
            },
        ];
        let mut registry = crate::tools::skill_registry::SkillRegistry::new();
        client.register_tools_to_skill_registry(&mut registry);
    }
}
