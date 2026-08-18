use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions,
    RemoveContainerOptions, StopContainerOptions, LogOutput,
};
use bollard::models::HostConfig;
use bollard::image::CreateImageOptions;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::API_DEFAULT_VERSION;
use futures_util::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub docker_socket: String,
    pub default_image: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            docker_socket: "unix:///var/run/docker.sock".to_string(),
            default_image: "python:3.12-slim".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInstance {
    pub container_id: String,
    pub task_id: String,
    pub status: String,
    pub created_at: String,
}

pub struct SandboxManager {
    docker: Docker,
    config: SandboxConfig,
    active: Arc<RwLock<HashMap<String, SandboxInstance>>>,
}

fn container_name(task_id: &str) -> String {
    format!("agentos-sandbox-{}", task_id)
}

impl SandboxManager {
    pub fn new(config: SandboxConfig) -> Self {
        let docker = Docker::connect_with_local(
            &config.docker_socket,
            120,
            API_DEFAULT_VERSION,
        )
        .expect("Failed to connect to Docker daemon");
        Self {
            docker,
            config,
            active: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_sandbox(&self, task_id: &str, work_dir: &str) -> anyhow::Result<SandboxInstance> {
        let image = &self.config.default_image;
        let create_opts = CreateImageOptions::<&str> {
            from_image: image.as_str(),
            tag: "latest",
            ..Default::default()
        };
        let _ = self.docker.create_image(Some(create_opts), None, None).try_collect::<Vec<_>>().await;

        let name = container_name(task_id);
        let config = Config {
            image: Some(image.clone()),
            cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
            working_dir: Some(work_dir.to_string()),
            host_config: Some(HostConfig {
                binds: Some(vec![format!("{}:{}", work_dir, work_dir)]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let options = CreateContainerOptions {
            name: name.clone(),
            platform: None,
        };
        let response = self.docker.create_container(Some(options), config).await?;
        self.docker.start_container::<String>(&response.id, None).await?;

        let now = chrono::Utc::now().to_rfc3339();
        let instance = SandboxInstance {
            container_id: response.id.clone(),
            task_id: task_id.to_string(),
            status: "running".to_string(),
            created_at: now,
        };
        self.active.write().await.insert(task_id.to_string(), instance.clone());
        Ok(instance)
    }

    pub async fn destroy_sandbox(&self, task_id: &str) -> anyhow::Result<()> {
        let name = container_name(task_id);
        let containers = self.docker.list_containers(Some(ListContainersOptions {
            all: true,
            filters: HashMap::from([("name".to_string(), vec![name.clone()])]),
            ..Default::default()
        })).await?;
        if let Some(container) = containers.first() {
            if let Some(ref id) = container.id {
                self.docker.stop_container(id, Some(StopContainerOptions { t: 10 })).await?;
                self.docker.remove_container(id, Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                })).await?;
            }
        }
        self.active.write().await.remove(task_id);
        Ok(())
    }

    pub async fn exec_in_sandbox(&self, task_id: &str, command: Vec<String>) -> anyhow::Result<String> {
        let containers = self.active.read().await;
        let instance = containers.get(task_id).ok_or_else(|| anyhow::anyhow!("Sandbox not found for task: {}", task_id))?;
        let container_id = &instance.container_id;

        let exec = self.docker.create_exec(container_id, CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(command),
            ..Default::default()
        }).await?;

        let result = self.docker.start_exec(&exec.id, None).await?;
        let mut output = Vec::new();
        if let StartExecResults::Attached { output: mut stream, .. } = result {
            while let Some(item) = stream.next().await {
                match item {
                    Ok(LogOutput::StdOut { message }) => {
                        output.push(String::from_utf8_lossy(&message).to_string());
                    }
                    Ok(LogOutput::StdErr { message }) => {
                        output.push(String::from_utf8_lossy(&message).to_string());
                    }
                    _ => {}
                }
            }
        }
        Ok(output.join(""))
    }

    pub async fn list_sandboxes(&self) -> anyhow::Result<Vec<SandboxInstance>> {
        let filters = HashMap::from([("name".to_string(), vec!["agentos-sandbox-".to_string()])]);
        let containers = self.docker.list_containers(Some(ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        })).await?;

        let mut instances = Vec::new();
        for container in containers {
            if let Some(ref id) = container.id {
                if let Some(ref names) = container.names {
                    for name in names {
                        if name.contains("agentos-sandbox-") {
                            let task_id = name.trim_start_matches('/').strip_prefix("agentos-sandbox-").unwrap_or("unknown").to_string();
                            let status = container.state.as_deref().unwrap_or("unknown").to_string();
                            let created_at = container.created.map(|c| c.to_string()).unwrap_or_default();
                            instances.push(SandboxInstance {
                                container_id: id.clone(),
                                task_id,
                                status,
                                created_at,
                            });
                        }
                    }
                }
            }
        }
        Ok(instances)
    }
}