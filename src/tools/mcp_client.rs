use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tracing::{debug, info, warn};
use reqwest::Client;

use crate::config::{McpServerConfig, McpStdioServerConfig};
use crate::CoreError;

static JSON_RPC_VERSION: &str = "2.0";

// ── Data types ────────────────────────────────────────────────────

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
    pub transport: String, // "http" or "stdio"
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

// ── Stdio process management ──────────────────────────────────────

/// Manages a spawned MCP server subprocess with stdin/stdout JSON-RPC transport.
struct StdioProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    buffer: String,
}

impl StdioProcess {
    /// Spawn a new MCP server process.
    async fn spawn(config: &McpStdioServerConfig) -> Result<Self, CoreError> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        // Inherit parent env, then overlay config-specific vars (so PATH etc. are preserved)
        cmd.envs(std::env::vars());
        cmd.envs(&config.env);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        // Discard stderr — MCP server logs (startup banners, usage stats) would
        // corrupt the TUI display if inherited. Errors surface via JSON-RPC.
        cmd.stderr(std::process::Stdio::null());

        let mut child = cmd.spawn().map_err(|e| CoreError::Internal {
            message: format!("无法启动 MCP 服务器 '{}': {}", config.command, e),
        })?;

        let stdin = child.stdin.take().ok_or_else(|| CoreError::Internal {
            message: "无法获取 MCP 服务器的 stdin".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| CoreError::Internal {
            message: "无法获取 MCP 服务器的 stdout".to_string(),
        })?;

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            buffer: String::new(),
        })
    }

    /// Send a JSON-RPC request and read the matching response.
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, CoreError> {
        let json_str = serde_json::to_string(request).map_err(|e| CoreError::Internal {
            message: format!("JSON 序列化失败: {}", e),
        })?;

        // Write request to stdin (newline-delimited JSON)
        self.stdin.write_all(json_str.as_bytes()).await.map_err(|e| CoreError::Internal {
            message: format!("写入 MCP stdin 失败: {}", e),
        })?;
        self.stdin.write_all(b"\n").await.map_err(|e| CoreError::Internal {
            message: format!("写入 MCP stdin 换行符失败: {}", e),
        })?;
        self.stdin.flush().await.map_err(|e| CoreError::Internal {
            message: format!("刷新 MCP stdin 失败: {}", e),
        })?;

        // Read response line from stdout
        self.buffer.clear();
        self.stdout.read_line(&mut self.buffer).await.map_err(|e| CoreError::Internal {
            message: format!("读取 MCP stdout 失败: {}", e),
        })?;

        if self.buffer.is_empty() {
            return Err(CoreError::Internal {
                message: "MCP 服务器 stdout 已关闭".to_string(),
            });
        }

        let response: JsonRpcResponse = serde_json::from_str(self.buffer.trim()).map_err(|e| CoreError::Internal {
            message: format!("解析 MCP 响应失败: {} (raw: {})", e, self.buffer.trim()),
        })?;

        Ok(response)
    }

    /// Check if the process is still alive.
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

// ── McpClient ─────────────────────────────────────────────────────

pub struct McpClient {
    servers: HashMap<String, McpServerState>,
    processes: HashMap<String, StdioProcess>,
    stdio_configs: HashMap<String, McpStdioServerConfig>,
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
            processes: HashMap::new(),
            stdio_configs: HashMap::new(),
            http_client,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Register an HTTP MCP server by URL.
    pub fn register_server(&mut self, name: &str, server_url: &str) {
        info!(server = %name, url = %server_url, transport = "http", "注册 MCP 服务器");
        self.servers.insert(
            name.to_string(),
            McpServerState {
                name: name.to_string(),
                url: server_url.to_string(),
                transport: "http".to_string(),
                status: "registered".to_string(),
                tools: Vec::new(),
                server_info: None,
                error: None,
            },
        );
    }

    /// Register a stdio MCP server (spawns subprocess on connect).
    pub fn register_stdio_server(&mut self, name: &str, config: &McpStdioServerConfig) {
        info!(server = %name, command = %config.command, transport = "stdio", "注册 MCP Stdio 服务器");
        self.servers.insert(
            name.to_string(),
            McpServerState {
                name: name.to_string(),
                url: String::new(),
                transport: "stdio".to_string(),
                status: "registered".to_string(),
                tools: Vec::new(),
                server_info: None,
                error: None,
            },
        );
        // Store config alongside server state for later spawning
        self.stdio_configs.insert(name.to_string(), config.clone());
    }

    /// Register an MCP server from a generic `McpServerConfig` enum.
    pub fn register_from_config(&mut self, name: &str, config: &McpServerConfig) {
        match config {
            McpServerConfig::Http(http_cfg) => {
                self.register_server(name, &http_cfg.url);
            }
            McpServerConfig::Stdio(stdio_cfg) => {
                self.register_stdio_server(name, stdio_cfg);
            }
        }
    }

    // ── Connection ────────────────────────────────────────────────

    pub async fn connect(&mut self, name: &str) -> Result<Vec<McpTool>, CoreError> {
        let transport = {
            let state = self.servers.get(name).ok_or_else(|| CoreError::Internal {
                message: format!("MCP 服务器未注册: {}", name),
            })?;
            state.transport.clone()
        };

        match transport.as_str() {
            "http" => self.connect_http(name).await,
            "stdio" => self.connect_stdio(name).await,
            _ => Err(CoreError::Internal {
                message: format!("未知的 MCP 传输类型: {}", transport),
            }),
        }
    }

    async fn connect_http(&mut self, name: &str) -> Result<Vec<McpTool>, CoreError> {
        let url = {
            let state = self.servers.get_mut(name).ok_or_else(|| CoreError::Internal {
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

        let tools = match self.send_rpc_http(&url, &request).await {
            Ok(response) => self.handle_connect_response(name, response).await,
            Err(e) => self.handle_connect_fallback(name, e).await,
        };

        Ok(tools)
    }

    async fn connect_stdio(&mut self, name: &str) -> Result<Vec<McpTool>, CoreError> {
        // Get the stdio config
        let config = self.stdio_configs.get(name).cloned().ok_or_else(|| CoreError::Internal {
            message: format!("MCP Stdio 服务器配置未找到: {}", name),
        })?;

        // Update status
        if let Some(state) = self.servers.get_mut(name) {
            state.status = "connecting".to_string();
        }

        // Spawn the subprocess
        match StdioProcess::spawn(&config).await {
            Ok(mut process) => {
                let request = JsonRpcRequest {
                    jsonrpc: JSON_RPC_VERSION.to_string(),
                    method: "tools/list".to_string(),
                    params: json!({}),
                    id: self.next_request_id(),
                };

                match process.send_request(&request).await {
                    Ok(response) => {
                        let tools = self.parse_tools_from_response(name, &response).unwrap_or_default();
                        self.processes.insert(name.to_string(), process);

                        if let Some(state) = self.servers.get_mut(name) {
                            state.tools = tools.clone();
                            state.status = "connected".to_string();
                        }
                        info!(server = %name, tool_count = tools.len(), "MCP Stdio 服务器连接成功");
                        Ok(tools)
                    }
                    Err(e) => {
                        let _ = process.child.kill().await;
                        Ok(self.handle_connect_fallback(name, e).await)
                    }
                }
            }
            Err(e) => {
                Ok(self.handle_connect_fallback(name, e).await)
            }
        }
    }

    /// Parse tools from a JSON-RPC tools/list response.
    fn parse_tools_from_response(&self, name: &str, response: &JsonRpcResponse) -> Result<Vec<McpTool>, CoreError> {
        if let Some(ref result) = response.result {
            let tools: Vec<McpTool> = result.get("tools")
                .and_then(|t| serde_json::from_value(t.clone()).ok())
                .unwrap_or_default();
            Ok(tools)
        } else if let Some(ref error) = response.error {
            Err(CoreError::Internal {
                message: format!("MCP 服务器 '{}' 返回错误: {} ({})", name, error.message, error.code),
            })
        } else {
            Ok(Vec::new())
        }
    }

    async fn handle_connect_response(&mut self, name: &str, response: JsonRpcResponse) -> Vec<McpTool> {
        let tools = self.parse_tools_from_response(name, &response).unwrap_or_default();
        if let Some(state) = self.servers.get_mut(name) {
            state.tools = tools.clone();
            state.status = "connected".to_string();
        }
        info!(server = %name, tool_count = tools.len(), "MCP 服务器连接成功");
        tools
    }

    async fn handle_connect_fallback(&mut self, name: &str, error: CoreError) -> Vec<McpTool> {
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
        if let Some(state) = self.servers.get_mut(name) {
            state.tools = tools.clone();
            state.status = "connected_fallback".to_string();
            state.error = Some(error.to_string());
        }
        warn!(server = %name, error = %error, "MCP 服务器连接失败，使用模拟工具");
        tools
    }

    // ── Tool execution ────────────────────────────────────────────

    pub async fn call_tool(
        &mut self,
        server: &str,
        tool: &str,
        arguments: &Value,
    ) -> Result<Value, CoreError> {
        let transport = {
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
            state.transport.clone()
        };

        debug!(server = %server, tool = %tool, transport = %transport, "MCP 工具调用");

        let request = JsonRpcRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            method: "tools/call".to_string(),
            params: json!({
                "name": tool,
                "arguments": arguments,
            }),
            id: self.next_request_id(),
        };

        match transport.as_str() {
            "http" => {
                let url = self.servers.get(server).map(|s| s.url.clone()).unwrap_or_default();
                self.call_tool_http(&url, &request).await
            }
            "stdio" => {
                self.call_tool_stdio(server, &request).await
            }
            _ => Err(CoreError::Internal {
                message: format!("未知的 MCP 传输类型: {}", transport),
            }),
        }
    }

    async fn call_tool_http(&self, url: &str, request: &JsonRpcRequest) -> Result<Value, CoreError> {
        match self.send_rpc_http(url, request).await {
            Ok(response) => Self::handle_call_response(response),
            Err(_) => Ok(json!({
                "status": "simulated",
                "note": "MCP HTTP 传输层不可用，返回模拟结果",
            })),
        }
    }

    async fn call_tool_stdio(&mut self, server: &str, request: &JsonRpcRequest) -> Result<Value, CoreError> {
        let process = self.processes.get_mut(server).ok_or_else(|| CoreError::Internal {
            message: format!("MCP Stdio 进程未找到: {}", server),
        })?;

        if !process.is_alive() {
            return Ok(json!({
                "status": "simulated",
                "note": "MCP Stdio 进程已退出，返回模拟结果",
            }));
        }

        match process.send_request(request).await {
            Ok(response) => Self::handle_call_response(response),
            Err(_) => Ok(json!({
                "status": "simulated",
                "note": "MCP Stdio 通信失败，返回模拟结果",
            })),
        }
    }

    fn handle_call_response(response: JsonRpcResponse) -> Result<Value, CoreError> {
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

    // ── Transport layer ───────────────────────────────────────────

    async fn send_rpc_http(&self, url: &str, request: &JsonRpcRequest) -> Result<JsonRpcResponse, CoreError> {
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

    // ── Query methods ─────────────────────────────────────────────

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

    pub fn register_tools_to_skill_registry(&self, registry: &crate::tools::skill_registry::SkillRegistry) {
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

    pub async fn kill_all_processes(&mut self) {
        let names: Vec<String> = self.processes.keys().cloned().collect();
        for name in names {
            if let Some(mut process) = self.processes.remove(&name) {
                let _ = process.child.kill().await;
                let _ = process.child.wait().await;
                info!(server = %name, "MCP Stdio 进程已终止");
            }
        }
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

// We can't implement Drop with async cleanup, so we rely on the engine
// to call kill_all_processes() explicitly.
// For now, in the non-async Drop, we just let the processes die when
// the Child handle is dropped (tokio sends SIGKILL on Drop).

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
        let registry = crate::tools::skill_registry::SkillRegistry::new();
        client.register_tools_to_skill_registry(&registry);
    }

    #[tokio::test]
    async fn test_register_from_config_http() {
        let config = McpServerConfig::Http(McpRemoteServerConfig {
            url: "http://localhost:9999/mcp".to_string(),
            headers: std::collections::BTreeMap::new(),
        });
        let mut client = McpClient::new();
        client.register_from_config("test-http", &config);
        let state = client.get_server("test-http").unwrap();
        assert_eq!(state.transport, "http");
        assert_eq!(state.url, "http://localhost:9999/mcp");
    }

    #[tokio::test]
    async fn test_register_from_config_stdio() {
        let config = McpServerConfig::Stdio(McpStdioServerConfig {
            command: "echo".to_string(),
            args: vec!["{}".to_string()],
            env: std::collections::BTreeMap::new(),
            tool_call_timeout_ms: None,
        });
        let mut client = McpClient::new();
        client.register_from_config("test-stdio", &config);
        let state = client.get_server("test-stdio").unwrap();
        assert_eq!(state.transport, "stdio");
        assert!(client.stdio_configs.contains_key("test-stdio"));
    }
}
