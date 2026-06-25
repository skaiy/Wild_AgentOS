use std::collections::HashMap;
use tracing::{debug, warn};

use crate::core::agent_instance::{AgentInstance, AgentRole};
use crate::core::sa::PlanStep;
use crate::memory::l1_session::L1Session;

use super::{TaskContext, LLM_RESPONSE_FORMAT_NO_THOUGHT, LLM_RESPONSE_FORMAT_WITH_THOUGHT};

impl super::AgentRunner {
    pub(super) fn build_agent_md_from_step(
        &self,
        role: AgentRole,
        step: &PlanStep,
    ) -> String {
        let role_name = match role {
            AgentRole::Plan => "计划",
            AgentRole::Do => "执行",
            AgentRole::Check => "检查",
            AgentRole::Act => "决策",
        };

        let tools_list = if step.tools_allowed.is_empty() {
            self.tool_executor.read().unwrap_or_else(|e| {
                warn!("ToolExecutor 读锁中毒: {}", e);
                e.into_inner()
            }).list_tools(&role.to_string())
        } else {
            step.tools_allowed.clone()
        };

        let model = self.gateway.get_model(&role.to_string().to_lowercase());
        let supports_reasoning = self.gateway.supports_native_reasoning(&model);
        let format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT
        };

        format!(
            r#"# {} Agent

## 当前任务目标
{}

## 预期输出
{}

## 成功标准
{}

## 可用工具
{}

## 输出格式要求
{}

## 注意事项
- 这是一个独立的执行环境，请专注于完成当前任务
- 不要假设任务已完成，必须实际执行并验证
- 如果遇到问题，请详细说明原因
- 完成任务后给出简洁的 summary
"#,
            role_name,
            step.objective,
            step.expected_output,
            step.success_criteria,
            tools_list.join(", "),
            format_constraint
        )
    }

    pub(super) async fn create_session(&self, agent: &AgentInstance, ctx: &TaskContext) -> L1Session {
        self.memory_manager.lock().await.create_session(
            &agent.agent_id,
            &agent.role.to_string(),
            &ctx.task_iri,
        )
    }

    fn gather_context_data(&self, role: AgentRole, ctx: &TaskContext) -> HashMap<String, String> {
        let mut context_data = HashMap::new();

        if let Some(ref original_task) = ctx.original_task {
            context_data.insert("original_task".to_string(), original_task.clone());
        }

        if let Some(ref summary) = ctx.prev_agent_summary {
            match role {
                AgentRole::Do => {
                    context_data.insert("plan_content".to_string(), summary.clone());
                }
                AgentRole::Check => {
                    context_data.insert("execution_result".to_string(), summary.clone());
                }
                AgentRole::Act => {
                    context_data.insert("check_result".to_string(), summary.clone());
                }
                _ => {}
            }
        }

        if !ctx.completed_steps.is_empty() {
            context_data.insert("completed_steps".to_string(), ctx.completed_steps.join(", "));
        }
        if !ctx.pending_steps.is_empty() {
            context_data.insert("pending_steps".to_string(), ctx.pending_steps.join(", "));
        }

        for (k, v) in &ctx.constraints {
            context_data.insert(k.clone(), v.clone());
        }

        if let Some(ref snapshot) = ctx.five_w2h_snapshot {
            // 按角色注入 5W2H 数据，避免冗余
            // PA: what, why, success_criteria, deadline, env
            // DA: what, required_steps, forbidden_tools
            // CA: 完整 7 维度
            // AA: what + why（最小参考集）
            match role {
                AgentRole::Plan => {
                    context_data.insert("five_w2h_what".to_string(), snapshot.what.clone());
                    context_data.insert("five_w2h_why".to_string(), snapshot.why.description.clone());
                    if !snapshot.why.success_criteria.is_empty() {
                        context_data.insert("five_w2h_success_criteria".to_string(), snapshot.why.success_criteria.join(", "));
                    }
                    if let Some(ref when) = snapshot.when {
                        if let Some(ref deadline) = when.deadline {
                            context_data.insert("five_w2h_deadline".to_string(), deadline.to_rfc3339());
                        }
                    }
                    if let Some(ref where_) = snapshot.where_ {
                        if let Some(ref env) = where_.execution_environment {
                            context_data.insert("five_w2h_execution_env".to_string(), env.clone());
                        }
                    }
                }
                AgentRole::Do => {
                    context_data.insert("five_w2h_what".to_string(), snapshot.what.clone());
                    if let Some(ref how) = snapshot.how {
                        if let Some(ref steps) = how.required_steps {
                            context_data.insert("five_w2h_required_steps".to_string(), steps.clone());
                        }
                        if !how.forbidden_tools.is_empty() {
                            context_data.insert("five_w2h_forbidden_tools".to_string(), how.forbidden_tools.join(", "));
                        }
                    }
                }
                AgentRole::Check => {
                    context_data.insert("five_w2h_what".to_string(), snapshot.what.clone());
                    context_data.insert("five_w2h_why".to_string(), snapshot.why.description.clone());
                    if !snapshot.why.success_criteria.is_empty() {
                        context_data.insert("five_w2h_success_criteria".to_string(), snapshot.why.success_criteria.join(", "));
                    }
                    if let Some(ref when) = snapshot.when {
                        if let Some(ref deadline) = when.deadline {
                            context_data.insert("five_w2h_deadline".to_string(), deadline.to_rfc3339());
                        }
                    }
                    if let Some(ref where_) = snapshot.where_ {
                        if let Some(ref env) = where_.execution_environment {
                            context_data.insert("five_w2h_execution_env".to_string(), env.clone());
                        }
                    }
                    if let Some(ref how) = snapshot.how {
                        if let Some(ref steps) = how.required_steps {
                            context_data.insert("five_w2h_required_steps".to_string(), steps.clone());
                        }
                        if !how.forbidden_tools.is_empty() {
                            context_data.insert("five_w2h_forbidden_tools".to_string(), how.forbidden_tools.join(", "));
                        }
                    }
                    if let Some(ref how_much) = snapshot.how_much {
                        if let Some(budget) = how_much.token_budget {
                            context_data.insert("five_w2h_token_budget".to_string(), budget.to_string());
                        }
                        if let Some(cycles) = how_much.max_pdca_cycles {
                            context_data.insert("five_w2h_max_cycles".to_string(), cycles.to_string());
                        }
                    }
                }
                AgentRole::Act => {
                    context_data.insert("five_w2h_what".to_string(), snapshot.what.clone());
                    context_data.insert("five_w2h_why".to_string(), snapshot.why.description.clone());
                }
            }
        }

        context_data
    }

    pub(super) async fn gather_context_data_async(
        &self,
        role: AgentRole,
        ctx: &TaskContext,
    ) -> HashMap<String, String> {
        let mut context_data = self.gather_context_data(role, ctx);

        let frame_name = match role {
            AgentRole::Plan => "pa_init",
            AgentRole::Do => "da_input",
            AgentRole::Check => "ca_review",
            AgentRole::Act => "aa_decision",
        };

        if let Ok(projection_str) = self
            .projection
            .project(&ctx.task_iri, frame_name, HashMap::new())
            .await
        {
            if !projection_str.is_empty() {
                context_data.insert("context_summary".to_string(), projection_str);
            }
        }

        context_data
    }

    pub(super) fn build_agent_md(
        &self,
        role: AgentRole,
        objective: &str,
        context_data: &HashMap<String, String>,
        model: &str,
    ) -> String {
        let role_name = role.to_string();
        let role_lower = role_name.to_lowercase();
        let tools_list = self.tool_executor.read().unwrap_or_else(|e| {
            warn!("ToolExecutor 读锁中毒 (build_system_prompt): {}", e);
            e.into_inner()
        }).list_tools(&role_name);

        let supports_reasoning = self.gateway.supports_native_reasoning(model);
        let _format_constraint = if supports_reasoning {
            LLM_RESPONSE_FORMAT_NO_THOUGHT
        } else {
            LLM_RESPONSE_FORMAT_WITH_THOUGHT
        };

        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        vars.insert(
            "task_description".to_string(),
            serde_json::Value::String(objective.to_string()),
        );
        vars.insert(
            "available_skills".to_string(),
            serde_json::Value::String(tools_list.join(", ")),
        );
        vars.insert(
            "context_summary".to_string(),
            serde_json::Value::String(
                context_data
                    .get("context_summary")
                    .cloned()
                    .unwrap_or_default(),
            ),
        );
        vars.insert(
            "task_specific_constraints".to_string(),
            serde_json::Value::String(context_data.get("constraints").cloned().unwrap_or_default()),
        );
        vars.insert(
            "plan_content".to_string(),
            serde_json::Value::String(
                context_data
                    .get("plan_content")
                    .cloned()
                    .unwrap_or_else(|| "(由 SA 填写)".to_string()),
            ),
        );
        vars.insert(
            "execution_result".to_string(),
            serde_json::Value::String(
                context_data
                    .get("execution_result")
                    .cloned()
                    .unwrap_or_else(|| "(由 DA 生成)".to_string()),
            ),
        );
        vars.insert(
            "check_result".to_string(),
            serde_json::Value::String(
                context_data
                    .get("check_result")
                    .cloned()
                    .unwrap_or_else(|| "(由 CA 生成)".to_string()),
            ),
        );

        if let Some(ref loader) = self.prompt_loader {
            let result = loader.load(&role_lower, "skeleton", &vars);
            if !result.is_empty() {
                let md = format!("# {} Agent.md\n\n{}", role_name, result);
                debug!(role = %role_name, "=== agent.md (from PromptLoader) ===\n{}", md);
                return md;
            }
        }

        if let Ok(rendered) =
            self.templates
                .render_prompt(&role_lower, "skeleton", &vars, false, None)
        {
            let md = format!(
                "# {} Agent.md\n\n{}\n",
                role_name,
                rendered,
            );
            debug!(role = %role_name, supports_reasoning = supports_reasoning, "=== agent.md (from template) ===\n{}", md);
            return md;
        }

        let role_prompt = match role {
            AgentRole::Plan => {
                let w2h_what = context_data.get("five_w2h_what").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_why = context_data.get("five_w2h_why").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_success = context_data.get("five_w2h_success_criteria").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_deadline = context_data.get("five_w2h_deadline").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_env = context_data.get("five_w2h_execution_env").cloned().unwrap_or_else(|| "（未指定）".to_string());
                format!("你是计划Agent(PA)。你的职责是分析用户任务并制定执行计划。\n\n🔴 严格禁止：\n1. 禁止调用写操作工具（file_write, file_edit等）\n2. 禁止执行具体工作（创建文件、修改代码等）\n3. 禁止使用 bash 执行写操作（如写入文件、安装包、删除等）\n\n✅ 允许的操作：\n1. 可以调用只读工具收集信息（file_read, file_list, grep_search等）\n2. 可以使用 bash 执行只读命令（如 ls, cat, grep, find, which, pwd, echo等）来探索环境\n3. 分析用户任务需求\n4. 制定清晰的执行步骤\n5. 输出JSON格式的计划\n\n📋 任务元数据（5W2H — 必须参考）：\n- What: {}\n- Why: {}\n- 成功标准: {}\n- 截止时间: {}\n- 执行环境: {}\n\n请在上述元数据约束下制定计划。如发现需要补充的信息，请在计划中说明。\n\n规划完成后，建议回填 How 和 Where 维度（可选）：\n{{\"five_w2h_updates\": {{\"how\": {{\"planIRI\": \"计划IRI\", \"preferredSkills\": [...], \"requiredSteps\": \"...\"}}, \"where\": {{\"dataSources\": [...], \"executionEnvironment\": \"...\"}}}}}}", w2h_what, w2h_why, w2h_success, w2h_deadline, w2h_env)
            }
            AgentRole::Do => "你是执行Agent(DA)。你的职责是具体执行任务。\n\n🔴 严格禁止：\n1. 禁止在当前目录执行递归搜索（如 grep -r, find / 等），这会导致超时\n2. 禁止使用相对路径，必须使用任务中指定的绝对路径\n3. 禁止执行与任务无关的操作\n\n✅ 执行要求：\n1. 严格按照任务中指定的路径创建/修改文件\n2. 如果任务要求创建目录，先创建目录再创建文件\n3. 每一步操作都要验证结果\n4. 完成任务后立即调用 finish 结束\n5. 对于需要最新信息的研究任务，优先使用 web_search 搜索获取资料。如果多次尝试后网络工具仍失败，再基于自身知识回答\n\n📋 输出管理规范（必须遵守）：\n1. 执行可能返回大量输出的命令时（ls, find, grep, cat 大文件等），必须使用 | head -N 限制输出行数\n2. 优先使用精确搜索（grep + 路径限制、glob 过滤），避免扫描整个目录\n3. 只需确认命令结果时，使用 | grep 关键字 或 | tail 过滤关键信息，不要看全部输出\n4. 系统对超过 16KB 的输出会自动截断，且超过 2KB 的结果会被摘要化——主动控制输出量以避免信息丢失\n5. 如果发现工具返回的结果显示「output truncated」或「已存档」标记，说明输出过大，应使用更精确的命令重新运行\n\n示例流程：\n1. 任务要求创建 /tmp/test/file.txt → 先用 Bash 创建目录，再用 file_write 写入\n2. 任务要求修改文件 → 用 file_read 读取，处理后用 file_write 写入\n3. 任务要求验证 → 用 file_read 读取并检查内容\n4. 搜索工具失败 → 尝试1次后如仍失败，基于自身知识回答".to_string(),
            AgentRole::Check => {
                let w2h_what = context_data.get("five_w2h_what").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_why = context_data.get("five_w2h_why").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_deadline = context_data.get("five_w2h_deadline").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_env = context_data.get("five_w2h_execution_env").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_steps = context_data.get("five_w2h_required_steps").cloned().unwrap_or_else(|| "（未指定）".to_string());
                let w2h_budget = context_data.get("five_w2h_token_budget").cloned().unwrap_or_else(|| "（未指定）".to_string());
                format!("你是检查Agent(CA)。你的职责是审查执行结果，确保任务目标达成。\n\n🔴 严格禁止：\n1. 禁止检查或报告任何与当前任务无关的文件/目录——即使在工作区中发现了其他项目，也必须忽略\n2. 禁止在审计报告中写入无关内容——报告必须仅聚焦于当前任务目标\n3. 禁止探索不属于当前任务的目录\n\n✅ 检查范围限制：\n1. 仅检查当前任务明确要求创建或修改的文件\n2. 如果发现 DA 创建了预期之外的文件，仅在它们与任务相关时才需要检查\n3. 工作区中的其他项目/目录（如之前的测试产出）与任务无关，必须忽略\n\n📋 推荐审计参考（5W2H 维度 — 必须关注的重要维度之一）：\n- What: {} — 任务目标是否达成？\n- Why: {} — 是否满足原始意图？\n- When: {} — 截止时间是否满足？\n- Where: {} — 是否在正确环境操作？\n- How: {} — 步骤是否按计划执行？\n- HowMuch: {} — 资源是否超支？\n\n注意：5W2H 是重要的分析维度之一，你可以根据任务特性增加其他审计视角（如安全性、可维护性、性能等）。\n\n📋 输出格式：\n请输出结构化的审计结果，包含：\n1. 各审计视角的检查结论（PASS/FAIL/CONDITIONAL + 证据）\n2. 总体结论（PASS/CONDITIONAL_PASS/FAIL）\n3. 发现的问题及建议", w2h_what, w2h_why, w2h_deadline, w2h_env, w2h_steps, w2h_budget)
            }
            AgentRole::Act => "你是决策Agent(AA)，不是执行Agent。你的唯一职责是基于 CA 的审计结果做决策，并给出处置建议。\n\n🔴 严格禁止（必须遵守）：\n1. 禁止调用 glob_search、file_list、file_read、grep_search 等文件探索工具——你的输入仅来自 CA 审计结果和任务上下文\n2. 禁止执行 bash 命令\n3. 禁止主动收集额外信息——你已经是最终决策层，不需要也不应该自行探索文件\n4. 禁止处理 CA 审计结果中提到的任何与当前任务无关的文件/目录\n\n✅ 允许的操作：\n1. 仅基于 CA 审计结果和任务上下文做决策\n2. 输出决策结论（任务状态 + 处置建议 + 最终总结）\n\n📋 决策参考：\n- CA 审计结论\n- 任务约束（5W2H 维度：What/Why/When/Where/How/HowMuch）\n- 任务实际情况\n\n📋 常见决策路径（仅供参考）：\n- 审计全部通过 → 归档任务，沉淀经验\n- 目标/意图未达成 → 建议回退重新分析或修正计划\n- 执行方式/环境问题 → 建议修正计划\n- 时间/资源超支 → 评估原因合理性后决定放行或降级\n\n📋 输出格式：\n1. 任务状态：完成 / 部分完成 / 未完成\n2. 处置建议：具体行动建议\n3. 最终结论：简洁总结".to_string(),
        };

        let context_section = if context_data.is_empty() {
            String::new()
        } else {
            let mut sections = Vec::new();
            if let Some(original) = context_data.get("original_task") {
                sections.push(format!("## 原始任务要求\n{}\n\n⚠️ 重要：你必须验证上述所有要求是否都已完成。", original));
            }
            if let Some(plan) = context_data.get("plan_content") {
                sections.push(format!("## 上级计划\n{}", plan));
            }
            if let Some(result) = context_data.get("execution_result") {
                sections.push(format!("## 执行结果\n{}", result));
            }
            if let Some(check) = context_data.get("check_result") {
                sections.push(format!("## 检查结论\n{}", check));
            }
            if let Some(ctx_summary) = context_data.get("context_summary") {
                sections.push(format!("## 相关上下文\n{}", ctx_summary));
            }
            if let Some(completed) = context_data.get("completed_steps") {
                sections.push(format!("## 已完成步骤\n{}", completed));
            }
            if let Some(pending) = context_data.get("pending_steps") {
                sections.push(format!("## 待完成步骤\n{}", pending));
            }
            let has_w2h = context_data.contains_key("five_w2h_what");
            if has_w2h {
                let mut w2h_lines = Vec::new();
                if let Some(v) = context_data.get("five_w2h_what") {
                    w2h_lines.push(format!("- What: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_why") {
                    w2h_lines.push(format!("- Why: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_success_criteria") {
                    w2h_lines.push(format!("- 成功标准: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_deadline") {
                    w2h_lines.push(format!("- 截止时间: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_execution_env") {
                    w2h_lines.push(format!("- 执行环境: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_required_steps") {
                    w2h_lines.push(format!("- 要求步骤: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_forbidden_tools") {
                    w2h_lines.push(format!("- 禁用工具: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_token_budget") {
                    w2h_lines.push(format!("- Token 预算: {}", v));
                }
                if let Some(v) = context_data.get("five_w2h_max_cycles") {
                    w2h_lines.push(format!("- 最大循环: {}", v));
                }
                if !w2h_lines.is_empty() {
                    sections.push(format!("## 任务元数据（5W2H）\n{}", w2h_lines.join("\n")));
                }
            }
            sections.join("\n\n")
        };

        let md = format!(
            "# {} Agent.md\n\n角色: {}\n任务: {}\n工作方式: {}\n\n{}\n\n重要：完成你的职责后，直接输出最终结果，不要再调用工具。你的回复应包含完整的结论或结果。",
            role_name, role_name, objective, role_prompt, context_section
        );
        debug!(role = %role_name, "=== agent.md (fallback) ===\n{}", md);
        md
    }

    pub(super) fn build_readable_tool_menu(&self, role: &AgentRole) -> String {
        let role_str = role.to_string();
        let tool_defs = self.tool_executor.read().unwrap_or_else(|e| {
            warn!("ToolExecutor 读锁中毒 (build_readable_tool_menu): {}", e);
            e.into_inner()
        }).tool_definitions_for_role(&role_str);

        if tool_defs.is_empty() {
            return String::new();
        }

        let os_hint = if cfg!(target_os = "windows") {
            "【系统平台: Windows | bash 工具实际使用 PowerShell 执行】"
        } else if cfg!(target_os = "macos") {
            "【系统平台: macOS】"
        } else {
            "【系统平台: Linux】"
        };
        let mut lines = vec![os_hint.to_string(), "可用工具列表：".to_string()];
        for tool_def in &tool_defs {
            let name = tool_def["function"]["name"].as_str().unwrap_or("");
            let desc = tool_def["function"]["description"].as_str().unwrap_or("");
            if desc.is_empty() {
                lines.push(format!("- ID: {}", name));
            } else {
                lines.push(format!("- ID: {} | 用途: {}", name, desc));
            }
        }
        lines.join("\n")
    }
}
