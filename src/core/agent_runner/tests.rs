use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn create_test_runner() -> AgentRunner {
    use crate::config::settings::AgentSettings;
    use crate::gateway::unified_gateway::UnifiedGateway;
    use crate::memory::l0_store::L0Store;
    use crate::memory::l2_blackboard::Blackboard;
    use crate::memory::memory_manager::MemoryManager;
    use crate::templates::template_engine::TemplateEngine;
    use crate::tools::skill_registry::SkillRegistry;
    use crate::config::settings::GatewaySettings;
    use crate::CoreConfig;
    use std::path::Path;

    let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let test_path = format!("./data/test_l0_{}", test_id);
    let l0 = Arc::new(L0Store::new(&test_path).unwrap());
    let blackboard = Arc::new(Blackboard::new().unwrap());
    let projection = Arc::new(ProjectionEngine::new(blackboard.clone(), 1024));
    let skills = Arc::new(SkillRegistry::new());
    let gateway_settings = GatewaySettings {
        base_url: "http://localhost:3000".to_string(),
        api_key: "test-key".to_string(),
        default_model: "deepseek-v4-pro".to_string(),
        timeout_seconds: 30,
        max_retries: 3,
        model_mapping: std::collections::HashMap::new(),
    };
    let gateway = Arc::new(UnifiedGateway::new(&gateway_settings).unwrap());
    let templates = Arc::new(TemplateEngine::new(Path::new("./templates")).unwrap());
    let config = CoreConfig::default();
    let memory_manager = Arc::new(tokio::sync::Mutex::new(MemoryManager::new(
        l0.clone(),
        blackboard.clone(),
        projection,
        config.clone(),
    )));
    let settings = AgentSettings::default();

    AgentRunner::new(
        gateway,
        skills,
        blackboard,
        l0,
        memory_manager,
        templates,
        settings,
    )
}

#[test]
fn test_parse_jsonld_response_valid() {
    let runner = create_test_runner();
    let response = json!({
        "@context": "https://pdca-agent.org/context/task",
        "@id": "iri://task/test123",
        "@type": "TaskNode",
        "summary": "Test task",
        "emphasis": ["重要约束1", "重要约束2"]
    })
    .to_string();

    let result = runner.parse_jsonld_response(&response);
    assert!(result.is_ok());

    let node = result.unwrap();
    assert_eq!(node.id, "iri://task/test123");
    assert_eq!(node.get_property("summary"), Some(&json!("Test task")));
}

#[test]
fn test_parse_jsonld_response_invalid() {
    let runner = create_test_runner();
    let response = json!({
        "summary": "Missing @id and @type"
    })
    .to_string();

    let result = runner.parse_jsonld_response(&response);
    assert!(result.is_err());
}

#[test]
fn test_extract_emphasis_from_array() {
    let runner = create_test_runner();
    let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
        .with_property("emphasis".to_string(), json!(["约束1", "约束2", "约束3"]));

    let emphasis = runner.extract_emphasis(&node);
    assert_eq!(emphasis.len(), 3);
    assert_eq!(emphasis[0], "约束1");
}

#[test]
fn test_extract_emphasis_from_string() {
    let runner = create_test_runner();
    let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
        .with_property("emphasis".to_string(), json!("单个强调内容"));

    let emphasis = runner.extract_emphasis(&node);
    assert_eq!(emphasis.len(), 1);
    assert_eq!(emphasis[0], "单个强调内容");
}

#[test]
fn test_extract_emphasis_with_constraints() {
    let runner = create_test_runner();
    let node = JsonLdNode::new("iri://task/test".to_string(), "TaskNode")
        .with_property("emphasis".to_string(), json!(["强调1"]))
        .with_property("constraints".to_string(), json!(["约束A", "约束B"]));

    let emphasis = runner.extract_emphasis(&node);
    assert_eq!(emphasis.len(), 3);
    assert!(emphasis.contains(&"强调1".to_string()));
    assert!(emphasis.contains(&"[约束] 约束A".to_string()));
}

#[test]
fn test_apply_output_mapping_plan() {
    let runner = create_test_runner();
    let output = json!({
        "plan": "执行计划内容",
        "steps": ["步骤1", "步骤2"],
        "objective": "任务目标"
    });

    let result = runner.apply_output_mapping(&output, &AgentRole::Plan, "iri://task/123");
    assert!(result.is_some());

    let jsonld = result.unwrap();
    assert!(jsonld.get("@id").is_some());
    assert_eq!(jsonld.get("execution_plan"), Some(&json!("执行计划内容")));
    assert_eq!(jsonld.get("plan_steps"), Some(&json!(["步骤1", "步骤2"])));
    assert_eq!(jsonld.get("task_iri"), Some(&json!("iri://task/123")));
    assert_eq!(jsonld.get("agent_role"), Some(&json!("PA")));
}

#[test]
fn test_apply_output_mapping_do() {
    let runner = create_test_runner();
    let output = json!({
        "result": "执行结果",
        "artifacts": ["文件1.py", "文件2.rs"]
    });

    let result = runner.apply_output_mapping(&output, &AgentRole::Do, "iri://task/456");
    assert!(result.is_some());

    let jsonld = result.unwrap();
    assert_eq!(jsonld.get("execution_result"), Some(&json!("执行结果")));
    assert_eq!(jsonld.get("created_artifacts"), Some(&json!(["文件1.py", "文件2.rs"])));
}

#[test]
fn test_apply_output_mapping_check() {
    let runner = create_test_runner();
    let output = json!({
        "review": "检查结果良好",
        "passed": true
    });

    let result = runner.apply_output_mapping(&output, &AgentRole::Check, "iri://task/789");
    assert!(result.is_some());

    let jsonld = result.unwrap();
    assert_eq!(jsonld.get("check_review"), Some(&json!("检查结果良好")));
    assert_eq!(jsonld.get("check_passed"), Some(&json!(true)));
}

#[test]
fn test_apply_output_mapping_act() {
    let runner = create_test_runner();
    let output = json!({
        "decision": "最终决策",
        "action": "执行下一步"
    });

    let result = runner.apply_output_mapping(&output, &AgentRole::Act, "iri://task/abc");
    assert!(result.is_some());

    let jsonld = result.unwrap();
    assert_eq!(jsonld.get("final_decision"), Some(&json!("最终决策")));
    assert_eq!(jsonld.get("recommended_action"), Some(&json!("执行下一步")));
}

#[test]
fn test_apply_output_mapping_string_output() {
    let runner = create_test_runner();
    let output = json!("简单的字符串输出");

    let result = runner.apply_output_mapping(&output, &AgentRole::Do, "iri://task/xyz");
    assert!(result.is_some());

    let jsonld = result.unwrap();
    assert_eq!(jsonld.get("content"), Some(&json!("简单的字符串输出")));
}

#[test]
fn test_task_result_jsonld_output() {
    let result = TaskResult {
        task_iri: "iri://task/test".to_string(),
        status: "success".to_string(),
        summary: "任务完成".to_string(),
        output: Some(json!("输出内容")),
        jsonld_output: Some(json!({
            "@id": "iri://task/test_output",
            "@type": "DoOutput",
            "content": "输出内容"
        })),
        artifacts: vec![],
        errors: vec![],
        turn_count: 5,
        tool_call_count: 3,
        five_w2h_updates: None,
        tracked_actions: Vec::new(),
    };

    assert!(result.jsonld_output.is_some());
    let jsonld = result.jsonld_output.unwrap();
    assert_eq!(jsonld.get("@id"), Some(&json!("iri://task/test_output")));
}

#[test]
fn test_try_extract_json_from_markdown_plain_json() {
    let input = r#"{"thought": "分析中", "content": "测试", "action": "continue"}"#;
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["action"], "continue");
}

#[test]
fn test_try_extract_json_from_markdown_json_code_block() {
    let input = "```json\n{\"thought\": \"思考\", \"content\": \"内容\", \"action\": \"tool_call\"}\n```";
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["action"], "tool_call");
}

#[test]
fn test_try_extract_json_from_markdown_code_block_no_lang() {
    let input = "```\n{\"thought\": \"思考\", \"content\": \"内容\"}\n```";
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["thought"], "思考");
}

#[test]
fn test_try_extract_json_from_markdown_with_surrounding_text() {
    let input = "好的，我来分析一下。\n{\"thought\": \"分析\", \"content\": \"结果\", \"action\": \"finish\"}\n以上就是我的分析。";
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["action"], "finish");
}

#[test]
fn test_try_extract_json_from_markdown_nested_braces() {
    let input = r#"{"thought": "嵌套", "content": {"sub": "value"}, "action": "continue"}"#;
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["content"]["sub"], "value");
}

#[test]
fn test_try_extract_json_from_markdown_no_json() {
    let input = "这是一段纯文本，没有JSON内容。";
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_none());
}

#[test]
fn test_try_extract_json_from_markdown_incomplete_json() {
    let input = r#"{"thought": "不完整", "content": "缺少结束括号"#;
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_none());
}

#[test]
fn test_try_extract_json_from_markdown_multiple_json_objects() {
    let input = r#"前一段 {"a": 1} 后一段 {"thought": "第二个", "content": "内容", "action": "finish"}"#;
    let result = AgentRunner::try_extract_json_from_markdown(input);
    assert!(result.is_some());
    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(parsed["a"], 1);
}

#[test]
fn test_task_result_partial_success_status() {
    let result = TaskResult {
        task_iri: "iri://task/test".to_string(),
        status: "partial_success".to_string(),
        summary: "任务部分完成".to_string(),
        output: None,
        jsonld_output: None,
        artifacts: vec![],
        errors: vec!["bash: timeout".to_string()],
        turn_count: 15,
        tool_call_count: 5,
        five_w2h_updates: None,
        tracked_actions: Vec::new(),
    };
    assert_eq!(result.status, "partial_success");
    assert!(!result.errors.is_empty());
    assert!(result.summary.contains("部分完成"));
}
