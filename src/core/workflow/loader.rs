//! JSON-LD → petgraph DAG 加载器
//!
//! 将 Web UI 生成的 JSON-LD 工作流定义反序列化并转换为 petgraph DiGraph。

use std::collections::HashMap;

use petgraph::prelude::*;
use serde_json::Value;
use tracing::{info, warn};

use super::definition::*;

/// 加载后的 DAG，包含 petgraph 图和节点/边权重映射
pub struct WorkflowDag {
    /// petgraph 有向图
    pub graph: DiGraph<GraphNode, GraphEdge>,
    /// 节点 ID → NodeIndex 映射
    pub node_index: HashMap<String, NodeIndex>,
    /// 入口节点索引
    pub entry_node: NodeIndex,
}

/// DAG 图中的节点权重
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub def: WorkflowNodeDef,
}

/// DAG 图中的边权重
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub relation: String, // "next" | "dependency" | "branch"
}

/// 从 JSON-LD 字符串加载工作流定义
///
/// 支持两种格式：
/// 1. 直接 `WorkflowDefinition`（snake_case 字段名）
/// 2. `@graph` 容器（JSON-LD 标准格式，`wf:` 前缀的字段名）
pub fn load_workflow_jsonld(json: &str) -> Result<WorkflowDefinition, String> {
    // 先尝试直接反序列化（处理简洁格式）
    if let Ok(wf) = serde_json::from_str::<WorkflowDefinition>(json) {
        return Ok(wf);
    }

    // 尝试解析为 @graph 容器
    let root: Value = serde_json::from_str(json)
        .map_err(|e| format!("JSON 解析失败: {}", e))?;

    let graph_array = match root.get("@graph") {
        Some(Value::Array(arr)) => arr,
        _ => return Err("无法解析 JSON-LD 工作流定义：既不是 WorkflowDefinition，也不含 @graph 数组".to_string()),
    };

    // 在 @graph 数组中找到 Workflow 定义和 AgentNode 定义
    let mut workflow_entry: Option<&Value> = None;
    let mut agent_nodes: Vec<&Value> = Vec::new();

    for entry in graph_array {
        match entry.get("@type").and_then(|t| t.as_str()) {
            Some("Workflow") | Some("wf:Workflow") => {
                workflow_entry = Some(entry);
            }
            Some("AgentNode") | Some("wf:AgentNode") => {
                agent_nodes.push(entry);
            }
            _ => {} // 忽略其他类型
        }
    }

    let wf_entry = workflow_entry.ok_or_else(|| {
        "在 @graph 数组中未找到 @type 为 Workflow 的条目".to_string()
    })?;

    // 从 Workflow 条目中提取字段（处理 wf: 前缀）
    let name = get_jsonld_str(wf_entry, "name")
        .unwrap_or_default();
    let description = get_jsonld_str(wf_entry, "description")
        .unwrap_or_default();
    let version = get_jsonld_str(wf_entry, "version")
        .unwrap_or_default();
    let entry_node = get_jsonld_str(wf_entry, "entryNode")
        .or_else(|| get_jsonld_str(wf_entry, "entry_node"))
        .ok_or_else(|| "Workflow 缺少 entryNode 或 entry_node".to_string())?;
    let wf_id = get_jsonld_str(wf_entry, "@id")
        .or_else(|| get_jsonld_str(wf_entry, "id"))
        .unwrap_or_else(|| "wf:unnamed".to_string());

    // 从 Workflow 的 nodes 列表获取节点顺序和引用
    let wf_node_refs: Vec<String> = get_jsonld_array(wf_entry, "nodes")
        .iter()
        .filter_map(|v| {
            // 节点可以是字符串 @id，也可以是对象
            v.as_str().map(|s| s.to_string())
                .or_else(|| v.get("@id").and_then(|id| id.as_str().map(|s| s.to_string())))
        })
        .collect();

    // 收集所有 AgentNode，建立 id → entry 映射
    let mut node_map: HashMap<String, &Value> = HashMap::new();
    for entry in &agent_nodes {
        let nid = get_jsonld_str(entry, "@id")
            .or_else(|| get_jsonld_str(entry, "id"))
            .unwrap_or_default();
        node_map.insert(nid, *entry);
    }

    // 按 wf_node_refs 顺序构建节点列表；如无引用列表则用 agent_nodes 原序
    let ordered_entries: Vec<&Value> = if wf_node_refs.is_empty() {
        agent_nodes
    } else {
        wf_node_refs.iter()
            .filter_map(|ref_id| {
                let found = node_map.get(ref_id).copied();
                if found.is_none() {
                    warn!("Workflow 引用了节点 '{}' 但在 @graph 中未找到", ref_id);
                }
                found
            })
            .collect()
    };

    // 将每个 AgentNode entry 转换为 WorkflowNodeDef
    let mut nodes = Vec::new();
    for entry in ordered_entries {
        nodes.push(parse_agent_node(entry)?);
    }

    Ok(WorkflowDefinition {
        id: wf_id,
        name,
        description,
        version,
        entry_node,
        nodes,
    })
}

/// 从 JSON-LD 值中提取字符串字段（支持 wf: 前缀和 snake_case）
fn get_jsonld_str(val: &Value, field: &str) -> Option<String> {
    let variants = [
        field.to_string(),
        format!("wf:{}", field),
        field.replace('_', ""),           // entry_node → entrynode
    ];
    for v in &variants {
        if let Some(s) = val.get(v).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

/// 从 JSON-LD 值中提取数组字段（支持 wf: 前缀）
fn get_jsonld_array<'a>(val: &'a Value, field: &str) -> Vec<&'a Value> {
    let variants = [field.to_string(), format!("wf:{}", field)];
    for v in &variants {
        if let Some(Value::Array(arr)) = val.get(v) {
            return arr.iter().collect();
        }
    }
    vec![]
}

/// 从 JSON-LD 值中提取布尔字段
fn _get_jsonld_bool(val: &Value, field: &str) -> Option<bool> {
    let variants = [field.to_string(), format!("wf:{}", field)];
    for v in &variants {
        if let Some(b) = val.get(v).and_then(|v| v.as_bool()) {
            return Some(b);
        }
    }
    None
}

/// 从 JSON-LD 值中提取 u64 字段（支持整数和字符串数字）
fn get_jsonld_u64(val: &Value, field: &str) -> Option<u64> {
    let variants = [field.to_string(), format!("wf:{}", field)];
    for v in &variants {
        if let Some(n) = val.get(v).and_then(|v| v.as_u64()) {
            return Some(n);
        }
        // 也尝试解析字符串数字
        if let Some(s) = val.get(v).and_then(|v| v.as_str()) {
            if let Ok(n) = s.parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

/// 解析 JSON-LD AgentNode entry 为 WorkflowNodeDef
fn parse_agent_node(entry: &Value) -> Result<WorkflowNodeDef, String> {
    let node_id = get_jsonld_str(entry, "@id")
        .or_else(|| get_jsonld_str(entry, "id"))
        .ok_or_else(|| "AgentNode 缺少 @id".to_string())?;

    let agent_role = get_jsonld_str(entry, "agent_role")
        .or_else(|| get_jsonld_str(entry, "agentRole"))
        .ok_or_else(|| format!("节点 {} 缺少 agent_role", node_id))?;

    let objective = get_jsonld_str(entry, "objective").unwrap_or_default();
    let expected_output = get_jsonld_str(entry, "expected_output")
        .or_else(|| get_jsonld_str(entry, "expectedOutput"))
        .unwrap_or_default();
    let success_criteria = get_jsonld_str(entry, "success_criteria")
        .or_else(|| get_jsonld_str(entry, "successCriteria"))
        .unwrap_or_default();

    let node_type = get_jsonld_str(entry, "@type")
        .unwrap_or_else(|| "AgentNode".to_string());

    // 解析字符串数组字段
    let tools = parse_string_array(entry, "tools");
    let dependencies = parse_string_array(entry, "dependencies");
    let next_nodes = parse_string_array(entry, "next_nodes")
        .or_else(|| parse_string_array(entry, "nextNodes"))
        .unwrap_or_default();

    let next = get_jsonld_str(entry, "next");
    let final_node = get_jsonld_str(entry, "final_node")
        .or_else(|| get_jsonld_str(entry, "finalNode"))
        .map(|s| s == "true" || s == "True")
        .unwrap_or(false);

    let retry_count = get_jsonld_u64(entry, "retry_count")
        .or_else(|| get_jsonld_u64(entry, "retryCount"))
        .unwrap_or(0) as u32;
    let retry_delay_secs = get_jsonld_u64(entry, "retry_delay_secs")
        .or_else(|| get_jsonld_u64(entry, "retryDelaySecs"))
        .unwrap_or(0);
    let timeout_secs = get_jsonld_u64(entry, "timeout_secs")
        .or_else(|| get_jsonld_u64(entry, "timeoutSecs"))
        .unwrap_or(0);

    // 解析 branch_on_failure
    let branch_on_failure = parse_branch_condition(entry);

    // 解析 input_mapping
    let input_mapping = parse_input_mapping(entry);

    Ok(WorkflowNodeDef {
        id: node_id,
        node_type,
        agent_role,
        objective,
        next,
        next_nodes,
        dependencies: dependencies.unwrap_or_default(),
        tools: tools.unwrap_or_default(),
        expected_output,
        success_criteria,
        input_mapping,
        branch_on_failure,
        retry_count,
        retry_delay_secs,
        timeout_secs,
        final_node,
        extra: Default::default(),
    })
}

/// 解析字符串数组字段
fn parse_string_array(entry: &Value, field: &str) -> Option<Vec<String>> {
    let variants = [field.to_string(), format!("wf:{}", field)];
    for v in &variants {
        if let Some(Value::Array(arr)) = entry.get(v) {
            let strs: Vec<String> = arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect();
            if !strs.is_empty() || !arr.is_empty() {
                return Some(strs);
            }
        }
    }
    None
}

/// 解析 branch_on_failure 条件分支
fn parse_branch_condition(entry: &Value) -> Option<BranchCondition> {
    let variants = ["branch_on_failure", "branchOnFailure", "wf:branchOnFailure"];
    for var in &variants {
        if let Some(bf) = entry.get(*var) {
            let condition = bf.get("condition")
                .or_else(|| bf.get("wf:condition"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let target = bf.get("target")
                .or_else(|| bf.get("wf:target"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let (Some(cond), Some(tgt)) = (condition, target) {
                return Some(BranchCondition {
                    condition: cond,
                    target: tgt,
                });
            }
        }
    }
    None
}

/// 解析 input_mapping
fn parse_input_mapping(entry: &Value) -> Option<InputMapping> {
    let variants = ["input_mapping", "inputMapping", "wf:inputMapping"];
    for var in &variants {
        if let Some(im) = entry.get(*var) {
            let mappings: HashMap<String, String> = im.get("mappings")
                .or_else(|| im.get("wf:mappings"))
                .and_then(|m| m.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let context_template = im.get("context_template")
                .or_else(|| im.get("contextTemplate"))
                .or_else(|| im.get("wf:contextTemplate"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if !mappings.is_empty() || !context_template.is_empty() {
                return Some(InputMapping {
                    mappings,
                    context_template,
                });
            }
        }
    }
    None
}

/// 从 WorkflowNodeDef 列表构建 petgraph DiGraph（自动推断依赖关系）
pub fn build_dag(def: &WorkflowDefinition) -> Result<WorkflowDag, String> {
    let mut graph = DiGraph::<GraphNode, GraphEdge>::new();
    let mut node_index: HashMap<String, NodeIndex> = HashMap::new();

    if def.nodes.is_empty() {
        return Err("工作流定义中没有节点".to_string());
    }

    // Phase 1: 添加所有节点
    for node_def in &def.nodes {
        let idx = graph.add_node(GraphNode {
            def: node_def.clone(),
        });
        node_index.insert(node_def.id.clone(), idx);
    }

    // Phase 2: 建立边
    for node_def in &def.nodes {
        let src_idx = node_index.get(&node_def.id).ok_or_else(|| {
            format!("节点 {} 在 node_index 中不存在", node_def.id)
        })?;

        // 2a: next 边（主要执行流）
        if let Some(ref next_id) = node_def.next {
            if let Some(dst_idx) = node_index.get(next_id) {
                graph.add_edge(*src_idx, *dst_idx, GraphEdge {
                    relation: "next".to_string(),
                });
            } else {
                warn!("节点 {} 的 next '{}' 不存在", node_def.id, next_id);
            }
        }

        // 2b: next_nodes 边（并行分叉）
        for next_id in &node_def.next_nodes {
            if let Some(dst_idx) = node_index.get(next_id) {
                graph.add_edge(*src_idx, *dst_idx, GraphEdge {
                    relation: "next".to_string(),
                });
            } else {
                warn!("节点 {} 的 next_node '{}' 不存在", node_def.id, next_id);
            }
        }

        // 2c: dependencies 边（额外前置约束）
        for dep_id in &node_def.dependencies {
            if let Some(dep_idx) = node_index.get(dep_id) {
                graph.add_edge(*dep_idx, *src_idx, GraphEdge {
                    relation: "dependency".to_string(),
                });
            }
        }

        // 2d: branch_on_failure 边（跳过自环—自环是运行时重试，不是 DAG 依赖）
        if let Some(ref branch) = node_def.branch_on_failure {
            if branch.target != node_def.id {
                if let Some(dst_idx) = node_index.get(&branch.target) {
                    graph.add_edge(*src_idx, *dst_idx, GraphEdge {
                        relation: "branch".to_string(),
                    });
                }
            }
        }
    }

    let entry_idx = *node_index.get(&def.entry_node).ok_or_else(|| {
        format!("入口节点 '{}' 在节点列表中不存在", def.entry_node)
    })?;

    info!(
        nodes = def.nodes.len(),
        edges = graph.edge_count(),
        entry = %def.entry_node,
        "DAG 构建完成"
    );

    Ok(WorkflowDag {
        graph,
        node_index,
        entry_node: entry_idx,
    })
}

/// 检测图中是否存在环
pub fn has_cycle(dag: &WorkflowDag) -> bool {
    petgraph::algo::is_cyclic_directed(&dag.graph)
}

/// 拓扑排序，返回执行顺序
pub fn topological_order(dag: &WorkflowDag) -> Result<Vec<NodeIndex>, String> {
    match petgraph::algo::toposort(&dag.graph, None) {
        Ok(order) => Ok(order),
        Err(cycle) => {
            let cycle_node = cycle.node_id();
            let node = &dag.graph[cycle_node];
            Err(format!("检测到环，涉及节点: {}", node.def.id))
        }
    }
}

/// 获取节点的所有前驱（入边邻居）
pub fn predecessors(dag: &WorkflowDag, node: NodeIndex) -> Vec<NodeIndex> {
    dag.graph
        .neighbors_directed(node, Incoming)
        .collect()
}

/// 判断节点的所有依赖是否已完成
pub fn all_dependencies_met(
    dag: &WorkflowDag,
    node: NodeIndex,
    completed: &HashMap<String, NodeResult>,
) -> bool {
    for pred in dag.graph.neighbors_directed(node, Incoming) {
        let pred_id = &dag.graph[pred].def.id;
        if !completed.contains_key(pred_id) {
            return false;
        }
    }
    true
}

/// 检查条件分支是否应触发
pub fn should_branch(
    node_def: &WorkflowNodeDef,
    result: &NodeResult,
) -> Option<String> {
    if let Some(ref branch) = node_def.branch_on_failure {
        let matches = match branch.condition.as_str() {
            "$.result.status == 'failed'" => result.status == "failed",
            "$.result.status == 'success'" => result.status == "success",
            _ => {
                // 简单字符串匹配条件
                let cond = branch.condition.trim();
                result.status == cond || result.summary.contains(cond)
            }
        };
        if matches {
            return Some(branch.target.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_jsonld_workflow() {
        let json = r#"{
            "@id": "wf:test",
            "name": "Test",
            "description": "test",
            "version": "1.0",
            "entry_node": "step_1",
            "nodes": [
                {
                    "@id": "step_1",
                    "@type": "AgentNode",
                    "agent_role": "Plan",
                    "objective": "plan step",
                    "next": "step_2"
                },
                {
                    "@id": "step_2",
                    "@type": "AgentNode",
                    "agent_role": "Do",
                    "objective": "do step",
                    "final_node": true
                }
            ]
        }"#;

        let def = load_workflow_jsonld(json).unwrap();
        assert_eq!(def.nodes.len(), 2);
        assert_eq!(def.entry_node, "step_1");
    }

    #[test]
    fn test_build_dag_linear() {
        let json = r#"{
            "@id": "wf:linear",
            "name": "Linear",
            "description": "linear test",
            "version": "1.0",
            "entry_node": "a",
            "nodes": [
                {"@id": "a", "@type": "AgentNode", "agent_role": "Do", "objective": "A", "next": "b"},
                {"@id": "b", "@type": "AgentNode", "agent_role": "Do", "objective": "B"}
            ]
        }"#;
        let def = load_workflow_jsonld(json).unwrap();
        let dag = build_dag(&def).unwrap();
        assert!(!has_cycle(&dag));
        let order = topological_order(&dag).unwrap();
        assert_eq!(order.len(), 2);
        // Verify topology: a before b
        let idx_a = dag.node_index.get("a").unwrap();
        let idx_b = dag.node_index.get("b").unwrap();
        let pos_a = order.iter().position(|i| i == idx_a).unwrap();
        let pos_b = order.iter().position(|i| i == idx_b).unwrap();
        assert!(pos_a < pos_b, "a should come before b in topological order");
    }

    #[test]
    fn test_cycle_detection() {
        let json = r#"{
            "@id": "wf:cycle",
            "name": "Cycle",
            "description": "cycle test",
            "version": "1.0",
            "entry_node": "a",
            "nodes": [
                {"@id": "a", "@type": "AgentNode", "agent_role": "Do", "objective": "A", "next": "b"},
                {"@id": "b", "@type": "AgentNode", "agent_role": "Do", "objective": "B", "next": "a"}
            ]
        }"#;
        let def = load_workflow_jsonld(json).unwrap();
        let dag = build_dag(&def).unwrap();
        assert!(has_cycle(&dag));
    }

    #[test]
    fn test_condition_branch() {
        let json = r#"{
            "@id": "wf:branch",
            "name": "Branch",
            "description": "branch test",
            "version": "1.0",
            "entry_node": "a",
            "nodes": [
                {"@id": "a", "@type": "AgentNode", "agent_role": "Do", "objective": "A", "next": "b"},
                {"@id": "b", "@type": "AgentNode", "agent_role": "Do", "objective": "B",
                 "branch_on_failure": {"condition": "$.result.status == 'failed'", "target": "c"}},
                {"@id": "c", "@type": "AgentNode", "agent_role": "Act", "objective": "C"}
            ]
        }"#;
        let def = load_workflow_jsonld(json).unwrap();
        let dag = build_dag(&def).unwrap();
        assert!(!has_cycle(&dag));
        let order = topological_order(&dag).unwrap();
        assert_eq!(order.len(), 3);

        // Test should_branch
        let node_b = &dag.graph[*dag.node_index.get("b").unwrap()].def;
        let result = NodeResult {
            node_id: "b".to_string(),
            status: "failed".to_string(),
            summary: "failed".to_string(),
            archive_iri: None,
            turn_count: 0,
            tool_call_count: 0,
            error: Some("error".to_string()),
            output: None,
            artifacts: vec![],
        };
        let branch_target = should_branch(node_b, &result);
        assert_eq!(branch_target, Some("c".to_string()));

        // No branch on success
        let result_ok = NodeResult {
            status: "success".to_string(),
            ..result.clone()
        };
        let no_branch = should_branch(node_b, &result_ok);
        assert_eq!(no_branch, None);
    }
}
