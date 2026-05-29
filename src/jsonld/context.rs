//! JSON-LD Context 定义
//!
//! 提供全局统一的 @context 定义，包含系统命名空间和字段映射
//!
//! # 设计说明
//!
//! ## 单 @context 源
//!
//! context.json 是权威的 @context 定义来源，编译期嵌入二进制。
//! `context_value()` 提供懒加载的 `Arc<Value>` 访问。
//! 除 context.json 外不维护任何独立的上下文 HashMap。
//!
//! ## map_field_to_iri() = 轻量级 Expansion
//!
//! 这是一个有意简化的 JSON-LD key 展开——从 context_value() 中读取字段→IRI 映射，
//! O(1) 查表而非完整 JSON-LD 1.1 Expansion 算法的 O(n) 递归树遍历。
//!
//! ## 不加载远程 @context
//!
//! 封闭系统中 @context 编译期已知，不引入远程加载的延迟和失败模式。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use serde_json::Value;

pub const NS_AGENT: &str = "agent";
pub const NS_TASK: &str = "task";
pub const NS_SKILL: &str = "skill";
pub const NS_MEM: &str = "mem";
pub const NS_SEC: &str = "sec";
pub const NS_MON: &str = "mon";
pub const NS_TMPL: &str = "tmpl";
pub const NS_EXP: &str = "exp";
pub const NS_ADV: &str = "adv";
pub const NS_NODE: &str = "node";

pub const URI_AGENT: &str = "https://pdca-agent.org/ontology/agent#";
pub const URI_TASK: &str = "https://pdca-agent.org/ontology/task#";
pub const URI_SKILL: &str = "https://pdca-agent.org/ontology/skill#";
pub const URI_MEM: &str = "https://pdca-agent.org/ontology/memory#";
pub const URI_SEC: &str = "https://pdca-agent.org/ontology/security#";
pub const URI_MON: &str = "https://pdca-agent.org/ontology/monitoring#";
pub const URI_TMPL: &str = "https://pdca-agent.org/ontology/template#";
pub const URI_EXP: &str = "https://pdca-agent.org/ontology/experience#";
pub const URI_ADV: &str = "https://pdca-agent.org/ontology/advisory#";
pub const URI_NODE: &str = "https://pdca-agent.org/ontology/node#";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonLdContext {
    pub prefix: String,
    pub uri: String,
    pub mappings: HashMap<String, String>,
}

static UNIFIED_CONTEXT: OnceLock<Arc<Value>> = OnceLock::new();

impl JsonLdContext {
    pub fn new(prefix: String, uri: String) -> Self {
        Self {
            prefix,
            uri,
            mappings: HashMap::new(),
        }
    }

    pub fn with_mappings(prefix: String, uri: String, mappings: HashMap<String, String>) -> Self {
        Self {
            prefix,
            uri,
            mappings,
        }
    }

    pub fn to_dict(&self) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();
        result.insert(
            self.prefix.clone(),
            serde_json::Value::String(self.uri.clone()),
        );

        for (key, value) in &self.mappings {
            result.insert(
                key.clone(),
                serde_json::Value::String(value.clone()),
            );
        }

        result
    }

    pub fn add_mapping(&mut self, field: String, iri: String) {
        self.mappings.insert(field, iri);
    }

    /// 构建统一的 @context 对象
    ///
    /// 从 context.json 加载基础定义（类型映射 + 字段映射 + 领域命名空间），
    /// 再追加编程方式注入的命名空间前缀作为安全兜底。
    ///
    /// 结果会被 OnceLock 缓存，此后 O(1) 访问。
    fn build_unified_context() -> Arc<Value> {
        let raw: Value = serde_json::from_str(include_str!("context.json"))
            .expect("context.json 解析失败");

        let mut context_map = raw
            .get("@context")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        // context.json 已包含这些命名空间定义（作为基础字段），
        // 此处通过入名常量再注入一次确保不会被误删的 context.json 字段破坏。
        // 值相同（由 const URI_* 常量保证），重复插入不产生副作用。
        for (prefix, uri) in [
            ("agent", URI_AGENT),
            ("task", URI_TASK),
            ("skill", URI_SKILL),
            ("mem", URI_MEM),
            ("sec", URI_SEC),
            ("mon", URI_MON),
            ("tmpl", URI_TMPL),
            ("exp", URI_EXP),
            ("adv", URI_ADV),
            ("node", URI_NODE),
        ] {
            context_map.entry(prefix.to_string()).or_insert(Value::String(uri.to_string()));
        }

        Arc::new(Value::Object(context_map))
    }

    pub fn context_value() -> Arc<Value> {
        UNIFIED_CONTEXT
            .get_or_init(Self::build_unified_context)
            .clone()
    }

    /// 向 JSON 值注入 @context（如果没有的话）
    pub fn inject(value: &mut Value) {
        if let Some(obj) = value.as_object_mut() {
            if !obj.contains_key("@context") {
                obj.insert("@context".to_string(), (*Self::context_value()).clone());
            }
        }
    }
}

/// 将字段名映射为完整 IRI
///
/// # 有意设计：轻量级 Expansion
///
/// 这不是 JSON-LD 1.1 标准的 Expansion 算法（递归树遍历 + 值展开），
/// 而是从 context.json 读取字段→IRI 映射的 flat lookup。
/// 对于封闭系统（@context 编译期已知）这提供了等价的语义，
/// 但成本从 O(n) 递归变为 O(1) 查表。
///
/// 不在 context.json 中定义的字段回退到 `node:{field}`。
pub fn map_field_to_iri(field: &str) -> String {
    if field == "@id" || field == "@type" {
        return field.to_string();
    }

    let ctx = JsonLdContext::context_value();
    ctx.get(field)
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Object(obj) => obj.get("@id").and_then(|id| id.as_str().map(String::from)),
            _ => None,
        })
        .unwrap_or_else(|| format!("node:{}", field))
}

/// 创建 Skill 专用的 @context（追加 skill_name / skill_version）
pub fn create_context_for_skill(skill_name: &str, skill_version: &str) -> HashMap<String, serde_json::Value> {
    let ctx = JsonLdContext::context_value();
    let mut context: HashMap<String, Value> = ctx
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let name_iri = map_field_to_iri("skill_name");
    let version_iri = map_field_to_iri("skill_version");

    context.insert("skill_name".to_string(), Value::String(name_iri));
    context.insert("skill_version".to_string(), Value::String(version_iri));

    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonld_context_new() {
        let ctx = JsonLdContext::new("task".to_string(), URI_TASK.to_string());
        assert_eq!(ctx.prefix, "task");
        assert_eq!(ctx.uri, URI_TASK);
        assert!(ctx.mappings.is_empty());
    }

    #[test]
    fn test_jsonld_context_to_dict() {
        let mut mappings = HashMap::new();
        mappings.insert("summary".to_string(), "task:summary".to_string());
        mappings.insert("status".to_string(), "task:status".to_string());

        let ctx = JsonLdContext::with_mappings(
            "task".to_string(),
            URI_TASK.to_string(),
            mappings,
        );

        let dict = ctx.to_dict();
        assert_eq!(dict.get("task"), Some(&serde_json::Value::String(URI_TASK.to_string())));
        assert_eq!(dict.get("summary"), Some(&serde_json::Value::String("task:summary".to_string())));
    }

    #[test]
    fn test_context_value_contains_namespaces() {
        let ctx = JsonLdContext::context_value();
        assert!(ctx.get("agent").is_some());
        assert!(ctx.get("task").is_some());
        assert!(ctx.get("skill").is_some());
    }

    #[test]
    fn test_map_field_to_iri() {
        assert_eq!(map_field_to_iri("summary"), "pdca:summary");
        assert_eq!(map_field_to_iri("status"), "pdca:status");
        assert_eq!(map_field_to_iri("confidence"), "pdca:confidence");
        assert_eq!(map_field_to_iri("created_at"), "pdca:createdAt");
        assert_eq!(map_field_to_iri("what"), "pdca:what");
        assert_eq!(map_field_to_iri("plan_iri"), "pdca:planIRI");
        assert_eq!(map_field_to_iri("@id"), "@id");
        assert_eq!(map_field_to_iri("unknown_field"), "node:unknown_field");
    }

    #[test]
    fn test_create_context_for_skill() {
        let ctx = create_context_for_skill("file_read", "1.0.0");
        assert!(ctx.contains_key("skill_name"));
        assert!(ctx.contains_key("skill_version"));
        assert!(ctx.contains_key("summary"));
        assert!(ctx.contains_key("status"));
    }

    #[test]
    fn test_inject_context() {
        let mut node = serde_json::json!({
            "@id": "iri://task/123",
            "summary": "test"
        });
        JsonLdContext::inject(&mut node);
        assert!(node.get("@context").is_some());
        JsonLdContext::inject(&mut node);
        assert!(node.get("@context").is_some());
    }
}
