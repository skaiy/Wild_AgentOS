//! JSON-LD Context 定义
//!
//! 提供全局统一的 @context 定义，包含系统命名空间和字段映射

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use once_cell::sync::Lazy;
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

    fn build_unified_context() -> Arc<Value> {
        let raw: Value = serde_json::from_str(include_str!("context.json"))
            .expect("context.json 解析失败");

        let mut context_map = raw
            .get("@context")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        for (prefix, uri) in [
            (NS_AGENT, URI_AGENT),
            (NS_TASK, URI_TASK),
            (NS_SKILL, URI_SKILL),
            (NS_MEM, URI_MEM),
            (NS_SEC, URI_SEC),
            (NS_MON, URI_MON),
            (NS_TMPL, URI_TMPL),
            (NS_EXP, URI_EXP),
            (NS_ADV, URI_ADV),
            (NS_NODE, URI_NODE),
        ] {
            context_map.insert(prefix.to_string(), Value::String(uri.to_string()));
        }

        Arc::new(Value::Object(context_map))
    }

    pub fn context_value() -> Arc<Value> {
        UNIFIED_CONTEXT
            .get_or_init(Self::build_unified_context)
            .clone()
    }

    pub fn inject(value: &mut Value) {
        if let Some(obj) = value.as_object_mut() {
            if !obj.contains_key("@context") {
                obj.insert("@context".to_string(), (*Self::context_value()).clone());
            }
        }
    }
}

pub static GLOBAL_CONTEXT: Lazy<HashMap<String, serde_json::Value>> = Lazy::new(|| {
    let mut context = HashMap::new();

    context.insert(NS_AGENT.to_string(), serde_json::Value::String(URI_AGENT.to_string()));
    context.insert(NS_TASK.to_string(), serde_json::Value::String(URI_TASK.to_string()));
    context.insert(NS_SKILL.to_string(), serde_json::Value::String(URI_SKILL.to_string()));
    context.insert(NS_MEM.to_string(), serde_json::Value::String(URI_MEM.to_string()));
    context.insert(NS_SEC.to_string(), serde_json::Value::String(URI_SEC.to_string()));
    context.insert(NS_MON.to_string(), serde_json::Value::String(URI_MON.to_string()));
    context.insert(NS_TMPL.to_string(), serde_json::Value::String(URI_TMPL.to_string()));
    context.insert(NS_EXP.to_string(), serde_json::Value::String(URI_EXP.to_string()));
    context.insert(NS_ADV.to_string(), serde_json::Value::String(URI_ADV.to_string()));
    context.insert(NS_NODE.to_string(), serde_json::Value::String(URI_NODE.to_string()));

    let mut task_mappings = HashMap::new();
    task_mappings.insert("@id".to_string(), "task:id".to_string());
    task_mappings.insert("@type".to_string(), "task:type".to_string());
    task_mappings.insert("summary".to_string(), "task:summary".to_string());
    task_mappings.insert("status".to_string(), "task:status".to_string());
    task_mappings.insert("confidence".to_string(), "task:confidence".to_string());
    task_mappings.insert("created_at".to_string(), "task:createdAt".to_string());
    task_mappings.insert("updated_at".to_string(), "task:updatedAt".to_string());
    task_mappings.insert("priority".to_string(), "task:priority".to_string());
    task_mappings.insert("assignee".to_string(), "task:assignee".to_string());
    task_mappings.insert("what".to_string(), "task:what".to_string());
    task_mappings.insert("why".to_string(), "task:why".to_string());
    task_mappings.insert("who".to_string(), "task:who".to_string());
    task_mappings.insert("when".to_string(), "task:when".to_string());
    task_mappings.insert("where".to_string(), "task:where".to_string());
    task_mappings.insert("how".to_string(), "task:how".to_string());
    task_mappings.insert("how_much".to_string(), "task:howMuch".to_string());
    task_mappings.insert("success_criteria".to_string(), "task:successCriteria".to_string());
    task_mappings.insert("description".to_string(), "task:description".to_string());
    task_mappings.insert("deadline".to_string(), "task:deadline".to_string());
    task_mappings.insert("data_sources".to_string(), "task:dataSources".to_string());
    task_mappings.insert("execution_environment".to_string(), "task:executionEnvironment".to_string());
    task_mappings.insert("plan_iri".to_string(), "task:planIRI".to_string());
    task_mappings.insert("preferred_skills".to_string(), "task:preferredSkills".to_string());
    task_mappings.insert("forbidden_tools".to_string(), "task:forbiddenTools".to_string());
    task_mappings.insert("required_steps".to_string(), "task:requiredSteps".to_string());
    task_mappings.insert("token_budget".to_string(), "task:tokenBudget".to_string());
    task_mappings.insert("max_pdca_cycles".to_string(), "task:maxPDCACycles".to_string());
    task_mappings.insert("actual_cost".to_string(), "task:actualCost".to_string());
    task_mappings.insert("requestor".to_string(), "task:requestor".to_string());
    task_mappings.insert("required_role".to_string(), "task:requiredRole".to_string());
    task_mappings.insert("access_level".to_string(), "task:accessLevel".to_string());

    for (key, value) in task_mappings {
        context.insert(key, serde_json::Value::String(value));
    }

    context
});

pub fn map_field_to_iri(field: &str) -> String {
    let field_mappings: HashMap<&str, &str> = [
        ("id", "task:id"),
        ("type", "task:type"),
        ("summary", "task:summary"),
        ("status", "task:status"),
        ("confidence", "task:confidence"),
        ("created_at", "task:createdAt"),
        ("updated_at", "task:updatedAt"),
        ("priority", "task:priority"),
        ("assignee", "task:assignee"),
        ("what", "task:what"),
        ("why", "task:why"),
        ("who", "task:who"),
        ("when", "task:when"),
        ("where", "task:where"),
        ("how", "task:how"),
        ("how_much", "task:howMuch"),
        ("success_criteria", "task:successCriteria"),
        ("description", "task:description"),
        ("deadline", "task:deadline"),
        ("data_sources", "task:dataSources"),
        ("execution_environment", "task:executionEnvironment"),
        ("plan_iri", "task:planIRI"),
        ("preferred_skills", "task:preferredSkills"),
        ("forbidden_tools", "task:forbiddenTools"),
        ("required_steps", "task:requiredSteps"),
        ("token_budget", "task:tokenBudget"),
        ("max_pdca_cycles", "task:maxPDCACycles"),
        ("actual_cost", "task:actualCost"),
        ("requestor", "task:requestor"),
        ("required_role", "task:requiredRole"),
        ("access_level", "task:accessLevel"),
        ("agent_id", "agent:id"),
        ("agent_role", "agent:role"),
        ("agent_status", "agent:status"),
        ("skill_name", "skill:name"),
        ("skill_version", "skill:version"),
        ("memory_key", "mem:key"),
        ("memory_value", "mem:value"),
    ].iter().cloned().collect();

    field_mappings
        .get(field)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("node:{}", field))
}

pub fn create_context_for_skill(skill_name: &str, skill_version: &str) -> HashMap<String, serde_json::Value> {
    let mut context = GLOBAL_CONTEXT.clone();

    context.insert(
        "skill_name".to_string(),
        serde_json::Value::String(format!("skill:{}", skill_name)),
    );
    context.insert(
        "skill_version".to_string(),
        serde_json::Value::String(format!("skill:{}", skill_version)),
    );

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
    fn test_global_context() {
        let ctx = &*GLOBAL_CONTEXT;
        assert!(ctx.contains_key("agent"));
        assert!(ctx.contains_key("task"));
        assert!(ctx.contains_key("skill"));
        assert_eq!(ctx.get("agent"), Some(&serde_json::Value::String(URI_AGENT.to_string())));
    }

    #[test]
    fn test_map_field_to_iri() {
        assert_eq!(map_field_to_iri("summary"), "task:summary");
        assert_eq!(map_field_to_iri("status"), "task:status");
        assert_eq!(map_field_to_iri("unknown_field"), "node:unknown_field");
    }

    #[test]
    fn test_create_context_for_skill() {
        let ctx = create_context_for_skill("file_read", "1.0.0");
        assert!(ctx.contains_key("skill_name"));
        assert!(ctx.contains_key("skill_version"));
    }
}
