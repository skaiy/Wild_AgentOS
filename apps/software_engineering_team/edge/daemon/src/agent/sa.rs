use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::da::DoAgent;
use super::runner::AgentRunner;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub llm_provider: String,
    pub llm_model: String,
    pub llm_api_key: String,
    pub llm_base_url: String,
    pub center_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    pub id: String,
    pub stage_type: String,
    pub input: serde_json::Value,
    pub prev_outputs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: String,
    pub output: serde_json::Value,
    pub summary: String,
}

pub struct SupervisorAgent {
    config: AgentConfig,
}

impl SupervisorAgent {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    pub async fn dispatch(&self, task: TaskDef) -> anyhow::Result<TaskResult> {
        let runner = AgentRunner::new(
            self.config.llm_api_key.clone(),
            self.config.llm_base_url.clone(),
            self.config.llm_model.clone(),
        );

        let da = DoAgent::new(runner);

        if self.is_simple_task(&task.stage_type) {
            let result = da.execute(task).await?;
            Ok(result)
        } else {
            let mut current_output = da.execute(task.clone()).await?;

            for i in 0..2 {
                let review_task = TaskDef {
                    id: format!("{}-review-{}", task.id, i),
                    stage_type: "review".to_string(),
                    input: current_output.output.clone(),
                    prev_outputs: {
                        let mut map = HashMap::new();
                        map.insert("previous_output".to_string(), current_output.output.clone());
                        map
                    },
                };

                let review_result = da.execute(review_task).await?;

                if review_result.status == "completed" {
                    let has_issues = review_result
                        .output
                        .get("issues")
                        .and_then(|i| i.as_array())
                        .map(|arr| arr.len() > 2)
                        .unwrap_or(false);

                    if !has_issues {
                        return Ok(TaskResult {
                            task_id: task.id.clone(),
                            status: "completed".to_string(),
                            output: current_output.output,
                            summary: current_output.summary,
                        });
                    }

                    let refine_task = TaskDef {
                        id: format!("{}-refine-{}", task.id, i),
                        stage_type: task.stage_type.clone(),
                        input: serde_json::json!({
                            "previous_output": current_output.output,
                            "review_feedback": review_result.output,
                            "original_input": task.input,
                        }),
                        prev_outputs: HashMap::new(),
                    };

                    current_output = da.execute(refine_task).await?;
                }
            }

            Ok(TaskResult {
                task_id: task.id.clone(),
                status: "completed".to_string(),
                output: current_output.output,
                summary: current_output.summary,
            })
        }
    }

    fn is_simple_task(&self, stage_type: &str) -> bool {
        matches!(stage_type, "requirement" | "generic" | "simple")
    }
}