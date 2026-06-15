use std::collections::HashMap;
use std::sync::atomic::Ordering;

use serde_json::{json, Value};
use tracing::{debug, error, info, trace, warn};

use crate::core::agent_instance::{AgentInstance, AgentRole, AgentStatus};
use crate::gateway::unified_gateway::ChatMessage;
use crate::jsonld::{generate_iri, validate_jsonld_node, JsonLdContext, JsonLdNode};
use crate::memory::l1_session::L1Session;
use crate::tools::hooks::{HookContext, HookPoint, HookResult};
use crate::tools::tool_executor::ToolExecutor;
use crate::core::system_prompt::{SystemPromptBuilder, SystemPromptRegion, build_constitution_prompt};
use crate::CoreError;

use super::{LlmParsedResponse, TaskContext, TaskResult, LLM_RESPONSE_FORMAT_NO_THOUGHT, LLM_RESPONSE_FORMAT_WITH_THOUGHT};

impl super::AgentRunner {
    fn extract_summary(&self, content: &str, reasoning_content: Option<&str>) -> String {
        // 优先从 JSON 中提取 summary 字段
        if let Ok(parsed) = serde_json::from_str::<Value>(content) {
            if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                return summary.chars().take(500).collect();
            }
            // 如果有原生思考，不需要从 JSON 中提取 thought（避免重复）
            if reasoning_content.is_none() {
                if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                    return thought.chars().take(500).collect();
                }
            }
            if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                return content_str.chars().take(500).collect();
            }
        }

        // 如果有原生思考内容，使用它作为 summary
        if let Some(reasoning) = reasoning_content {
            let reasoning_summary: String = reasoning.chars().take(300).collect();
            return format!("[思考] {}", reasoning_summary);
        }

        // 最后 fallback：直接使用 content 前 500 字符
        content.chars().take(500).collect()
    }

    pub(super) fn parse_llm_response(
        &self,
        content: &str,
        reasoning_content: Option<&str>,
        supports_native_reasoning: bool,
    ) -> LlmParsedResponse {
        let mut response = LlmParsedResponse {
            thought: None,
            content: content.to_string(),
            summary: None,
            action: None,
            is_valid_json: false,
            has_native_reasoning: reasoning_content.is_some(),
            emphasis: Vec::new(),
        };

        // 如果有原生思考内容，直接使用
        if let Some(reasoning) = reasoning_content {
            response.thought = Some(reasoning.to_string());
            response.has_native_reasoning = true;
        }

        // 尝试解析 JSON
        if let Ok(parsed) = serde_json::from_str::<Value>(content) {
            response.is_valid_json = true;

            // 提取 summary
            if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                response.summary = Some(summary.to_string());
            }

            // 提取 content
            if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                response.content = content_str.to_string();
            }

            // 提取 thought（仅当模型不支持原生思考时）
            if !supports_native_reasoning {
                if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                    response.thought = Some(thought.to_string());
                }
            }

            // 提取 emphasis 字段（LLM 自己识别的强调内容）
            if let Some(emphasis) = parsed.get("emphasis") {
                if let Some(arr) = emphasis.as_array() {
                    response.emphasis = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                } else if let Some(s) = emphasis.as_str() {
                    response.emphasis = vec![s.to_string()];
                }
            }

            let content_text = parsed.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let keyword_emphasis = Self::extract_emphasis_by_keywords(content_text);
            for kw_em in keyword_emphasis {
                if !response.emphasis.iter().any(|e| e == &kw_em) {
                    response.emphasis.push(kw_em);
                }
            }

            // 提取 action 字段
            if let Some(action) = parsed.get("action").and_then(|a| a.as_str()) {
                response.action = Some(action.to_string());
            }
        } else {
            if let Some(extracted) = Self::try_extract_json_from_markdown(content) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&extracted) {
                    response.is_valid_json = true;
                    if let Some(summary) = parsed.get("summary").and_then(|s| s.as_str()) {
                        response.summary = Some(summary.to_string());
                    }
                    if let Some(content_str) = parsed.get("content").and_then(|s| s.as_str()) {
                        response.content = content_str.to_string();
                    }
                    if !supports_native_reasoning {
                        if let Some(thought) = parsed.get("thought").and_then(|s| s.as_str()) {
                            response.thought = Some(thought.to_string());
                        }
                    }
                    if let Some(action) = parsed.get("action").and_then(|a| a.as_str()) {
                        response.action = Some(action.to_string());
                    }
                } else {
                    response.summary = Some(Self::generate_auto_summary(content));
                }
            } else {
                response.summary = Some(Self::generate_auto_summary(content));
            }
        }

        response
    }

    pub(super) fn generate_auto_summary(content: &str) -> String {
        let content_clean = content.trim();
        if content_clean.len() <= 200 {
            return content_clean.to_string();
        }

        if let Some(first_sentence_end) =
            content_clean.find(|c| c == '。' || c == '.' || c == '！' || c == '!')
        {
            let end_byte = first_sentence_end
                + content_clean[first_sentence_end..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            if end_byte <= 200 {
                return content_clean[..end_byte].to_string();
            }
        }

        content_clean.chars().take(200).collect()
    }

    pub(crate) fn try_extract_json_from_markdown(content: &str) -> Option<String> {
        let trimmed = content.trim();

        if trimmed.starts_with("```json") {
            let without_start = trimmed.trim_start_matches("```json").trim();
            if let Some(pos) = without_start.rfind("```") {
                return Some(without_start[..pos].trim().to_string());
            }
            return Some(without_start.trim().to_string());
        }

        if trimmed.starts_with("```") {
            let without_start = trimmed.trim_start_matches("```").trim();
            if let Some(pos) = without_start.rfind("```") {
                let candidate = without_start[..pos].trim();
                if candidate.starts_with('{') && candidate.ends_with('}') {
                    return Some(candidate.to_string());
                }
            }
            return Some(without_start.trim().to_string());
        }

        if let Some(start) = trimmed.find('{') {
            let mut depth = 0i32;
            for (i, c) in trimmed[start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(trimmed[start..start + i + 1].to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        None
    }

    pub(super) async fn save_emphasis_to_l0(
        &self,
        emphasis_items: &[String],
        task_iri: &str,
        agent_id: &str,
        dedup_threshold: f64,
    ) {
        if emphasis_items.is_empty() {
            return;
        }

        // 应用 max_items 截断，防止 emphasis 无限膨胀
        let max_items = self
            .emphasis_config
            .as_ref()
            .map(|c| c.max_items)
            .unwrap_or(50);
        let items: Vec<&String> = emphasis_items.iter().take(max_items).collect();

        // 先加载已有的强调内容用于去重
        let existing = self.load_emphasis_from_l0(task_iri).await;

        for content in items {
            // 去重检测
            let is_duplicate = existing.iter().any(|existing_content| {
                let similarity = Self::calculate_similarity(content, existing_content);
                similarity >= dedup_threshold
            });

            if is_duplicate {
                debug!(
                    "[L0] 跳过重复强调内容: {}",
                    content.chars().take(50).collect::<String>()
                );
                continue;
            }

            let iri = format!(
                "iri://emphasis/{}/{}",
                task_iri.strip_prefix("iri://").unwrap_or(task_iri),
                uuid::Uuid::new_v4()
            );
            let node = json!({
                "@id": &iri,
                "@type": "EmphasisContent",
                "content": content,
                "task_iri": task_iri,
                "agent_id": agent_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "permanent": true
            });

            if let Err(e) = self.l0_store.store(&iri, &node.to_string()) {
                warn!("保存强调内容到 L0 失败: {}", e);
            } else {
                info!("[L0] 保存强调内容: {} -> {}", agent_id, &iri);
            }
        }
    }

    pub(super) async fn load_emphasis_from_l0(&self, task_iri: &str) -> Vec<String> {
        let mut result = Vec::new();

        // 使用 IRI 前缀扫描替代全量标签搜索
        // 保存时 IRI 格式: iri://emphasis/{task_iri}/{uuid}
        let scan_prefix = format!(
            "iri://emphasis/{}",
            task_iri.strip_prefix("iri://").unwrap_or(task_iri)
        );
        if let Ok(entries) = self.l0_store.scan_iri_prefix(&scan_prefix, 200) {
            for entry in &entries {
                if let Ok(parsed) = serde_json::from_str::<Value>(&entry.content) {
                    if let Some(content) = parsed.get("content").and_then(|c| c.as_str()) {
                        result.push(content.to_string());
                    }
                }
            }
        }

        // 也加载全局 emphasis（无 task_iri 的条目），使用 emphasis 标签回退扫描
        if let Ok(nodes) = self.l0_store.search_by_tags(&[String::from("emphasis")]) {
            for node in nodes {
                if let Ok(parsed) = serde_json::from_str::<Value>(&node.content) {
                    let is_global = parsed.get("task_iri").is_none();
                    if is_global {
                        if let Some(content) = parsed.get("content").and_then(|c| c.as_str()) {
                            if !result.contains(&content.to_string()) {
                                result.push(content.to_string());
                            }
                        }
                    }
                }
            }
        }

        result
    }

    fn calculate_similarity(a: &str, b: &str) -> f64 {
        if a == b {
            return 1.0;
        }

        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();

        if a_chars.is_empty() || b_chars.is_empty() {
            return 0.0;
        }

        // 使用简单的 Jaccard 相似度
        let a_set: std::collections::HashSet<char> = a_chars.iter().copied().collect();
        let b_set: std::collections::HashSet<char> = b_chars.iter().copied().collect();

        let intersection = a_set.intersection(&b_set).count();
        let union = a_set.union(&b_set).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f64 / union as f64
    }

    pub(crate) fn parse_jsonld_response(&self, response: &str) -> Result<JsonLdNode, CoreError> {
        let parsed: Value =
            serde_json::from_str(response).map_err(|e| CoreError::InvalidJsonLd {
                message: format!("Failed to parse JSON: {}", e),
            })?;

        if let Err(e) = validate_jsonld_node(&parsed) {
            return Err(CoreError::InvalidJsonLd {
                message: format!("Invalid JSON-LD node: {}", e),
            });
        }

        JsonLdNode::from_json(&parsed).map_err(|e| CoreError::InvalidJsonLd {
            message: format!("Failed to parse JsonLdNode: {}", e),
        })
    }

    pub(super) fn extract_emphasis(&self, node: &JsonLdNode) -> Vec<String> {
        let mut emphasis_items = Vec::new();

        if let Some(emphasis) = node.get_property("emphasis") {
            match emphasis {
                Value::Array(arr) => {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            if !s.is_empty() {
                                emphasis_items.push(s.to_string());
                            }
                        }
                    }
                }
                Value::String(s) => {
                    if !s.is_empty() {
                        emphasis_items.push(s.clone());
                    }
                }
                _ => {}
            }
        }

        if let Some(constraints) = node.get_property("constraints") {
            if let Some(arr) = constraints.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.is_empty() {
                            emphasis_items.push(format!("[约束] {}", s));
                        }
                    }
                }
            }
        }

        emphasis_items
    }

    fn extract_emphasis_by_keywords(text: &str) -> Vec<String> {
        let keywords = [
            "必须", "重要", "关键", "务必", "不要忘记", "切记", "一定",
            "禁止", "不允许", "注意", "千万不要", "绝不能",
            "MUST", "IMPORTANT", "CRITICAL", "NEVER", "ALWAYS",
            "REQUIRED", "MANDATORY", "ESSENTIAL", "WARNING",
        ];
        let mut results = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            for keyword in &keywords {
                if trimmed.contains(keyword) {
                    let clean = if trimmed.len() > 200 {
                        let mut end = 200;
                        while end > 0 && !trimmed.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &trimmed[..end])
                    } else {
                        trimmed.to_string()
                    };
                    if !results.contains(&clean) {
                        results.push(clean);
                    }
                    break;
                }
            }
        }
        results
    }

    pub(super) fn apply_output_mapping(
        &self,
        output: &Value,
        role: &AgentRole,
        task_iri: &str,
    ) -> Option<Value> {
        let output_mapping = match role {
            AgentRole::Plan => HashMap::from([
                ("plan".to_string(), "execution_plan".to_string()),
                ("steps".to_string(), "plan_steps".to_string()),
                ("objective".to_string(), "task_objective".to_string()),
            ]),
            AgentRole::Do => HashMap::from([
                ("result".to_string(), "execution_result".to_string()),
                ("output".to_string(), "do_output".to_string()),
                ("artifacts".to_string(), "created_artifacts".to_string()),
            ]),
            AgentRole::Check => HashMap::from([
                ("review".to_string(), "check_review".to_string()),
                ("issues".to_string(), "found_issues".to_string()),
                ("passed".to_string(), "check_passed".to_string()),
            ]),
            AgentRole::Act => HashMap::from([
                ("decision".to_string(), "final_decision".to_string()),
                ("action".to_string(), "recommended_action".to_string()),
                ("summary".to_string(), "act_summary".to_string()),
            ]),
        };

        let node_id = generate_iri(
            "task",
            &format!(
                "{}_{}",
                role.to_string().to_lowercase(),
                uuid::Uuid::new_v4()
            ),
        );
        let mut node = JsonLdNode::new(node_id.clone(), format!("{}Output", role.to_string()))
            .with_context((*JsonLdContext::context_value()).clone());

        if let Some(obj) = output.as_object() {
            for (key, value) in obj {
                let mapped_key = output_mapping
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| key.clone());
                node = node.with_property(mapped_key, value.clone());
            }
        } else {
            node = node.with_property("content".to_string(), output.clone());
        }

        node = node.with_property("task_iri".to_string(), Value::String(task_iri.to_string()));
        node = node.with_property("agent_role".to_string(), Value::String(role.to_string()));
        node = node.with_property(
            "timestamp".to_string(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );

        node.to_json().ok()
    }

    pub(super) async fn store_jsonld_to_l2(
        &self,
        node: &JsonLdNode,
        task_iri: &str,
    ) -> Result<String, CoreError> {
        let node_iri = node.id.clone();
        let node_json = node.to_json().map_err(|e| CoreError::Internal {
            message: format!("Failed to serialize JsonLdNode: {}", e),
        })?;

        let cfg = crate::CoreConfig::default();
        self.blackboard
            .write_node(&node_iri, &node_json.to_string(), &cfg)?;

        info!("[L2] 存储 JSON-LD 节点: {} for task {}", node_iri, task_iri);
        Ok(node_iri)
    }

    pub async fn execute_streaming<F>(
        &self,
        agent: &mut AgentInstance,
        ctx: TaskContext,
        mut on_event: F,
    ) -> Result<TaskResult, CoreError>
    where
        F: FnMut(&crate::llm::StreamEvent) + Send,
    {
        agent.status = AgentStatus::Running;

        let task_iri_for_guard = ctx.task_iri.clone();
        let mut session = self.memory_manager.lock().await.create_session(
            &agent.agent_id,
            &agent.role.to_string(),
            &ctx.task_iri,
        );

        // 计算任务 embedding，用于语义相关度淘汰
        if let Some(ref embedder) = self.embedder {
            if let Ok(task_emb) = embedder.embed(&ctx.objective).await {
                session.set_task_embedding(task_emb.clone());
                if let Some(ref tracker_lock) = self.relevance_tracker {
                    let mut tracker = tracker_lock.lock().unwrap();
                    tracker.reset();
                    tracker.set_task_context(task_emb);
                }
            }
        }

        let result = self.execute_streaming_inner(agent, ctx, session, on_event).await;

        session = result.1;

        {
            let mut mm = self.memory_manager.lock().await;
            let _ = mm.finalize_session(session, &task_iri_for_guard);
        }

        result.0
    }

    async fn execute_streaming_inner<F>(
        &self,
        agent: &mut AgentInstance,
        ctx: TaskContext,
        mut session: L1Session,
        mut on_event: F,
    ) -> (Result<TaskResult, CoreError>, L1Session)
    where
        F: FnMut(&crate::llm::StreamEvent) + Send,
    {

        let model = self
            .gateway
            .get_model(&agent.role.to_string().to_lowercase());
        let supports_reasoning = self.gateway.supports_native_reasoning(&model);

        let context_data = self.gather_context_data_async(agent.role, &ctx).await;
        let agent_md = self.build_agent_md(agent.role, &ctx.objective, &context_data, &model);

        let mut prompt_builder = SystemPromptBuilder::new();
        prompt_builder.set_region(SystemPromptRegion::RoleDefinition, agent_md.clone());

        let emphasis_items = self.load_emphasis_from_l0(&ctx.task_iri).await;
        if !emphasis_items.is_empty() {
            let emphasis_content = emphasis_items
                .iter()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            prompt_builder.set_region(SystemPromptRegion::EmphasizedConstraints, emphasis_content);
        }

        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT.to_string()
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT.to_string()
        };
        prompt_builder.set_region(SystemPromptRegion::OutputFormat, format_constraint);

        let tool_menu = self.build_readable_tool_menu(&agent.role);
        if !tool_menu.is_empty() {
            prompt_builder.set_region(SystemPromptRegion::Tools, tool_menu);
        }

        let system_content = prompt_builder.build();

        let summary_chain = session.get_summary_chain();
        let summary_text = summary_chain
            .first()
            .and_then(|v| v.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let context_msg = if summary_text.is_empty() {
            format!(
                "## 当前任务\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective
            )
        } else {
            format!(
                "## 当前任务\n{}\n\n## 历史摘要\n{}\n\n## 可用工具\n请根据需要使用工具完成任务。",
                ctx.objective, summary_text
            )
        };

        let messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_content,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: context_msg,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        ];

        let tools = self
            .tool_executor
            .read()
            .expect("tool_executor RwLock poisoned")
            .tool_definitions_for_role(&agent.role.to_string());

        info!(
            "AgentRunner Streaming 开始: role={}, model={}, tools={}",
            agent.role,
            model,
            tools.len()
        );

        let mut running_messages = messages;
        let max_turns = 10u32;
        let mut tc = 0u32;
        let mut turn = 0u32;
        let mut errs = Vec::new();
        let mut guard_pending_pre_injections: Vec<String> = Vec::new();
        let mut last_content = String::new();
        let mut last_thought = String::new();
        let mut last_summary = String::new();

        loop {
            if !guard_pending_pre_injections.is_empty() {
                let prompt = format!(
                    "\n\n[ToolGuard 约束指令]\n{}\n注意：以上约束仅适用于你接下来发起的同名工具调用。请严格遵守。",
                    guard_pending_pre_injections.join("\n")
                );
                if let Some(sys_msg) = running_messages.first_mut() {
                    if sys_msg.role == "system" {
                        sys_msg.content.push_str(&prompt);
                    }
                }
                guard_pending_pre_injections.clear();
            }

            let mut stream = match self.gateway
                .stream_chat_with_params(
                    &model,
                    running_messages.clone(),
                    None,
                    None,
                    {
                        // 每次调用前刷新 tools 列表，确保新注册的微工具被包含
                        let current_tools = self
                            .tool_executor
                            .read()
                            .expect("tool_executor RwLock poisoned")
                            .tool_definitions_for_role(&agent.role.to_string());
                        if current_tools.is_empty() { None } else { Some(current_tools) }
                    },
                    None,
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => return (Err(e), session),
                };

            let mut accumulator = crate::llm::StreamAccumulator::new();

            let stream_result: Result<(), CoreError> = loop {
                match stream.next_event().await {
                    Ok(Some(event)) => {
                        on_event(&event);
                        accumulator.process_event(&event);
                        if let crate::llm::StreamEvent::MessageStop(_) = event {
                            break Ok(());
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(e) => break Err(CoreError::Internal { message: e.to_string() }),
                }
            };
            if let Err(e) = stream_result {
                return (Err(e), session);
            }

            let stream_response: crate::llm::StreamResponse = accumulator.into();

            // Accumulate token usage from streaming response
            if let Some(ref usage) = stream_response.usage {
                self.total_prompt_tokens.fetch_add(usage.prompt_tokens as u64, Ordering::Relaxed);
                self.total_completion_tokens.fetch_add(usage.completion_tokens as u64, Ordering::Relaxed);
            }

            let parsed = self.parse_llm_response(
                &stream_response.content,
                stream_response.thought.as_deref(),
                supports_reasoning,
            );

            match parsed.action.as_deref() {
                Some("tool_call") => {
                    if !stream_response.tool_calls.is_empty() {
                        let tool_calls = &stream_response.tool_calls;
                        if agent.role == AgentRole::Plan {
                            let write_tools: Vec<&str> = tool_calls
                                .iter()
                                .map(|c| c.name.as_str())
                                .filter(|name| !ToolExecutor::is_pa_readonly_tool(name))
                                .collect();
                            let force_finish = if let Some(ref tc) = self.tool_controller {
                                let tc_calls: Vec<(String, Value)> = tool_calls.iter()
                                    .map(|c| (c.name.clone(), c.arguments.clone()))
                                    .collect();
                                tc.should_force_finish(&tc_calls, &agent.role)
                            } else {
                                !write_tools.is_empty()
                            };
                            if force_finish {
                                warn!("[PA Streaming] 写操作工具调用被阻止: {:?}", write_tools);
                                break;
                            }
                        }

                        let asst_summary = parsed.summary.clone()
                            .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                        running_messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: asst_summary,
                            name: None,
                            tool_calls: Some(
                                tool_calls
                                    .iter()
                                    .map(|c| crate::gateway::unified_gateway::ToolCallPayload {
                                        id: c.id.clone(),
                                        call_type: "function".to_string(),
                                        function: crate::gateway::unified_gateway::ToolCallFunction {
                                            name: c.name.clone(),
                                            arguments: serde_json::to_string(&c.arguments).unwrap_or_default(),
                                        },
                                    })
                                    .collect(),
                            ),
                            tool_call_id: None,
                            reasoning_content: stream_response.thought.clone(),
                        });

                        for c in tool_calls {
                            tc += 1;
                            let name = &c.name;
                            let args: Value = c.arguments.clone();

                            // SkillBefore hook
                            {
                                let mut hook_ctx = HookContext::new(
                                    HookPoint::SkillBefore,
                                    &agent.agent_id,
                                    &agent.role.to_string(),
                                )
                                .with_task(&ctx.task_iri, &ctx.task_iri)
                                .with_data("tool_name", Value::String(name.clone()));
                                self.hook_manager
                                    .execute(HookPoint::SkillBefore, &mut hook_ctx)
                                    .await;
                                // Capture ToolGuard pre-injections for next streaming turn
                                if let Some(injections) = hook_ctx.metadata.remove("guard_pre_injections") {
                                    if let Value::Array(arr) = injections {
                                        for v in arr {
                                            if let Some(s) = v.as_str() {
                                                guard_pending_pre_injections.push(s.to_string());
                                            }
                                        }
                                    }
                                }
                            }

                            let handler = {
                                let executor = self.tool_executor.read().unwrap_or_else(|e| {
                                    warn!("ToolExecutor 读锁中毒 (streaming handler): {}", e);
                                    e.into_inner()
                                });
                                executor.try_get_handler(name)
                            };
                            let result = match handler {
                                Some(f) => f(args).await.unwrap_or_else(|e| json!({"error": e})),
                                None => json!({"error": format!("Tool not found: {}", name)}),
                            };
                            let raw_result_str = serde_json::to_string(&result).unwrap_or_default();
                            let result_str = self.route_tool_result(&raw_result_str, name, &c.id).await;

                            // SkillAfter hook
                            let guard_aborted = {
                                let mut hook_ctx = HookContext::new(
                                    HookPoint::SkillAfter,
                                    &agent.agent_id,
                                    &agent.role.to_string(),
                                )
                                .with_task(&ctx.task_iri, &ctx.task_iri)
                                .with_data("tool_name", Value::String(name.clone()))
                                .with_data("tool_result", Value::String(raw_result_str.clone()));
                                let hook_result = self.hook_manager
                                    .execute(HookPoint::SkillAfter, &mut hook_ctx)
                                    .await;

                                if hook_result == HookResult::Abort {
                                    Some(hook_ctx.error.unwrap_or_else(|| "Tool result rejected by guard".to_string()))
                                } else {
                                    None
                                }
                            };

                            if let Some(guard_msg) = &guard_aborted {
                                warn!("[Streaming] {} ToolGuard 拦截: {}", name, guard_msg);
                            } else if let Some(err) = result.get("error") {
                                warn!("[Streaming] tool {} 失败: {}", name, err);
                                errs.push(format!("{}: {}", name, err));
                                if let Some(ref event_bus) = self.event_bus {
                                    let _ = event_bus.emit(&ctx.task_iri, "AGENT_ERROR", &agent.agent_id, &serde_json::json!({"error": err, "tool": name}).to_string()).await;
                                }
                            } else {
                                info!("[Streaming] tool {} 成功", name);
                            }

                            let tool_content = if let Some(guard_msg) = &guard_aborted {
                                format!("[ToolGuard 拦截] 工具 {} 的结果被安全系统拒绝。{}", name, guard_msg)
                            } else {
                                result_str
                            };
                            running_messages.push(ChatMessage {
                                role: "tool".to_string(),
                                content: tool_content,
                                name: None,
                                tool_calls: None,
                                tool_call_id: Some(c.id.clone()),
                                reasoning_content: None,
                            });
                        }

                        turn += 1;

                        // A1: 每 3 轮压缩 running_messages，避免上下文无限增长
                        if turn % 3 == 0 {
                            if let Some(ref cwm_lock) = self.context_window_manager {
                                if let Ok(cwm) = cwm_lock.lock() {
                                    if cwm.should_compress(running_messages.len(), &running_messages) {
                                        let (compressed, _summary) = cwm.compress_messages(&running_messages);
                                        let orig_count = running_messages.len();
                                        running_messages = compressed;
                                        debug!(
                                            "[Streaming] 上下文压缩: {} → {} 条",
                                            orig_count,
                                            running_messages.len()
                                        );
                                    }
                                }
                            }
                        }

                        if turn >= max_turns {
                            warn!("[Streaming] 达到最大工具调用轮次 {}", max_turns);
                            break;
                        }
                        continue;
                    }
                    break;
                }
                _ => {
                    last_content = parsed.content.clone();
                    last_thought = parsed.thought.clone().unwrap_or_default();
                    last_summary = parsed.summary.clone()
                        .unwrap_or_else(|| Self::generate_auto_summary(&parsed.content));
                    info!("AgentRunner Streaming 完成: role={}, tools={}, turn={}",
                        agent.role, tc, turn);
                    break;
                }
            }
        }

        let final_summary = if last_summary.is_empty() {
            Self::generate_auto_summary(&last_content)
        } else {
            last_summary.clone()
        };

        let l0_iri = session
            .archive_full_to_l0(
                &self.l0_store,
                &agent.role.to_string(),
                &last_thought,
                &last_content,
            )
            .ok();

        let l1_turn = session.add_summary(
            &agent.role.to_string(),
            &last_summary,
            l0_iri.clone(),
        );
        // 计算 turn embedding 和 relevance_score
        if let (Some(ref embedder), Some(ref tracker_lock)) = (&self.embedder, &self.relevance_tracker) {
            if let Ok(emb) = embedder.embed(&last_summary).await {
                let mut tracker = tracker_lock.lock().unwrap();
                let score = tracker.on_new_input(&emb);
                l1_turn.embedding = Some(emb);
                l1_turn.relevance_score = Some(score);
            }
        }

        let task_id = ctx.task_iri
            .strip_prefix("iri://task/")
            .unwrap_or_else(|| ctx.task_iri.strip_prefix("iri://").unwrap_or(&ctx.task_iri));
        let node_iri = format!("iri://task/{}/turn_{}", task_id, turn);
        let mut node_json = json!({
            "@id": &node_iri,
            "@type": "AgentTurn",
            "role": agent.role.to_string(),
            "content_len": last_content.len(),
        });
        if !last_thought.is_empty() {
            node_json["has_thought"] = Value::Bool(true);
            node_json["thought_len"] = Value::Number(last_thought.len().into());
        }

        let output_value = Value::String(last_content.clone());
        let jsonld_output = self.apply_output_mapping(&output_value, &agent.role, &ctx.task_iri);

        info!("AgentRunner Streaming 完成: {} tools", tc);

        (Ok(TaskResult {
            task_iri: ctx.task_iri,
            status: "success".to_string(),
            summary: final_summary,
            output: Some(output_value),
            jsonld_output,
            artifacts: vec![],
            errors: errs,
            turn_count: turn,
            tool_call_count: tc,
            five_w2h_updates: None,
                tracked_actions: Vec::new(),
            archive_iri: None,
        }), session)
    }

    /// 将微工具数据同时写入内存和 L0 持久化存储
    fn store_micro_tool_data_persistent(&self, storage_key: &str, data: serde_json::Value) {
        let mut exe = self.tool_executor.write().unwrap_or_else(|e| {
            warn!("ToolExecutor 写锁中毒 (store_micro_tool_data): {}", e);
            e.into_inner()
        });
        exe.store_micro_tool_data(storage_key, data.clone());
        // L0 持久化，保证跨会话可用
        if let Ok(data_str) = serde_json::to_string(&data) {
            let _ = self.l0_store.store(storage_key, &data_str);
        }
    }

    pub(super) async fn route_tool_result(
        &self,
        result_str: &str,
        tool_name: &str,
        call_id: &str,
    ) -> String {
        use crate::tools::result_router::router::ResultRouter;
        use crate::tools::result_router::summary;
        use crate::tools::result_router::RouteDecision;
        use crate::tools::result_router::graphify::GraphifyEngine;
        use crate::tools::result_router::micro_tools::MicroToolGenerator;
        use crate::tools::tool_executor::MicroToolContext;

        // Scheduling MCP tools whose complete results must never be
        // summarised/truncated — otherwise the DA sees a summary with
        // assignments=[] and passes that to save_assignments.
        const PASSTHROUGH_TOOLS: &[&str] = &[
            "solve_schedule",
            "save_assignments",
            "compute_eligibility",
            "get_tasks",
            "get_pins",
            "get_preferences",
            "get_schedule",
        ];

        let settings = crate::config::settings::ToolResultRouterSettings::default();
        let router = ResultRouter::new(&settings);

        let decision = if PASSTHROUGH_TOOLS.contains(&tool_name) {
            RouteDecision::PassThrough
        } else {
            router.route(result_str, tool_name, call_id)
        };
        let iri = format!("iri://tool-result/{}", call_id);

        match decision {
            RouteDecision::PassThrough => {
                // 小结果: 直通但附加 IRI 元信息
                format!("{}\nIRI: {}", result_str, iri)
            }

            RouteDecision::Truncate { max_chars } => {
                let truncated = if result_str.len() <= max_chars {
                    result_str.to_string()
                } else {
                    summary::smart_truncate(result_str, max_chars)
                };
                // 持久化完整结果到内存+L0
                self.store_micro_tool_data_persistent(&iri, serde_json::json!({
                    "content": result_str,
                    "tool_name": tool_name,
                }));
                let read_tool_name = format!("read_full_result_{}", call_id);
                let ctx = MicroToolContext {
                    call_id: call_id.to_string(),
                    storage_key: iri.clone(),
                    tool_name: tool_name.to_string(),
                    entity_types: vec![],
                    preview_size: settings.preview_size,
                };
                let mut exe = self.tool_executor.write().unwrap_or_else(|e| {
                    warn!("ToolExecutor 写锁中毒 (register_micro_tool trunc): {}", e);
                    e.into_inner()
                });
                exe.register_micro_tool(&read_tool_name, ctx);
                summary::format_iri_message(tool_name, call_id, &truncated, result_str.len())
            }

            RouteDecision::Graphify { call_id: g_call_id, graph_name } => {
                let parsed: Option<serde_json::Value> = serde_json::from_str(result_str.trim()).ok();
                match parsed {
                    Some(json_val) => {
                        self.store_micro_tool_data_persistent(&iri, json_val.clone());
                        let engine_result = match &self.unified_graph_store {
                            Some(store) => GraphifyEngine::with_shared_store(store.clone(), settings.max_graph_entities),
                            None => GraphifyEngine::new(settings.max_graph_entities),
                        };
                        match engine_result {
                            Ok(mut engine) => {
                                let graphify_result = engine.graphify_json(
                                    &json_val, &g_call_id, settings.max_graph_entities,
                                );
                                let analysis = crate::tools::result_router::SchemaAnalysis {
                                    entity_types: graphify_result.entity_types.iter().map(|t| (t.clone(), 0)).collect(),
                                    relation_types: vec![],
                                    property_names: vec![],
                                    total_entities: graphify_result.entity_count,
                                    total_relations: graphify_result.relation_count,
                                };
                                let micro_tools = MicroToolGenerator::generate_from_schema(
                                    &analysis, &g_call_id, settings.max_micro_tools,
                                );
                                for mt in &micro_tools {
                                    let ctx = MicroToolContext {
                                        call_id: g_call_id.clone(),
                                        storage_key: iri.clone(),
                                        tool_name: tool_name.to_string(),
                                        entity_types: vec![],
                                        preview_size: settings.preview_size,
                                    };
                                    let mut exe = self.tool_executor.write().unwrap_or_else(|e| {
                                        warn!("ToolExecutor 写锁中毒 (register_micro_tool graphify): {}", e);
                                        e.into_inner()
                                    });
                                    exe.register_micro_tool(&mt.name, ctx);
                                }
                                info!(
                                    "[ResultRouter] 图谱化: {} 个实体, {} 个关系, {} 个微工具, graph={}",
                                    graphify_result.entity_count, graphify_result.relation_count,
                                    micro_tools.len(), graph_name,
                                );
                                summary::format_iri_message(tool_name, call_id, &graphify_result.summary, result_str.len())
                            }
                            Err(e) => {
                                warn!("[ResultRouter] 图谱化失败: {}, 回退到 IRI 格式", e);
                                let truncated = summary::smart_truncate(result_str, settings.threshold_large);
                                summary::format_iri_message(tool_name, call_id, &truncated, result_str.len())
                            }
                        }
                    }
                    None => {
                        let text_summary = summary::generate_text_summary(result_str, tool_name, settings.preview_size);
                        summary::format_iri_message(tool_name, call_id, &text_summary, result_str.len())
                    }
                }
            }

            RouteDecision::Summarize { call_id: s_call_id, preview_size } => {
                self.store_micro_tool_data_persistent(&iri, serde_json::json!({
                    "content": result_str,
                    "tool_name": tool_name,
                }));

                let read_tool_name = format!("read_full_result_{}", s_call_id);
                let ctx = MicroToolContext {
                    call_id: s_call_id.to_string(),
                    storage_key: iri.clone(),
                    tool_name: tool_name.to_string(),
                    entity_types: vec![],
                    preview_size,
                };
                let mut exe = self.tool_executor.write().unwrap_or_else(|e| {
                    warn!("ToolExecutor 写锁中毒 (register_micro_tool summarize): {}", e);
                    e.into_inner()
                });
                exe.register_micro_tool(&read_tool_name, ctx);

                let preview = summary::generate_text_summary(result_str, tool_name, preview_size);
                info!(
                    "[ResultRouter] 摘要化: {} 字节 -> 预览 {} 字节, 微工具: {}, IRI: {}",
                    result_str.len(), preview_size, read_tool_name, iri,
                );
                summary::format_iri_message(tool_name, call_id, &preview, result_str.len())
            }
        }
    }
}
