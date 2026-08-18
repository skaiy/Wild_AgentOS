use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CenterConfig {
    pub url: String,
    #[serde(default)]
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
    #[serde(default = "default_llm_base_url")]
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default = "default_docker_socket")]
    pub docker_socket: String,
    #[serde(default = "default_sandbox_image")]
    pub default_image: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    #[serde(default = "default_graph_db_path")]
    pub db_path: String,
}



#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KernelConfig {
    #[serde(default = "default_kernel_target")]
    pub target: String,
    #[serde(default)]
    pub enabled: bool,
}

impl KernelConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.target.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub center: CenterConfig,
    pub llm: LlmConfig,
    pub sandbox: SandboxConfig,
    pub graph: GraphConfig,
    #[serde(default)]
    pub kernel: KernelConfig,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_env_files()?;

        let path = Self::config_path()?;
        let content = std::fs::read_to_string(&path)?;
        let mut config: Config = serde_yaml::from_str(&content)?;

        if let Ok(val) = std::env::var("LLM_API_KEY") {
            if !val.is_empty() {
                config.llm.api_key = val;
            }
        }
        if let Ok(val) = std::env::var("LLM_BASE_URL") {
            if !val.is_empty() {
                config.llm.base_url = val;
            }
        }
        if let Ok(val) = std::env::var("LLM_MODEL") {
            if !val.is_empty() {
                config.llm.model = val;
            }
        }
        if let Ok(val) = std::env::var("LLM_PROVIDER") {
            if !val.is_empty() {
                config.llm.provider = val;
            }
        }
        if let Ok(val) = std::env::var("CENTER_URL") {
            if !val.is_empty() {
                config.center.url = val;
            }
        }
        if let Ok(val) = std::env::var("CENTER_AUTH_TOKEN") {
            if !val.is_empty() {
                config.center.auth_token = val;
            }
        }

        Ok(config)
    }

    fn load_env_files() -> anyhow::Result<()> {
        let paths = [
            Self::env_path()?,
            Self::user_env_path(),
        ];
        for env_path in paths.iter() {
            if env_path.exists() {
                let env_content = std::fs::read_to_string(env_path)?;
                for line in env_content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        std::env::set_var(key.trim(), value.trim());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    fn env_path() -> anyhow::Result<std::path::PathBuf> {
        let cwd = std::env::current_dir()?;
        Ok(cwd.join(".env"))
    }

    fn user_env_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        std::path::PathBuf::from(home).join(".agentos").join(".env")
    }

    fn config_path() -> anyhow::Result<std::path::PathBuf> {
        let cwd = std::env::current_dir()?;
        Ok(cwd.join("config.yaml"))
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    7890
}

fn default_llm_provider() -> String {
    "openai".to_string()
}

fn default_llm_model() -> String {
    "gpt-4o".to_string()
}

fn default_llm_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_docker_socket() -> String {
    "/var/run/docker.sock".to_string()
}

fn default_sandbox_image() -> String {
    "ubuntu:22.04".to_string()
}

fn default_graph_db_path() -> String {
    "./data/graph".to_string()
}

fn default_kernel_target() -> String {
    "http://[::1]:50051".to_string()
}