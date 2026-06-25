use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::gateway::unified_gateway::{ChatMessage, UnifiedGateway};
use crate::skill_graph::graph_store::SkillGraphStore;
use crate::skill_graph::types::*;
use crate::tools::skill_registry::{SkillMeta, SkillRegistry};
use crate::CoreError;

const SKILL_CREATOR_SYSTEM_PROMPT: &str = r#"你是一个 Skill 定义生成器。根据用户的自然语言描述，生成符合以下 JSON-LD 规范的 Skill 定义。

## 输出格式

你必须输出一个合法的 JSON 对象，包含以下字段：

```json
{
  "name": "skill_name (小写，下划线分隔)",
  "description": "一句话描述这个 Skill 的功能",
  "category": "分类: file|network|ai|execution|validation|data|meta|system",
  "security_level": "安全等级: low|normal|high|critical",
  "allowed_roles": ["PA","DA","CA","AA"],
  "input_schema": {
    "type": "object",
    "properties": { ... },
    "required": [...]
  },
  "output_schema": {
    "type": "object",
    "properties": { ... }
  },
  "steps": [
    { "step_id": "step_1", "order": 1, "action": "步骤描述" }
  ],
  "tags": ["tag1", "tag2"],
  "what": "这个 Skill 做什么",
  "why": "为什么需要这个 Skill",
  "approach": "实现方法概述"
}
```

## 规则

1. name 必须是小写字母+下划线，如 `web_search`、`code_review`
2. input_schema 遵循 JSON Schema 规范，required 字段必须列出
3. 每个 input property 必须有 type 和 description
4. output_schema 必须包含执行结果的关键字段
5. steps 至少包含 1 个步骤，描述执行流程
6. security_level 规则：
   - low: 只读操作，无副作用
   - normal: 有网络请求或文件读取
   - high: 有文件写入或数据修改
   - critical: 有代码执行或系统操作
7. allowed_roles 根据安全等级设定：
   - low: ["PA","DA","CA","AA"]
   - normal: ["PA","DA","CA","AA"]
   - high: ["DA","CA"]
   - critical: ["DA"]
8. tags 至少包含 2 个标签
9. what/why/approach 必须简洁明确

只输出 JSON，不要输出其他内容。"#;

const MARKDOWN_CONVERTER_SYSTEM_PROMPT: &str = r#"你是一个 Markdown Skill 转换器。将用户提供的 Markdown 格式的 Skill 描述转换为符合本系统规范的 JSON-LD Skill 定义。

## 输入格式

Markdown Skill 通常包含：
- 标题（# Skill Name）
- 描述段落
- 参数列表（表格或列表）
- 工具/依赖说明
- 步骤说明

## 输出格式

与 Skill Creator 相同的 JSON 格式：

```json
{
  "name": "skill_name",
  "description": "描述",
  "category": "分类",
  "security_level": "安全等级",
  "allowed_roles": [...],
  "input_schema": { ... },
  "output_schema": { ... },
  "steps": [ ... ],
  "tags": [...],
  "what": "...",
  "why": "...",
  "approach": "..."
}
```

## 转换规则

1. 从 Markdown 标题提取 skill name
2. 从描述段落提取 what/why
3. 从参数列表构建 input_schema
4. 从工具列表确定 security_level 和 category
5. 从步骤说明构建 steps
6. 如果 Markdown 中提到了工具（如 read_file、write_file、bash 等），在 tags 中标注
7. 确保生成的 JSON 完整且合法

只输出 JSON，不要输出其他内容。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSkillRequest {
    pub description: String,
    pub skill_name_hint: Option<String>,
    pub category_hint: Option<String>,
    pub security_level_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertMarkdownRequest {
    pub markdown_content: String,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedSkill {
    pub skill_iri: String,
    pub name: String,
    pub graph_node: SkillGraphNode,
    pub registry_meta: SkillMeta,
    pub json_ld: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCreatorConfig {
    pub output_dir: PathBuf,
    pub auto_register: bool,
    pub validate_before_register: bool,
    pub default_security_level: String,
}

impl Default for SkillCreatorConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./skills"),
            auto_register: true,
            validate_before_register: true,
            default_security_level: "normal".to_string(),
        }
    }
}

pub struct SkillCreator {
    gateway: Arc<UnifiedGateway>,
    graph_store: Arc<SkillGraphStore>,
    skill_registry: Arc<SkillRegistry>,
    config: SkillCreatorConfig,
}

impl SkillCreator {
    pub fn new(
        gateway: Arc<UnifiedGateway>,
        graph_store: Arc<SkillGraphStore>,
        skill_registry: Arc<SkillRegistry>,
        config: SkillCreatorConfig,
    ) -> Self {
        Self {
            gateway,
            graph_store,
            skill_registry,
            config,
        }
    }

    pub async fn create_from_description(
        &self,
        request: CreateSkillRequest,
    ) -> Result<CreatedSkill, CoreError> {
        info!(description = %request.description, "开始从自然语言描述创建 Skill");

        let user_message = if let Some(hint) = &request.skill_name_hint {
            format!(
                "请创建一个 Skill：{}\n\n建议名称：{}\n分类建议：{}",
                request.description,
                hint,
                request.category_hint.as_deref().unwrap_or("自动判断")
            )
        } else {
            format!("请创建一个 Skill：{}", request.description)
        };

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: SKILL_CREATOR_SYSTEM_PROMPT.to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_message,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        ];

        let response = self.gateway.chat(messages).await.map_err(|e| {
            CoreError::Internal { message: format!("LLM 调用失败: {}", e) }
        })?;

        let content = response.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| CoreError::Internal { message: "LLM 返回空内容".to_string() })?;

        let skill_def = self.parse_llm_response(&content)?;
        let created = self.build_and_register(skill_def, request.security_level_override.as_deref())?;

        info!(skill_iri = %created.skill_iri, "Skill 创建完成");
        Ok(created)
    }

    pub async fn convert_from_markdown(
        &self,
        request: ConvertMarkdownRequest,
    ) -> Result<CreatedSkill, CoreError> {
        info!(source = ?request.source_path, "开始从 Markdown 转换 Skill");

        let user_message = format!(
            "请将以下 Markdown 格式的 Skill 描述转换为 JSON-LD Skill 定义：\n\n```markdown\n{}\n```",
            request.markdown_content
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: MARKDOWN_CONVERTER_SYSTEM_PROMPT.to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_message,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        ];

        let response = self.gateway.chat(messages).await.map_err(|e| {
            CoreError::Internal { message: format!("LLM 调用失败: {}", e) }
        })?;

        let content = response.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| CoreError::Internal { message: "LLM 返回空内容".to_string() })?;

        let skill_def = self.parse_llm_response(&content)?;
        let created = self.build_and_register(skill_def, None)?;

        info!(skill_iri = %created.skill_iri, "Markdown Skill 转换完成");
        Ok(created)
    }

    pub fn convert_markdown_static(content: &str) -> Result<SkillDefinition, CoreError> {
        let mut def = SkillDefinition::default();
        let mut in_code_block = false;
        let mut current_section = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }

            if in_code_block {
                continue;
            }

            if trimmed.starts_with("# ") {
                def.name = trimmed.trim_start_matches("# ").trim().to_lowercase()
                    .replace(' ', "_")
                    .replace(|c: char| !c.is_alphanumeric() && c != '_', "");
                def.description = format!("{} Skill", trimmed.trim_start_matches("# ").trim());
            } else if trimmed.starts_with("## ") {
                current_section = trimmed.trim_start_matches("## ").trim().to_lowercase();
            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                let item = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim();
                if current_section.contains("参数") || current_section.contains("parameter") {
                    if let Some((name, desc)) = item.split_once(':').or_else(|| item.split_once(' ')) {
                        def.input_properties.push(InputProperty {
                            name: name.trim().to_string(),
                            prop_type: "string".to_string(),
                            description: desc.trim().to_string(),
                            required: !desc.contains("可选") && !desc.contains("optional"),
                        });
                    }
                } else if current_section.contains("步骤") || current_section.contains("step") {
                    let order = (def.steps.len() + 1) as u32;
                    def.steps.push(SkillStep::new(
                        &format!("step_{}", order), order, item,
                    ));
                } else if current_section.contains("标签") || current_section.contains("tag") {
                    def.tags.push(item.to_string());
                }
            } else if !trimmed.is_empty() && def.what.is_empty() {
                def.what = trimmed.to_string();
            }
        }

        if def.name.is_empty() {
            def.name = "unnamed_skill".to_string();
        }
        if def.what.is_empty() {
            def.what = def.description.clone();
        }
        if def.why.is_empty() {
            def.why = format!("自动从 Markdown 转换: {}", def.description);
        }
        if def.approach.is_empty() {
            def.approach = "按照步骤执行".to_string();
        }
        if def.tags.is_empty() {
            def.tags.push("converted".to_string());
            def.tags.push("markdown".to_string());
        }

        Ok(def)
    }

    pub fn create_from_definition(
        &self,
        def: SkillDefinition,
        security_level_override: Option<&str>,
    ) -> Result<CreatedSkill, CoreError> {
        self.build_and_register(def, security_level_override)
    }

    fn parse_llm_response(&self, content: &str) -> Result<SkillDefinition, CoreError> {
        let json_str = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| CoreError::ValidationFailed {
                message: format!("LLM 返回的 JSON 解析失败: {}", e),
            })?;

        let name = parsed["name"].as_str().unwrap_or("unnamed_skill")
            .to_lowercase()
            .replace(' ', "_")
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "");

        let description = parsed["description"].as_str().unwrap_or("").to_string();
        let category = parsed["category"].as_str().unwrap_or("system").to_string();
        let security_level = parsed["security_level"].as_str().unwrap_or("normal").to_string();

        let allowed_roles = parsed["allowed_roles"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["DA".to_string()]);

        let input_schema = parsed.get("input_schema").cloned()
            .unwrap_or(json!({"type":"object","properties":{},"required":[]}));

        let output_schema = parsed.get("output_schema").cloned()
            .unwrap_or(json!({"type":"object","properties":{}}));

        let steps = parsed["steps"].as_array()
            .map(|arr| {
                arr.iter().enumerate().map(|(i, s)| {
                    SkillStep::new(
                        s["step_id"].as_str().unwrap_or(&format!("step_{}", i + 1)),
                        (i + 1) as u32,
                        s["action"].as_str().unwrap_or(""),
                    )
                }).collect()
            })
            .unwrap_or_default();

        let tags = parsed["tags"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["auto-generated".to_string()]);

        let what = parsed["what"].as_str().unwrap_or(&description).to_string();
        let why = parsed["why"].as_str().unwrap_or("").to_string();
        let approach = parsed["approach"].as_str().unwrap_or("").to_string();

        Ok(SkillDefinition {
            name,
            description,
            category,
            security_level,
            allowed_roles,
            input_schema,
            output_schema,
            steps,
            tags,
            what,
            why,
            approach,
            input_properties: vec![],
        })
    }

    fn build_and_register(
        &self,
        def: SkillDefinition,
        security_level_override: Option<&str>,
    ) -> Result<CreatedSkill, CoreError> {
        let security_level = security_level_override
            .unwrap_or(&def.security_level)
            .to_string();

        let skill_iri = format!("iri://skills/{}", def.name);

        let mut w2h = Skill5W2H::new(&def.what, &def.why);
        w2h.how.approach = def.approach.clone();
        if let Some(primary_role) = def.allowed_roles.first() {
            w2h.who.role_name = primary_role.clone();
            w2h.who.required_agent_role = Some(primary_role.clone());
        }
        w2h.when.applicable_phases = match def.category.as_str() {
            "validation" => vec!["Check".to_string()],
            "meta" | "system" => vec!["Plan".to_string(), "Do".to_string()],
            _ => vec!["Do".to_string()],
        };

        let mut graph_node = SkillGraphNode::new(&skill_iri, &def.name, &def.description)
            .with_node_type(SkillNodeType::Atomic)
            .with_5w2h(w2h);

        for tag in &def.tags {
            graph_node = graph_node.with_tag(tag);
        }

        if !def.steps.is_empty() {
            let mut content = SkillContent {
                summary: def.description.clone(),
                steps: def.steps.clone(),
                validation: None,
            };
            if !def.steps.is_empty() {
                content.validation = Some(SkillValidation {
                    method: "output_schema_check".to_string(),
                    success_condition: "输出符合 output_schema 定义".to_string(),
                });
            }
            graph_node = graph_node.with_content(content);
        }

        let json_ld = graph_node.to_json_ld();

        let registry_meta = SkillMeta {
            skill_iri: skill_iri.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
            version: "1.0.0".to_string(),
            category: def.category.clone(),
            security_level: security_level.clone(),
            allowed_roles: def.allowed_roles.clone(),
            input_schema: def.input_schema.clone(),
            output_schema: def.output_schema.clone(),
            compiled_template: Self::build_compiled_template(&def.input_schema),
            signature: None,
            signature_algorithm: None,
            input_mapping: Self::build_input_mapping(&def.name, &def.input_schema).into_iter().collect(),
            output_mapping: Self::build_output_mapping(&def.name, &def.output_schema).into_iter().collect(),
            skill_types: vec!["executor".to_string()],
        };

        if self.config.auto_register {
            self.graph_store.register_skill(graph_node.clone())?;
            self.skill_registry.register_skill(registry_meta.clone());
            debug!(skill_iri = %skill_iri, "Skill 已注册到 GraphStore 和 SkillRegistry");
        }

        if self.config.output_dir.exists() || std::fs::create_dir_all(&self.config.output_dir).is_ok() {
            let skill_dir = self.config.output_dir.join(&def.name);
            if std::fs::create_dir_all(&skill_dir).is_ok() {
                let jsonld_path = skill_dir.join("skill.jsonld");
                let jsonld_str = serde_json::to_string_pretty(&json_ld).unwrap_or_default();
                if let Err(e) = std::fs::write(&jsonld_path, &jsonld_str) {
                    warn!(path = %jsonld_path.display(), error = %e, "写入 skill.jsonld 失败");
                } else {
                    debug!(path = %jsonld_path.display(), "skill.jsonld 已写入");
                }
            }
        }

        Ok(CreatedSkill {
            skill_iri,
            name: def.name.clone(),
            graph_node,
            registry_meta,
            json_ld,
        })
    }

    fn build_compiled_template(input_schema: &serde_json::Value) -> String {
        let props = input_schema.get("properties")
            .and_then(|p| p.as_object());

        let Some(props) = props else {
            return "{}".to_string();
        };

        let mut template = serde_json::Map::new();
        for (key, value) in props {
            let default = value.get("default");
            if let Some(d) = default {
                template.insert(key.clone(), d.clone());
            } else {
                let prop_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("string");
                match prop_type {
                    "string" => { template.insert(key.clone(), json!("___")); }
                    "number" | "integer" => { template.insert(key.clone(), json!(0)); }
                    "boolean" => { template.insert(key.clone(), json!(false)); }
                    "array" => { template.insert(key.clone(), json!([])); }
                    "object" => { template.insert(key.clone(), json!({})); }
                    _ => { template.insert(key.clone(), json!("___")); }
                }
            }
        }

        serde_json::Value::Object(template).to_string()
    }

    fn build_input_mapping(skill_name: &str, input_schema: &serde_json::Value) -> Vec<(String, String)> {
        let props = input_schema.get("properties")
            .and_then(|p| p.as_object());

        let Some(props) = props else {
            return Vec::new();
        };

        props.keys().map(|k| {
            (k.clone(), format!("iri://schema/{}/{}", skill_name, k))
        }).collect()
    }

    fn build_output_mapping(skill_name: &str, output_schema: &serde_json::Value) -> Vec<(String, String)> {
        let props = output_schema.get("properties")
            .and_then(|p| p.as_object());

        let Some(props) = props else {
            return Vec::new();
        };

        props.keys().map(|k| {
            (k.clone(), format!("iri://schema/{}/output/{}", skill_name, k))
        }).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(default = "default_security_level")]
    pub security_level: String,
    #[serde(default = "default_allowed_roles")]
    pub allowed_roles: Vec<String>,
    #[serde(default)]
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub output_schema: serde_json::Value,
    #[serde(default)]
    pub steps: Vec<SkillStep>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub approach: String,
    #[serde(default)]
    pub input_properties: Vec<InputProperty>,
}

fn default_security_level() -> String { "normal".to_string() }
fn default_allowed_roles() -> Vec<String> { vec!["DA".to_string()] }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputProperty {
    pub name: String,
    pub prop_type: String,
    pub description: String,
    pub required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_markdown_static() {
        let markdown = r#"# Web Search

搜索互联网获取信息

## 参数

- query: 搜索关键词
- max_results: 最大结果数（可选）

## 步骤

- 构建搜索 URL
- 发送 HTTP 请求
- 解析返回结果
- 格式化输出

## 标签

- search
- web"#;

        let def = SkillCreator::convert_markdown_static(markdown).unwrap();

        assert_eq!(def.name, "web_search");
        assert!(def.description.contains("Web Search"));
        assert_eq!(def.input_properties.len(), 2);
        assert!(def.input_properties[0].required);
        assert!(!def.input_properties[1].required);
        assert_eq!(def.steps.len(), 4);
        assert!(def.tags.contains(&"search".to_string()));
    }

    #[test]
    fn test_build_compiled_template() {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 10}
            },
            "required": ["query"]
        });

        let template = SkillCreator::build_compiled_template(&input_schema);
        let parsed: serde_json::Value = serde_json::from_str(&template).unwrap();

        assert_eq!(parsed["query"], "___");
        assert_eq!(parsed["limit"], 10);
    }

    #[test]
    fn test_parse_llm_response() {
        let config = SkillCreatorConfig::default();
        let gateway = Arc::new(UnifiedGateway::new(&crate::config::GatewaySettings {
            base_url: "http://localhost:3000".to_string(),
            api_key: "test".to_string(),
            default_model: "test".to_string(),
            timeout_seconds: 30,
            max_retries: 1,
            model_mapping: Default::default(),
        }).unwrap());
        let graph_store = Arc::new(SkillGraphStore::new());
        let registry = Arc::new(SkillRegistry::new());
        let creator = SkillCreator::new(gateway, graph_store, registry, config);

        let llm_response = r#"```json
{
  "name": "code_review",
  "description": "审查代码质量并提供改进建议",
  "category": "validation",
  "security_level": "low",
  "allowed_roles": ["CA"],
  "input_schema": {
    "type": "object",
    "properties": {
      "code": {"type": "string", "description": "待审查的代码"},
      "language": {"type": "string", "description": "编程语言"}
    },
    "required": ["code"]
  },
  "output_schema": {
    "type": "object",
    "properties": {
      "issues": {"type": "array"},
      "score": {"type": "number"}
    }
  },
  "steps": [
    {"step_id": "step_1", "order": 1, "action": "解析代码结构"},
    {"step_id": "step_2", "order": 2, "action": "检查代码规范"},
    {"step_id": "step_3", "order": 3, "action": "生成审查报告"}
  ],
  "tags": ["code", "review", "quality"],
  "what": "审查代码质量",
  "why": "确保代码符合规范和最佳实践",
  "approach": "静态分析+规则匹配"
}
```"#;

        let def = creator.parse_llm_response(llm_response).unwrap();

        assert_eq!(def.name, "code_review");
        assert_eq!(def.category, "validation");
        assert_eq!(def.security_level, "low");
        assert_eq!(def.allowed_roles, vec!["CA"]);
        assert_eq!(def.steps.len(), 3);
        assert_eq!(def.tags.len(), 3);
    }

    #[test]
    fn test_create_from_definition() {
        let config = SkillCreatorConfig::default();
        let gateway = Arc::new(UnifiedGateway::new(&crate::config::GatewaySettings {
            base_url: "http://localhost:3000".to_string(),
            api_key: "test".to_string(),
            default_model: "test".to_string(),
            timeout_seconds: 30,
            max_retries: 1,
            model_mapping: Default::default(),
        }).unwrap());
        let graph_store = Arc::new(SkillGraphStore::new());
        let registry = Arc::new(SkillRegistry::new());
        let creator = SkillCreator::new(gateway, graph_store, registry, config);

        let def = SkillDefinition {
            name: "test_skill".to_string(),
            description: "测试技能".to_string(),
            category: "system".to_string(),
            security_level: "low".to_string(),
            allowed_roles: vec!["DA".to_string()],
            input_schema: json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"]}),
            output_schema: json!({"type":"object","properties":{"result":{"type":"string"}}}),
            steps: vec![SkillStep::new("step_1", 1, "执行测试")],
            tags: vec!["test".to_string()],
            what: "测试".to_string(),
            why: "验证".to_string(),
            approach: "直接执行".to_string(),
            input_properties: vec![],
        };

        let created = creator.create_from_definition(def, None).unwrap();

        assert_eq!(created.skill_iri, "iri://skills/test_skill");
        assert_eq!(created.name, "test_skill");
        assert!(created.json_ld.get("@id").is_some());
    }
}
