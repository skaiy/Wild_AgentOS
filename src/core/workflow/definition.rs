//! JSON-LD 工作流定义类型
//!
//! 定义了可在 Web UI 中编辑、以 JSON-LD 格式持久化的 DAG 工作流结构。
//! 与 petgraph DiGraph 不同，这些是反序列化的"蓝图"类型。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 条件分支
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCondition {
    /// 条件表达式，如 "$.result.status == 'failed'"
    pub condition: String,
    /// 条件满足时跳转的目标节点 ID
    pub target: String,
}

/// 输入映射：JSONPath 表达式 → 上下文键
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMapping {
    /// JSONPath 映射表，如 { "prev_summary": "$.nodes.step_1.summary" }
    #[serde(default)]
    pub mappings: HashMap<String, String>,
    /// context 模板，引用 mappings 中的键
    #[serde(default)]
    pub context_template: String,
}

/// 工作流中的单个 Agent 节点（JSON-LD 可序列化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeDef {
    /// 节点唯一 ID，对应 json-ld @id
    #[serde(rename = "@id")]
    pub id: String,
    /// 节点类型
    #[serde(rename = "@type", default = "default_node_type")]
    pub node_type: String,
    /// Agent 角色
    pub agent_role: String,
    /// 节点目标描述
    #[serde(default)]
    pub objective: String,
    /// 后续节点 ID（单下一步）
    #[serde(default)]
    pub next: Option<String>,
    /// 分叉：多个后续节点 ID（并行）
    #[serde(default)]
    pub next_nodes: Vec<String>,
    /// 依赖节点 ID（前置条件）
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// 允许的工具列表
    #[serde(default)]
    pub tools: Vec<String>,
    /// 成功标准
    #[serde(default)]
    pub success_criteria: String,
    /// 输入映射
    #[serde(default)]
    pub input_mapping: Option<InputMapping>,
    /// 条件分支（失败时跳转）
    #[serde(default)]
    pub branch_on_failure: Option<BranchCondition>,
    /// 重试配置
    #[serde(default)]
    pub retry_count: u32,
    /// 重试间隔秒数
    #[serde(default)]
    pub retry_delay_secs: u64,
    /// 节点超时秒数（0 = 不限制）
    #[serde(default)]
    pub timeout_secs: u64,
    /// 是否为最终节点
    #[serde(default)]
    pub final_node: bool,
    /// 自定义属性
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_node_type() -> String {
    "AgentNode".to_string()
}

/// 完整的工作流定义（JSON-LD @graph 格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// 工作流 IRI
    #[serde(rename = "@id")]
    pub id: String,
    /// 工作流名称
    #[serde(default)]
    pub name: String,
    /// 描述
    #[serde(default)]
    pub description: String,
    /// 版本号
    #[serde(default)]
    pub version: String,
    /// 入口节点 ID
    pub entry_node: String,
    /// 所有节点的定义
    pub nodes: Vec<WorkflowNodeDef>,
}

/// JSON-LD 容器（解析 @graph 数组）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContainer {
    #[serde(rename = "@context", default)]
    pub context: Value,
    #[serde(default)]
    pub graph: Vec<Value>,
}

/// 节点执行结果（内存中）
#[derive(Debug, Clone)]
pub struct NodeResult {
    pub node_id: String,
    pub status: String,
    pub summary: String,
    pub archive_iri: Option<String>,
    pub turn_count: u32,
    pub tool_call_count: u32,
    pub error: Option<String>,
    pub output: Option<Value>,
    pub artifacts: Vec<Value>,
}
