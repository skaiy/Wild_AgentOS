use super::runner::{AgentRunner, ChatMessage};
use crate::agent::sa::{TaskDef, TaskResult};

pub struct DoAgent {
    runner: AgentRunner,
}

impl DoAgent {
    pub fn new(runner: AgentRunner) -> Self {
        Self { runner }
    }

    pub async fn execute(&self, task: TaskDef) -> anyhow::Result<TaskResult> {
        let prompt = Self::build_prompt(&task.stage_type, &task.input);
        let system_message = ChatMessage {
            role: "system".to_string(),
            content: "You are a professional software engineer AI assistant. Follow instructions precisely and output structured results.".to_string(),
        };
        let user_message = ChatMessage {
            role: "user".to_string(),
            content: prompt,
        };
        let messages = vec![system_message, user_message];
        let response = self.runner.chat(messages).await?;
        let (output, summary) = Self::parse_response(&response)?;

        Ok(TaskResult {
            task_id: task.id.clone(),
            status: "completed".to_string(),
            output,
            summary,
        })
    }

    fn build_prompt(stage_type: &str, input: &serde_json::Value) -> String {
        let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
        match stage_type {
            "requirement" => {
                format!(
                    "You are analyzing requirements. Given the following input:\n{}\n\n\
                    Please perform requirement analysis:\n\
                    1. Identify core functional requirements\n\
                    2. Identify non-functional requirements\n\
                    3. List key stakeholders and their concerns\n\
                    4. Identify potential risks and constraints\n\
                    5. Create user stories for each feature\n\n\
                    Output your analysis in structured JSON format with keys: functional_requirements, non_functional_requirements, stakeholders, risks, user_stories.",
                    input_str
                )
            }
            "design" => {
                format!(
                    "You are designing system architecture. Given the following input:\n{}\n\n\
                    Please create a detailed design:\n\
                    1. System architecture overview\n\
                    2. Component breakdown and responsibilities\n\
                    3. Data models and relationships\n\
                    4. API interfaces and contracts\n\
                    5. Technology stack recommendations\n\
                    6. Deployment architecture\n\n\
                    Output your design in structured JSON format.",
                    input_str
                )
            }
            "coding" => {
                format!(
                    "You are implementing code. Given the following input:\n{}\n\n\
                    Please implement the solution:\n\
                    1. Write clean, idiomatic code\n\
                    2. Include proper error handling\n\
                    3. Follow best practices and patterns\n\
                    4. Ensure type safety\n\n\
                    Output the implementation in structured JSON format with keys: files (array of {{path, language, code}}), main_logic_description.",
                    input_str
                )
            }
            "testing" => {
                format!(
                    "You are writing tests. Given the following input:\n{}\n\n\
                    Please create comprehensive tests:\n\
                    1. Unit tests for core logic\n\
                    2. Integration tests for interfaces\n\
                    3. Edge cases and error scenarios\n\
                    4. Performance considerations\n\n\
                    Output the tests in structured JSON format with keys: test_files, test_summary, coverage_notes.",
                    input_str
                )
            }
            "review" => {
                format!(
                    "You are reviewing code. Given the following input:\n{}\n\n\
                    Please perform a thorough code review:\n\
                    1. Check for bugs and logical errors\n\
                    2. Evaluate code quality and style\n\
                    3. Check security vulnerabilities\n\
                    4. Assess performance implications\n\
                    5. Suggest improvements\n\n\
                    Output your review in structured JSON format with keys: issues (array of {{severity, file, line, description, suggestion}}), overall_assessment, score.",
                    input_str
                )
            }
            "cicd" => {
                format!(
                    "You are setting up CI/CD pipeline. Given the following input:\n{}\n\n\
                    Please design the CI/CD pipeline:\n\
                    1. Build and compilation steps\n\
                    2. Test automation strategy\n\
                    3. Code quality checks\n\
                    4. Deployment stages and environments\n\
                    5. Rollback strategy\n\n\
                    Output the pipeline configuration in structured JSON format.",
                    input_str
                )
            }
            "deploy" => {
                format!(
                    "You are deploying the application. Given the following input:\n{}\n\n\
                    Please create a deployment plan:\n\
                    1. Deployment prerequisites and environment setup\n\
                    2. Step-by-step deployment instructions\n\
                    3. Health check and verification steps\n\
                    4. Monitoring and alerting setup\n\
                    5. Rollback procedure\n\n\
                    Output the deployment plan in structured JSON format.",
                    input_str
                )
            }
            _ => {
                format!(
                    "You are performing a general task. Given the following input:\n{}\n\n\
                    Please process this task thoroughly and provide structured output.",
                    input_str
                )
            }
        }
    }

    fn parse_response(response: &str) -> anyhow::Result<(serde_json::Value, String)> {
        let trimmed = response.trim();

        if let Some(json_start) = trimmed.find('{') {
            if let Some(json_end) = trimmed.rfind('}') {
                let json_str = &trimmed[json_start..=json_end];
                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(val) => {
                        let summary = val
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or("task completed")
                            .to_string();
                        return Ok((val, summary));
                    }
                    Err(_) => {}
                }
            }
        }

        if let Some(json_start) = trimmed.find('[') {
            if let Some(json_end) = trimmed.rfind(']') {
                let json_str = &trimmed[json_start..=json_end];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let summary = "task completed with array output".to_string();
                    return Ok((val, summary));
                }
            }
        }

        let output = serde_json::Value::String(trimmed.to_string());
        let summary = if trimmed.len() > 100 {
            format!("{}...", &trimmed[..100])
        } else {
            trimmed.to_string()
        };

        Ok((output, summary))
    }
}