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
    #[serde(default)]
    pub token_optimization: TokenOptimizationSettings,
    #[serde(default)]
    pub batch_agents: BatchSettings,
    #[serde(default)]
    pub workspace: WorkspaceSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorkspaceSettings {
    /// 工作区根目录路径，为空时使用进程 CWD
    pub root: Option<String>,
    /// 文件扫描排除模式
    pub exclude_patterns: Vec<String>,
    /// 是否启用文件系统监听
    pub watch_enabled: bool,
    /// 内容缓存最大字节数
    pub content_store_max_bytes: usize,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            root: None,
            exclude_patterns: vec![
                "node_modules/".into(),
                "target/".into(),
                ".git/".into(),
                "dist/".into(),
                "build/".into(),
                "__pycache__/".into(),
                ".venv/".into(),
                "venv/".into(),
                ".next/".into(),
                "data/".into(),
                ".gliding_horse/".into(),
            ],
            watch_enabled: true,
            content_store_max_bytes: 64 * 1024 * 1024,
        }
    }
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
    #[serde(default)]
    pub max_memory_mb: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L2Settings {
    pub max_node_size: usize,
    pub max_projection_size: usize,
    #[serde(default)]
    pub max_memory_mb: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct L3Settings {
    pub default_frame: String,
    pub max_size: usize,
    #[serde(default)]
    pub max_memory_mb: u64,
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
    #[serde(default = "default_max_pdca_cycles")]
    pub max_pdca_cycles: u32,
}

fn default_max_pdca_cycles() -> u32 { 7 }

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
            max_pdca_cycles: 7,
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

impl LoggingSettings {
    pub fn test_default(prefix: &str) -> Self {
        Self {
            level: "debug".to_string(),
            format: "text".to_string(),
            console_output: true,
            file_output: FileOutputSettings {
                enabled: true,
                path: "./logs".to_string(),
                prefix: prefix.to_string(),
                rotation: "daily".to_string(),
                max_files: 10,
            },
            filters: vec![
                LogFilter { module: "glidinghorse::core".to_string(), level: "debug".to_string() },
                LogFilter { module: "glidinghorse::gateway".to_string(), level: "debug".to_string() },
                LogFilter { module: "glidinghorse::memory".to_string(), level: "info".to_string() },
                LogFilter { module: "glidinghorse::tools".to_string(), level: "info".to_string() },
                LogFilter { module: "redb".to_string(), level: "warn".to_string() },
            ],
            sensitive_fields: vec![
                "api_key".to_string(),
                "password".to_string(),
            ],
        }
    }
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
                LogFilter { module: "glidinghorse::gateway".to_string(), level: "debug".to_string() },
                LogFilter { module: "glidinghorse::core".to_string(), level: "debug".to_string() },
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
    pub micro_tool_threshold: usize,
    pub preview_size: usize,
    pub max_graph_entities: usize,
    pub max_micro_tools: usize,
    pub sparql_query_timeout_ms: u64,
    pub auto_cleanup: bool,
    /// PassThrough 的结果超过此字节数时也持久化并注册 micro-tool，
    /// 为将来上下文压力下的引用式回收做准备。
    #[serde(default = "default_prepare_threshold")]
    pub prepare_threshold: usize,
}

fn default_prepare_threshold() -> usize { 3072 }

impl Default for ToolResultRouterSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_small: 16384,
            threshold_large: 32768,
            micro_tool_threshold: 16384,
            preview_size: 2000,
            max_graph_entities: 500,
            max_micro_tools: 5,
            sparql_query_timeout_ms: 100,
            auto_cleanup: true,
            prepare_threshold: default_prepare_threshold(),
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

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TokenOptimizationSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tool_groups: ToolGroupSettings,
    #[serde(default)]
    pub tool_result_compressor: ToolResultCompressorSettings,
    #[serde(default)]
    pub context_window: ContextWindowSettings,
    #[serde(default)]
    pub tool_result_aging: ToolResultAgingSettings,
    #[serde(default)]
    pub prompt_optimization: PromptOptimizationSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolGroupSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub roles: std::collections::HashMap<String, RoleToolConfig>,
}

impl Default for ToolGroupSettings {
    fn default() -> Self {
        let mut roles = std::collections::HashMap::new();
        roles.insert("Plan".to_string(), RoleToolConfig {
            default: vec!["Core".to_string(), "Search".to_string(), "Knowledge".to_string(), "System".to_string()],
            on_demand: vec!["Web".to_string(), "Code".to_string(), "Skill".to_string()],
        });
        roles.insert("Do".to_string(), RoleToolConfig {
            default: vec!["Core".to_string(), "Write".to_string(), "Search".to_string(), "Web".to_string(), "Code".to_string(), "Skill".to_string(), "System".to_string()],
            on_demand: vec!["Knowledge".to_string()],
        });
        roles.insert("Check".to_string(), RoleToolConfig {
            default: vec!["Core".to_string(), "Search".to_string(), "Knowledge".to_string(), "System".to_string()],
            on_demand: vec!["Web".to_string(), "Code".to_string()],
        });
        roles.insert("Act".to_string(), RoleToolConfig {
            default: vec!["Core".to_string(), "System".to_string()],
            on_demand: vec!["Search".to_string(), "Knowledge".to_string()],
        });
        Self { enabled: true, roles }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoleToolConfig {
    #[serde(default)]
    pub default: Vec<String>,
    #[serde(default)]
    pub on_demand: Vec<String>,
}

impl Default for RoleToolConfig {
    fn default() -> Self {
        Self { default: vec![], on_demand: vec![] }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolResultCompressorSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_full_results")]
    pub max_full_results: usize,
    #[serde(default = "default_max_summary_length")]
    pub max_summary_length: usize,
    #[serde(default = "default_compression_trigger")]
    pub compression_trigger: usize,
    /// Tool 消息内容超过此字节数时，若存在对应 micro-tool 则替换为引用式压缩。
    #[serde(default = "default_compress_tool_result_threshold")]
    pub compress_tool_result_threshold: usize,
}

fn default_compress_tool_result_threshold() -> usize { 500 }

fn default_max_full_results() -> usize { 2 }
fn default_max_summary_length() -> usize { 200 }
fn default_compression_trigger() -> usize { 10 }

impl Default for ToolResultCompressorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_full_results: default_max_full_results(),
            max_summary_length: default_max_summary_length(),
            compression_trigger: default_compression_trigger(),
            compress_tool_result_threshold: default_compress_tool_result_threshold(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ContextWindowSettings {
    #[serde(default = "default_max_messages")]
    pub max_messages: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_compression_ratio")]
    pub compression_ratio: f32,
    #[serde(default = "default_preserve_recent")]
    pub preserve_recent: usize,
}

fn default_max_messages() -> usize { 30 }
fn default_max_tokens() -> usize { 16000 }
fn default_compression_ratio() -> f32 { 0.3 }
fn default_preserve_recent() -> usize { 4 }

impl Default for ContextWindowSettings {
    fn default() -> Self {
        Self {
            max_messages: default_max_messages(),
            max_tokens: default_max_tokens(),
            compression_ratio: default_compression_ratio(),
            preserve_recent: default_preserve_recent(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolResultAgingSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 保留完整结果的数量（最新的 N 个 tool 结果保持完整）
    #[serde(default = "default_aging_keep_full")]
    pub keep_full: usize,
    /// 对旧结果尝试微工具引用的数量（在 keep_full 之后尝试引用式压缩的数量）
    #[serde(default = "default_aging_try_microtool")]
    pub try_microtool: usize,
    /// 压缩阈值: 超过此字节数的 tool 消息才被处理
    #[serde(default = "default_aging_compress_threshold")]
    pub compress_threshold: usize,
}

fn default_aging_keep_full() -> usize { 5 }
fn default_aging_try_microtool() -> usize { 5 }
fn default_aging_compress_threshold() -> usize { 500 }

impl Default for ToolResultAgingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_full: default_aging_keep_full(),
            try_microtool: default_aging_try_microtool(),
            compress_threshold: default_aging_compress_threshold(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct PromptOptimizationSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub use_layered_prompts: bool,
    #[serde(default = "default_true")]
    pub store_specs_in_kg: bool,
}

impl Default for PromptOptimizationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            use_layered_prompts: true,
            store_specs_in_kg: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct BatchSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_batch_default_model")]
    pub default_model: String,
    #[serde(default = "default_batch_temperature")]
    pub default_temperature: f32,
    #[serde(default = "default_batch_max_retries")]
    pub default_max_retries: u32,
    #[serde(default = "default_true")]
    pub inject_user_reminders: bool,
    #[serde(default = "default_true")]
    pub inject_context_summary: bool,
    #[serde(default = "default_true")]
    pub inject_related_entities: bool,
    #[serde(default)]
    pub agents: Vec<BatchAgentSettings>,
}

fn default_batch_default_model() -> String { "deepseek-v4-flash".to_string() }
fn default_batch_temperature() -> f32 { 0.1 }
fn default_batch_max_retries() -> u32 { 3 }

#[derive(Debug, Deserialize, Clone)]
pub struct BatchAgentSettings {
    pub name: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub window_type: Option<String>,
    pub window_max_messages: Option<usize>,
    pub window_max_seconds: Option<u64>,
    #[serde(default)]
    pub triggers: Vec<BatchTriggerSettings>,
    #[serde(default)]
    pub prompt_source: String,
    pub prompt_template_name: Option<String>,
    pub prompt_template_path: Option<String>,
    pub business_domain: String,
    #[serde(default)]
    pub entity_types: Vec<String>,
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub intent_types: Vec<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_retries: Option<u32>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub emit_on: Vec<String>,
    #[serde(default = "default_true")]
    pub inject_user_reminders: bool,
    #[serde(default = "default_true")]
    pub inject_context_summary: bool,

    // Maintenance Agent specific options
    #[serde(default)]
    pub min_confidence_auto_apply: Option<f64>,
    #[serde(default)]
    pub batch_size: Option<usize>,
    #[serde(default)]
    pub max_candidates: Option<usize>,
    #[serde(default)]
    pub lookback_hours: Option<u64>,
    #[serde(default)]
    pub llm_analysis_threshold: Option<f64>,
    #[serde(default)]
    pub max_items_per_run: Option<usize>,
    #[serde(default)]
    pub max_suggestions_per_run: Option<usize>,
}

impl Default for BatchAgentSettings {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            enabled: true,
            window_type: None,
            window_max_messages: Some(5),
            window_max_seconds: Some(600),
            triggers: vec![],
            prompt_source: "HybridWithTemplate".to_string(),
            prompt_template_name: None,
            prompt_template_path: None,
            business_domain: "default".to_string(),
            entity_types: vec![],
            relation_types: vec![],
            intent_types: vec![],
            model: None,
            temperature: None,
            max_retries: None,
            timeout_seconds: None,
            emit_on: vec![],
            inject_user_reminders: true,
            inject_context_summary: true,
            min_confidence_auto_apply: None,
            batch_size: None,
            max_candidates: None,
            lookback_hours: None,
            llm_analysis_threshold: None,
            max_items_per_run: None,
            max_suggestions_per_run: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct BatchTriggerSettings {
    pub trigger_type: String,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

impl Default for BatchTriggerSettings {
    fn default() -> Self {
        Self {
            trigger_type: "WindowFull".to_string(),
            params: std::collections::HashMap::new(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway: GatewaySettings {
                base_url: String::new(),
                api_key: String::new(),
                default_model: String::new(),
                timeout_seconds: 60,
                max_retries: 3,
                model_mapping: std::collections::HashMap::new(),
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
                    max_memory_mb: 0,
                },
                l2: L2Settings {
                    max_node_size: 5_242_880,
                    max_projection_size: 500,
                    max_memory_mb: 0,
                },
                l3: L3Settings {
                    default_frame: "summary_only".to_string(),
                    max_size: 500,
                    max_memory_mb: 0,
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
                    "CycleTimeout".to_string(),
                    "AgentBlocked".to_string(),
                    "ResourceConflict".to_string(),
                    "QualityDegradation".to_string(),
                    "UserFeedback".to_string(),
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
                max_pdca_cycles: 7,
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
            token_optimization: TokenOptimizationSettings::default(),
            batch_agents: BatchSettings::default(),
            workspace: WorkspaceSettings::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name("config").required(false))
            // 运行期覆盖文件（由 PUT /api/v1/config 写入）：优先级高于 config.yaml，低于环境变量。
            // 格式为 JSON，路径：data/config_override.json
            .add_source(config::File::with_name("data/config_override").required(false))
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
            tracing::warn!("gateway.base_url is not set. LLM features will be unavailable until configured via UI.");
        }
        if self.gateway.api_key.is_empty() {
            tracing::warn!("gateway.api_key is not set. LLM features will be unavailable until configured via UI or env var.");
        }

        if self.agents.max_iterations == 0 {
            return Err("agents.max_iterations must be > 0".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_settings_test_default() {
        let settings = LoggingSettings::test_default("test_prefix");
        assert_eq!(settings.level, "debug");
        assert_eq!(settings.format, "text");
        assert!(settings.console_output);
        assert!(settings.file_output.enabled);
        assert_eq!(settings.file_output.prefix, "test_prefix");
        assert!(settings.filters.iter().any(|f| f.module == "redb" && f.level == "warn"));
        assert!(settings.filters.iter().any(|f| f.module == "glidinghorse::core" && f.level == "debug"));
        assert!(settings.filters.iter().any(|f| f.module == "glidinghorse::memory" && f.level == "info"));
    }

    #[test]
    fn test_logging_settings_default_has_redb_in_init() {
        let settings = LoggingSettings::default();
        assert_eq!(settings.level, "info");
    }
}
