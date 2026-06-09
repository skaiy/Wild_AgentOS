use glidinghorse::config::GatewaySettings;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub gateway: GatewaySettings,
    pub model: String,
    pub workspace: String,
    pub max_iterations: u32,
    pub max_l1_mb: u64,
    pub max_l2_mb: u64,
    pub max_l3_mb: u64,
    pub data_dir: Option<String>,
    /// JSON-LD 工作流文件路径（可选，替代 LLM 生成的 plan）
    pub workflow_path: Option<String>,
}

impl CliConfig {
    pub fn from_env_and_args(model: String, workspace: String, max_iterations: u32, workflow_path: Option<String>) -> Self {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .or_else(|_| std::env::var("AGENT_OS_GATEWAY_API_KEY"))
            .unwrap_or_else(|_| {
                eprintln!("错误: 请设置 DEEPSEEK_API_KEY 或 AGENT_OS_GATEWAY_API_KEY 环境变量");
                std::process::exit(1);
            });

        let base_url = std::env::var("DEEPSEEK_API_URL")
            .or_else(|_| std::env::var("AGENT_OS_GATEWAY_BASE_URL"))
            .unwrap_or_else(|_| "https://api.deepseek.com".to_string());

        let model = if model.is_empty() {
            "deepseek-v4-flash".to_string()
        } else {
            model
        };

        let gateway = GatewaySettings {
            base_url,
            api_key,
            default_model: model.clone(),
            timeout_seconds: 300,
            max_retries: 2,
            model_mapping: HashMap::from([
                ("planning".to_string(), model.clone()),
                ("execution".to_string(), model.clone()),
                ("analysis".to_string(), model.clone()),
                ("default".to_string(), model.clone()),
            ]),
        };

        // Try to load memory limits from agent_os config file, fall back to env vars, then defaults
        let (max_l1_mb, max_l2_mb, max_l3_mb) = Self::load_memory_limits();
        let data_dir = std::env::var("GLIDING_HORSE_DATA").ok().or_else(|| {
            std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .map(|home| format!("{}/.gliding_horse/data", home))
        });

        Self {
            gateway,
            model,
            workspace,
            max_iterations,
            max_l1_mb,
            max_l2_mb,
            max_l3_mb,
            data_dir,
            workflow_path,
        }
    }

    /// Load memory limits from agent_os Settings (config file / env vars) or use defaults.
    fn load_memory_limits() -> (u64, u64, u64) {
        // First, try to load from the agent_os Settings config file
        if let Ok(settings) = glidinghorse::config::Settings::load() {
            return (
                settings.memory.l1.max_memory_mb,
                settings.memory.l2.max_memory_mb,
                settings.memory.l3.max_memory_mb,
            );
        }
        // Fall back to environment variables
        let l1 = std::env::var("AGENT_OS_L1_MEMORY_MB")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(512);
        let l2 = std::env::var("AGENT_OS_L2_MEMORY_MB")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(256);
        let l3 = std::env::var("AGENT_OS_L3_MEMORY_MB")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(256);
        (l1, l2, l3)
    }

    pub fn clone_with_model(&self, model: String) -> Self {
        let gateway = GatewaySettings {
            default_model: model.clone(),
            model_mapping: HashMap::from([
                ("planning".to_string(), model.clone()),
                ("execution".to_string(), model.clone()),
                ("analysis".to_string(), model.clone()),
                ("default".to_string(), model.clone()),
            ]),
            ..self.gateway.clone()
        };

        Self {
            gateway,
            model,
            workspace: self.workspace.clone(),
            max_iterations: self.max_iterations,
            max_l1_mb: self.max_l1_mb,
            max_l2_mb: self.max_l2_mb,
            max_l3_mb: self.max_l3_mb,
            data_dir: self.data_dir.clone(),
            workflow_path: self.workflow_path.clone(),
        }
    }

    pub fn clone_with_api_key(&self, api_key: String) -> Self {
        let mut gateway = self.gateway.clone();
        gateway.api_key = api_key;
        Self {
            gateway,
            model: self.model.clone(),
            workspace: self.workspace.clone(),
            max_iterations: self.max_iterations,
            max_l1_mb: self.max_l1_mb,
            max_l2_mb: self.max_l2_mb,
            max_l3_mb: self.max_l3_mb,
            data_dir: self.data_dir.clone(),
            workflow_path: self.workflow_path.clone(),
        }
    }

    pub fn clone_with_api_url(&self, api_url: String) -> Self {
        let mut gateway = self.gateway.clone();
        gateway.base_url = api_url;
        Self {
            gateway,
            model: self.model.clone(),
            workspace: self.workspace.clone(),
            max_iterations: self.max_iterations,
            max_l1_mb: self.max_l1_mb,
            max_l2_mb: self.max_l2_mb,
            max_l3_mb: self.max_l3_mb,
            data_dir: self.data_dir.clone(),
            workflow_path: self.workflow_path.clone(),
        }
    }
}