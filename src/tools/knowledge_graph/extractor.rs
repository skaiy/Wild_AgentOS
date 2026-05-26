use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::ontology::OntologyManager;
use super::rdf_mapper::RdfMapper;
use super::store::KnowledgeGraphStore;
use super::types::{LLMExtractionOutput, RdfMappingResult};

pub struct KnowledgeExtractor {
    ontology: OntologyManager,
    store: KnowledgeGraphStore,
    api_url: String,
    api_key: String,
    model: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatRequestMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatRequestBody {
    model: String,
    messages: Vec<ChatRequestMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
}

impl KnowledgeExtractor {
    pub fn new(
        ontology: OntologyManager,
        store: KnowledgeGraphStore,
        api_url: String,
        api_key: String,
        model: String,
    ) -> Self {
        Self {
            ontology,
            store,
            api_url,
            api_key,
            model,
        }
    }

    pub fn build_extraction_prompt(text: &str, vocabulary: &str) -> String {
        format!(
            r#"你是一个知识图谱抽取专家。请从给定文本中提取实体和关系。

{vocabulary}

## 输出格式要求

请严格输出以下 JSON 格式，不要包含任何其他文字、解释或 markdown 代码块标记：

{{
  "nodes": [
    {{
      "id": "实体唯一标识（英文/拼音，无空格）",
      "node_type": "使用上面列出的实体类型 IRI",
      "label": "实体中文名称",
      "properties": {{}}
    }}
  ],
  "edges": [
    {{
      "source": "源实体 id",
      "target": "目标实体 id",
      "relation": "使用上面列出的关系 IRI",
      "properties": {{}}
    }}
  ]
}}

## 规则
1. nodes 和 edges 中的所有字段不能为空
2. 至少提取一个实体（node）
3. type 和 relation 字段必须使用上面列出的 IRI
4. id 使用英文或拼音，不要使用中文
5. properties 可以为空对象 {{}}

## 待抽取文本

{text}"#
        )
    }

    async fn call_llm(&self, prompt: &str) -> Result<String, String> {
        let client = Client::builder()
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        let url = format!(
            "{}/chat/completions",
            self.api_url.trim_end_matches('/').trim_end_matches("/v1")
        );

        let body = ChatRequestBody {
            model: self.model.clone(),
            messages: vec![ChatRequestMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: 0.1,
        };

        debug!(model = %self.model, url = %url, "调用 LLM API 进行知识抽取");

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM API 请求失败: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("LLM API 返回错误 ({}): {}", status, text));
        }

        let response_text = resp
            .text()
            .await
            .map_err(|e| format!("读取 LLM 响应失败: {}", e))?;

        let chat_resp: ChatCompletionResponse =
            serde_json::from_str(&response_text)
                .map_err(|e| format!("解析 LLM 响应 JSON 失败: {} (原始响应: {})", e, truncate_str(&response_text, 200)))?;

        let choice = chat_resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| "LLM 响应中无 choices".to_string())?;

        choice
            .message
            .content
            .ok_or_else(|| "LLM 响应中 content 为空".to_string())
    }

    pub fn validate_extraction(json_str: &str) -> Result<LLMExtractionOutput, String> {
        let cleaned = clean_json_response(json_str);

        let output: LLMExtractionOutput = serde_json::from_str(&cleaned).map_err(|e| {
            format!(
                "JSON 解析失败: {} (输入前 200 字符: {})",
                e,
                truncate_str(&cleaned, 200)
            )
        })?;

        if output.nodes.is_empty() {
            return Err("抽取结果中至少需要一个实体 (node)".to_string());
        }

        for (i, node) in output.nodes.iter().enumerate() {
            if node.id.trim().is_empty() {
                return Err(format!("第 {} 个实体的 id 为空", i + 1));
            }
            if node.node_type.trim().is_empty() {
                return Err(format!("第 {} 个实体的 type 为空", i + 1));
            }
            if node.label.trim().is_empty() {
                return Err(format!("第 {} 个实体的 label 为空", i + 1));
            }
        }

        for (i, edge) in output.edges.iter().enumerate() {
            if edge.source.trim().is_empty() {
                return Err(format!("第 {} 条关系的 source 为空", i + 1));
            }
            if edge.target.trim().is_empty() {
                return Err(format!("第 {} 条关系的 target 为空", i + 1));
            }
            if edge.relation.trim().is_empty() {
                return Err(format!("第 {} 条关系的 relation 为空", i + 1));
            }
        }

        Ok(output)
    }

    pub fn extract(&self, text: &str, domain: Option<&str>) -> Result<RdfMappingResult, String> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| "无法获取 tokio 运行时".to_string())
            .or_else(|_| {
                tokio::runtime::Runtime::new()
                    .map(|rt| rt.handle().clone())
                    .map_err(|e| format!("创建 tokio 运行时失败: {}", e))
            })
            .or_else(|e: String| {
                tokio::runtime::Runtime::new()
                    .map(|rt| rt.handle().clone())
                    .map_err(|_| e)
            })
            .unwrap_or_else(|_| {
                tokio::runtime::Runtime::new()
                    .expect("创建 tokio 运行时失败")
                    .handle()
                    .clone()
            });

        let vocab = self.ontology.get_vocabulary(domain);
        let vocabulary = self.ontology.format_vocabulary_for_prompt(&vocab);
        let base_prompt = Self::build_extraction_prompt(text, &vocabulary);

        let mut last_error = String::new();
        let mut current_prompt = base_prompt;

        for attempt in 1..=3 {
            debug!(attempt, "知识抽取尝试");

            let llm_result = rt.block_on(self.call_llm(&current_prompt));

            let raw_response = match llm_result {
                Ok(resp) => resp,
                Err(e) => {
                    warn!(attempt, error = %e, "LLM API 调用失败");
                    last_error = e;
                    if attempt < 3 {
                        current_prompt = format!(
                            "{}\n\n---\n上次调用失败，错误信息: {}\n请重试。",
                            Self::build_extraction_prompt(text, &vocabulary),
                            last_error
                        );
                    }
                    continue;
                }
            };

            match Self::validate_extraction(&raw_response) {
                Ok(extraction) => {
                    let graph = self.store.default_graph();
                    let result = RdfMapper::map_extraction(&extraction, graph);

                    self.store.write_quads(&result.quads, graph)?;

                    debug!(
                        entities = result.entity_count,
                        relations = result.relation_count,
                        quads = result.quads.len(),
                        "知识抽取完成并写入存储"
                    );

                    return Ok(result);
                }
                Err(e) => {
                    warn!(attempt, error = %e, "抽取结果校验失败");
                    last_error = e;
                    if attempt < 3 {
                        current_prompt = format!(
                            "{}\n\n---\n上次抽取结果校验失败，错误: {}\nLLM 原始输出:\n{}\n请修正后重新输出。",
                            Self::build_extraction_prompt(text, &vocabulary),
                            last_error,
                            truncate_str(&raw_response, 500)
                        );
                    }
                }
            }
        }

        Err(format!(
            "知识抽取在 3 次尝试后仍失败，最后错误: {}",
            last_error
        ))
    }

    pub fn ontology(&self) -> &OntologyManager {
        &self.ontology
    }

    pub fn store(&self) -> &KnowledgeGraphStore {
        &self.store
    }
}

fn clean_json_response(input: &str) -> String {
    let trimmed = input.trim();

    if trimmed.starts_with("```json") {
        let without_start = trimmed.trim_start_matches("```json").trim();
        if let Some(pos) = without_start.rfind("```") {
            return without_start[..pos].trim().to_string();
        }
        return without_start.trim().to_string();
    }

    if trimmed.starts_with("```") {
        let without_start = trimmed.trim_start_matches("```").trim();
        if let Some(pos) = without_start.rfind("```") {
            return without_start[..pos].trim().to_string();
        }
        return without_start.trim().to_string();
    }

    if let Some(start) = trimmed.find('{') {
        let mut depth = 0i32;
        for (i, c) in trimmed[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return trimmed[start..start + i + 1].to_string();
                    }
                }
                _ => {}
            }
        }
    }

    trimmed.to_string()
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...(截断)", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_clean_json_response_plain() {
        let input = r#"{"nodes": [], "edges": []}"#;
        assert_eq!(clean_json_response(input), input);
    }

    #[test]
    fn test_clean_json_response_markdown_block() {
        let input = "```json\n{\"nodes\": [], \"edges\": []}\n```";
        assert_eq!(clean_json_response(input), r#"{"nodes": [], "edges": []}"#);
    }

    #[test]
    fn test_clean_json_response_with_prefix() {
        let input = "Here is the result:\n{\"nodes\": [], \"edges\": []}\nDone.";
        assert_eq!(clean_json_response(input), r#"{"nodes": [], "edges": []}"#);
    }

    #[test]
    fn test_validate_extraction_empty_nodes() {
        let json = r#"{"nodes": [], "edges": []}"#;
        let result = KnowledgeExtractor::validate_extraction(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("至少需要一个实体"));
    }

    #[test]
    fn test_validate_extraction_empty_node_id() {
        let json = r#"{"nodes": [{"id": "", "node_type": "T", "label": "L", "properties": {}}], "edges": []}"#;
        let result = KnowledgeExtractor::validate_extraction(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("id 为空"));
    }

    #[test]
    fn test_validate_extraction_empty_edge_source() {
        let json = r#"{"nodes": [{"id": "a", "node_type": "T", "label": "L", "properties": {}}], "edges": [{"source": "", "target": "b", "relation": "R", "properties": {}}]}"#;
        let result = KnowledgeExtractor::validate_extraction(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("source 为空"));
    }

    #[test]
    fn test_validate_extraction_valid() {
        let json = r#"{"nodes": [{"id": "a", "node_type": "https://agentos.ontology/core/Person", "label": "Alice", "properties": {}}], "edges": [{"source": "a", "target": "b", "relation": "https://agentos.ontology/business/worksFor", "properties": {}}]}"#;
        let result = KnowledgeExtractor::validate_extraction(json);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.nodes.len(), 1);
        assert_eq!(output.edges.len(), 1);
    }

    #[test]
    fn test_validate_extraction_invalid_json() {
        let json = "not json at all";
        let result = KnowledgeExtractor::validate_extraction(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JSON 解析失败"));
    }

    #[test]
    fn test_build_extraction_prompt() {
        let vocab = "## 可用实体类型\n- IRI: https://agentos.ontology/core/Person | 名称: 人物 | 表示一个人";
        let prompt = KnowledgeExtractor::build_extraction_prompt("测试文本", vocab);
        assert!(prompt.contains("知识图谱抽取专家"));
        assert!(prompt.contains("https://agentos.ontology/core/Person"));
        assert!(prompt.contains("测试文本"));
        assert!(prompt.contains("nodes"));
        assert!(prompt.contains("edges"));
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("abcdefghij", 5);
        assert_eq!(result, "abcde...(截断)");
    }

    #[test]
    fn test_clean_json_response_nested() {
        let input = r#"prefix {"nodes": [{"id": "a"}], "edges": [{"source": "a"}]} suffix"#;
        let cleaned = clean_json_response(input);
        let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(&cleaned).unwrap();
        assert!(parsed.contains_key("nodes"));
    }
}
