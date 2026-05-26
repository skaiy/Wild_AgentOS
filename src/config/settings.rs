use anyhow::Result;
use serde::Deserialize;
use config::{Config, ConfigError, Environment};

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub gateway: GatewaySettings,
    pub memory: MemorySettings,
    pub perception: PerceptionSettings,
    pub agents: AgentSettings,
    pub api: ApiSettings,
    pub output: OutputSettings,
    pub emphasis: EmphasisConfig,
    pub logging: LoggingSettings,
    pub tool_result_router: ToolResultRouterSettings,
    #[serde(default)]
    pub embedding: EmbeddingSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GatewaySettings {
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub model_mapping: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MemorySettings {
    pub l0: L0Settings,
    pub l1: L1Settings,
    pub l2: L2Settings,
    pub l3: L3Settings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L0Settings {
    pub path: String,
    pub max_entries: u64,
    pub compression: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L1Settings {
    pub max_messages: usize,
    pub compression_threshold: usize,
    pub max_tokens: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L2Settings {
    pub max_node_size: usize,
    pub max_projection_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L3Settings {
    pub default_frame: String,
    pub max_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PerceptionSettings {
    pub enabled: bool,
    pub triggers: Vec<String>,
    pub cache_ttl_seconds: u64,
    pub cache_max_entries: usize,
    pub anomaly_dedup_window_seconds: u64,
    #[serde(default = "default_simple_threshold")]
    pub simple_input_threshold: usize,
    #[serde(default = "default_medium_threshold")]
    pub medium_input_threshold: usize,
    #[serde(default = "default_cycle_timeout_secs")]
    pub cycle_timeout_secs: u64,
    #[serde(default = "default_max_iterations_before_alert")]
    pub max_iterations_before_alert: usize,
    #[serde(default = "default_error_rate_threshold")]
    pub error_rate_threshold: f64,
}

fn default_simple_threshold() -> usize { 50 }
fn default_medium_threshold() -> usize { 200 }
fn default_cycle_timeout_secs() -> u64 { 300 }
fn default_max_iterations_before_alert() -> usize { 10 }
fn default_error_rate_threshold() -> f64 { 0.5 }

#[derive(Debug, Deserialize, Clone)]
pub struct AgentSettings {
    pub max_iterations: u32,
    pub parallel_execution: bool,
    pub max_parallel_agents: usize,
    pub timeout_seconds: u64,
    pub api_timeout_seconds: u64,
    pub event_bus_capacity: usize,
    pub template_path: Option<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            parallel_execution: true,
            max_parallel_agents: 10,
            timeout_seconds: 300,
            api_timeout_seconds: 120,
            event_bus_capacity: 100,
            template_path: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiSettings {
    pub grpc_addr: String,
    pub http_addr: String,
    pub enable_metrics: bool,
    pub metrics_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OutputSettings {
    pub directory: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmphasisConfig {
    pub enabled: bool,
    pub extraction_prompt: String,
    pub max_items: usize,
    pub dedup_threshold: f64,
}

impl Default for EmphasisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extraction_prompt: r#"## 强调内容提取
如果用户输入中包含强调性质的内容（如"必须"、"重要"、"不要忘记"、"关键"等），
请将这些内容提取出来，放在 JSON 的 "emphasis" 字段中（字符串数组）。

示例：
{
  "thought": "用户强调了必须使用异步方式...",
  "content": "好的，我会...",
  "summary": "确认异步实现",
  "emphasis": ["必须使用异步方式实现"]
}"#.to_string(),
            max_items: 50,
            dedup_threshold: 0.85,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSettings {
    pub level: String,
    pub format: String,
    pub console_output: bool,
    pub file_output: FileOutputSettings,
    pub filters: Vec<LogFilter>,
    pub sensitive_fields: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FileOutputSettings {
    pub enabled: bool,
    pub path: String,
    pub prefix: String,
    pub rotation: String,
    pub max_files: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LogFilter {
    pub module: String,
    pub level: String,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "text".to_string(),
            console_output: true,
            file_output: FileOutputSettings {
                enabled: true,
                path: "./logs".to_string(),
                prefix: "agent_os".to_string(),
                rotation: "daily".to_string(),
                max_files: 30,
            },
            filters: vec![
                LogFilter { module: "agent_os::gateway".to_string(), level: "debug".to_string() },
                LogFilter { module: "agent_os::core".to_string(), level: "debug".to_string() },
            ],
            sensitive_fields: vec![
                "api_key".to_string(),
                "password".to_string(),
                "token".to_string(),
                "secret".to_string(),
            ],
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolResultRouterSettings {
    pub enabled: bool,
    pub threshold_small: usize,
    pub threshold_large: usize,
    pub preview_size: usize,
    pub max_graph_entities: usize,
    pub max_micro_tools: usize,
    pub sparql_query_timeout_ms: u64,
    pub auto_cleanup: bool,
}

impl Default for ToolResultRouterSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_small: 2048,
            threshold_large: 8192,
            preview_size: 2000,
            max_graph_entities: 500,
            max_micro_tools: 5,
            sparql_query_timeout_ms: 100,
            auto_cleanup: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub ollama: OllamaEmbeddingConfig,
    #[serde(default)]
    pub oneapi: OneApiEmbeddingConfig,
    #[serde(default)]
    pub fallback: FallbackEmbeddingConfig,
}

fn default_true() -> bool { true }
fn default_provider() -> String { "ollama".to_string() }

#[derive(Debug, Deserialize, Clone)]
pub struct OllamaEmbeddingConfig {
    #[serde(default = "default_ollama_url")]
    pub base_url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
    #[serde(default = "default_ollama_dim")]
    pub dimension: usize,
}

fn default_ollama_url() -> String { "http://localhost:11434".to_string() }
fn default_ollama_model() -> String { "nomic-embed-text".to_string() }
fn default_ollama_dim() -> usize { 768 }

impl Default for OllamaEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_url(),
            model: default_ollama_model(),
            dimension: default_ollama_dim(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct OneApiEmbeddingConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_oneapi_model")]
    pub model: String,
    #[serde(default = "default_oneapi_dim")]
    pub dimension: usize,
}

fn default_oneapi_model() -> String { "text-embedding-3-small".to_string() }
fn default_oneapi_dim() -> usize { 1536 }

impl Default for OneApiEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            model: default_oneapi_model(),
            dimension: default_oneapi_dim(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct FallbackEmbeddingConfig {
    #[serde(default = "default_fallback_dim")]
    pub dimension: usize,
}

fn default_fallback_dim() -> usize { 128 }

impl Default for FallbackEmbeddingConfig {
    fn default() -> Self {
        Self { dimension: default_fallback_dim() }
    }
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: default_provider(),
            ollama: OllamaEmbeddingConfig::default(),
            oneapi: OneApiEmbeddingConfig::default(),
            fallback: FallbackEmbeddingConfig::default(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway: GatewaySettings {
                base_url: "http://localhost:3000".to_string(),
                api_key: String::new(),
                default_model: "gpt-3.5-turbo".to_string(),
                timeout_seconds: 30,
                max_retries: 3,
                model_mapping: std::collections::HashMap::from([
                    ("planning".to_string(), "gpt-4-turbo-preview".to_string()),
                    ("execution".to_string(), "gpt-4-turbo-preview".to_string()),
                    ("analysis".to_string(), "gpt-3.5-turbo".to_string()),
                    ("default".to_string(), "gpt-3.5-turbo".to_string()),
                ]),
            },
            memory: MemorySettings {
                l0: L0Settings {
                    path: "./data/l0".to_string(),
                    max_entries: 1_000_000,
                    compression: true,
                },
                l1: L1Settings {
                    max_messages: 100,
                    compression_threshold: 50,
                    max_tokens: 4096,
                },
                l2: L2Settings {
                    max_node_size: 2048,
                    max_projection_size: 500,
                },
                l3: L3Settings {
                    default_frame: "summary_only".to_string(),
                    max_size: 500,
                },
            },
            perception: PerceptionSettings {
                enabled: true,
                triggers: vec![
                    "TaskStart".to_string(),
                    "PlanCompleted".to_string(),
                    "ProgressAnomaly".to_string(),
                    "CheckCompleted".to_string(),
                    "TaskEnd".to_string(),
                ],
                cache_ttl_seconds: 300,
                cache_max_entries: 1000,
                anomaly_dedup_window_seconds: 60,
                simple_input_threshold: 50,
                medium_input_threshold: 200,
                cycle_timeout_secs: 300,
                max_iterations_before_alert: 10,
                error_rate_threshold: 0.5,
            },
            agents: AgentSettings {
                max_iterations: 10,
                parallel_execution: true,
                max_parallel_agents: 10,
                timeout_seconds: 300,
                api_timeout_seconds: 120,
                event_bus_capacity: 100,
                template_path: None,
            },
            api: ApiSettings {
                grpc_addr: "0.0.0.0:50051".to_string(),
                http_addr: "0.0.0.0:8080".to_string(),
                enable_metrics: true,
                metrics_port: 9090,
            },
            output: OutputSettings {
                directory: "./data/output".to_string(),
            },
            emphasis: EmphasisConfig::default(),
            logging: LoggingSettings::default(),
            tool_result_router: ToolResultRouterSettings::default(),
            embedding: EmbeddingSettings::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(
                Environment::with_prefix("AGENT_OS")
                    .separator("_")
                    .try_parsing(true)
            )
            .build()?;

        config.try_deserialize()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.gateway.base_url.is_empty() {
            return Err("gateway.base_url must be set".to_string());
        }
        if self.gateway.api_key.is_empty() {
            return Err("gateway.api_key must be set (via config.yaml or AGENT_OS_GATEWAY_API_KEY)".to_string());
        }
        if self.gateway.default_model.is_empty() {
            return Err("gateway.default_model must be set".to_string());
        }
        if self.agents.max_iterations == 0 {
            return Err("agents.max_iterations must be > 0".to_string());
        }
        Ok(())
    }
}
