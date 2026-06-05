use std::collections::HashMap;



#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemPromptRegion {
    RoleDefinition,
    BehavioralPolicy,
    FiveW2HConstraints,
    EmphasizedConstraints,
    OutputFormat,
    OutputManagement,
    Tools,
    ExtractionPrompt,
}

impl SystemPromptRegion {
    pub fn order(&self) -> usize {
        match self {
            Self::RoleDefinition => 1,
            Self::BehavioralPolicy => 2,
            Self::FiveW2HConstraints => 3,
            Self::EmphasizedConstraints => 4,
            Self::OutputFormat => 5,
            Self::OutputManagement => 6,
            Self::Tools => 7,
            Self::ExtractionPrompt => 8,
        }
    }

    pub fn header(&self) -> &'static str {
        match self {
            Self::RoleDefinition => "# 角色",
            Self::BehavioralPolicy => "# 行为准则",
            Self::FiveW2HConstraints => "# 任务约束",
            Self::EmphasizedConstraints => "# 重要约束",
            Self::OutputFormat => "# 输出格式",
            Self::OutputManagement => "# 输出管理",
            Self::Tools => "# 工具",
            Self::ExtractionPrompt => "# 强调内容",
        }
    }
}

pub const OUTPUT_FORMAT_SIMPLE: &str = r#"返回 JSON: {"content": "...", "summary": "...", "action": "tool_call|finish"}
- summary: ≤50字摘要
- action: tool_call(调用工具) 或 finish(任务完成)"#;

pub const OUTPUT_FORMAT_FULL: &str = r#"返回 JSON: {"content": "...", "summary": "...", "action": "tool_call|finish|continue", "emphasis": []}
- content: 回复内容
- summary: ≤50字摘要
- action: tool_call(调用工具) / finish(任务完成) / continue(继续思考)
- emphasis: 识别的重要约束（数组）"#;

/// Instructions for output management — injected into system prompt between OutputFormat and Tools regions.
/// Tells the LLM how to handle large command output proactively.
pub const OUTPUT_MANAGEMENT: &str = r#"📋 输出管理 — 所有工具（尤其是 bash）必须遵守：

1. **大输出必须过滤**：可能返回 >100 行的命令，必须加 | head -N 或 | grep 关键字 限制输出量
2. **精确搜索优先**：grep / find 等必须指定路径范围，不得扫描整个工作区
3. **按需确认**：只需确认结果是否存在时，使用 | grep -c 或 | wc -l 而非查看全部内容
4. **截断感知**：超过 16KB 的输出会被静默截断，超过 2KB 的结果会被摘要化并附带 IRI 存档
   - 若看到「output truncated」标记或「[已存档]」标签 → 说明输出太大，请缩小范围重新搜索
   - 如需查看完整结果，可使用 read_full_result_* 工具按需读取"#;

/// Layer 1: 通用行为准则 — 适用于所有 PA/DA/CA/AA Agent
pub const UNIVERSAL_BEHAVIORAL_POLICY: &str = r#"🧠 通用行为准则（所有 Agent 必须遵守）

【感知原则】
1. 全量阅读 — 涉及文件/文档决策时，必须完整阅读后再判断，禁止仅凭文件名或片段推测
2. 索引优先 — 大量文件时先用搜索工具获取索引/概览，再按需精确读取，禁止盲目遍历
3. 实时确认 — 时间敏感信息（当前时间、实时状态、最新数据）必须使用实时查询工具，禁止使用内部知识猜测
4. 歧义澄清 — 需求/上下文模糊时必须主动追问或查阅权威定义，禁止自行推理假设

【验证原则】
1. 自动验证优先 — 完成可自动验证的任务后，立即使用 linter/测试/dry-run 等工具检查并通过
2. 根因分析 — 执行失败或验证不通过时，先分析日志和错误码定位根因再修复，禁止盲目重试
3. 回归验证 — 修复缺陷后必须重新运行相关验证，确保不引入新问题

【边界原则】
1. 最小权限 — 工具调用和数据访问严格限制在任务所需的最小范围，禁止访问无关资源
2. 风险预警 — 执行有副作用操作前，评估并明确告知潜在风险（修改公共 API、变更数据、消耗大量资源等）
3. 边界拒绝 — 涉及非法/不安全/不道德内容，或超出自身能力范围时明确拒绝并说明原因
4. 任务范围坚守 — 发现任务规模超出当前资源/能力时，主动建议缩小范围或分阶段执行，不在不可持续条件下硬撑"#;

/// Layer 2: 计划 Agent (PA) 附加准则
pub const PA_BEHAVIORAL_ADDENDUM: &str = r#"

【计划 Agent 附加准则】
1. 字面证据 — 任何结论/判断必须直接引用可追溯的字面来源（文档、代码、对话记录），禁止以「我觉得」「通常如此」为依据
2. 既有规则优先 — 用户指令/项目规则与自身知识冲突时，严格遵循现有规则；如有更好方案，先指出现有规则再提建议，获得确认后方可偏离
3. 最小假设 — 推理必须基于已知事实。必要假设必须声明为「假设」并说明假设不成立时的兜底方案
4. 成本意识 — 多个可行方案中选择整体成本最低者（Token、时间、计算资源）
5. 内在品质 — 计划必须经过自检确认无缺陷后才能交付，禁止将已知缺陷传递到执行阶段"#;

/// Layer 2: 执行 Agent (DA) 附加准则
pub const DA_BEHAVIORAL_ADDENDUM: &str = r#"

【执行 Agent 附加准则】
1. 读前修改 — 修改任何现有文件前，必须先读取当前内容，了解当前状态后再修改。禁止不知当前状态就覆盖写入
2. 唯一复用 — 创建新文件/函数/模块前，先搜索系统是否存在可复用的现有资源。存在时优先扩展复用而非新建。新产物的命名和结构必须与现有风格一致
3. 原子输出 — 每次工具调用完成一个具体目标，每个代码修改对应一个具体问题，禁止一个操作嵌入多个不相关目标
4. 自文档化 — 输出必须包含足够的注释、参数说明或辅助信息，使其他 Agent 或人能独立理解其目的和逻辑，无需翻阅完整对话历史
5. 安全边际 — 高风险操作（删除、配置变更、批量数据操作）偏向保守，优先模拟/验证/获取用户确认
6. 成本意识 — 大输出必须过滤，精确搜索替代全量扫描，自觉控制 Token 和计算资源消耗"#;

/// Layer 2: 检查 Agent (CA) 附加准则
pub const CA_BEHAVIORAL_ADDENDUM: &str = r#"

【检查 Agent 附加准则】
1. 关键点审查 — 对无法完全自动验证的关键输出（如需求分析），逐项对照原始需求进行审查，主动提交用户确认
2. 字面证据 — 审查结论必须直接引用可验证的来源（文件内容、执行日志、代码行等），禁止凭印象或推测判断
3. 既有规则优先 — 按项目标准（Agent.md、Rules、Specs）进行审查，而不是按自己的通用标准
4. PDCA 闭环 — 发现偏差时立即记录发现的问题，给出具体的纠正建议，建议回退/修正/重新执行的具体路径"#;

/// Layer 2: 决策 Agent (AA) 附加准则
pub const AA_BEHAVIORAL_ADDENDUM: &str = r#"

【决策 Agent 附加准则】
1. 字面证据 — 决策必须基于 CA 审计证据和任务约束，禁止主观臆断或猜测
2. 安全边际 — 高风险决策偏向保守路径，选择更安全的处置方案
3. 成本意识 — 评估继续执行/回退修正/降级交付/终止任务各路径的 Token、时间和计算成本
4. 建议执行分离 — 当被问到「怎么做」时，先给出分析、建议和选项，未经明确授权不得直接执行"#;

pub fn build_five_w2h_section(snapshot: &crate::core::five_w2h::Task5W2H) -> String {
    let mut lines = Vec::new();
    
    lines.push(format!("- 目标: {}", snapshot.what));
    
    lines.push(format!("- 原因: {}", snapshot.why.description));
    if !snapshot.why.success_criteria.is_empty() {
        lines.push(format!("- 成功标准: {}", snapshot.why.success_criteria.join(", ")));
    }
    
    if let Some(ref who) = snapshot.who {
        if let Some(ref requestor) = who.requestor {
            lines.push(format!("- 请求者: {}", requestor));
        }
        if let Some(ref access_level) = who.access_level {
            lines.push(format!("- 访问级别: {:?}", access_level));
        }
        if !who.assignees.is_empty() {
            lines.push(format!("- 执行者: {}", who.assignees.join(", ")));
        }
    }
    
    if let Some(ref when) = snapshot.when {
        if let Some(ref deadline) = when.deadline {
            lines.push(format!("- 截止时间: {}", deadline));
        }
    }
    
    if let Some(ref where_) = snapshot.where_ {
        if let Some(ref env) = where_.execution_environment {
            lines.push(format!("- 执行环境: {}", env));
        }
        if !where_.data_sources.is_empty() {
            lines.push(format!("- 数据源: {}", where_.data_sources.join(", ")));
        }
    }
    
    if let Some(ref how) = snapshot.how {
        if !how.forbidden_tools.is_empty() {
            lines.push(format!("- 禁用工具: {}", how.forbidden_tools.join(", ")));
        }
        if let Some(ref steps) = how.required_steps {
            lines.push(format!("- 要求步骤: {}", steps));
        }
        if !how.preferred_skills.is_empty() {
            lines.push(format!("- 所需技能: {}", how.preferred_skills.join(", ")));
        }
    }
    
    if let Some(ref how_much) = snapshot.how_much {
        if let Some(ref budget) = how_much.token_budget {
            lines.push(format!("- Token预算: {}", budget));
        }
        if let Some(ref cycles) = how_much.max_pdca_cycles {
            lines.push(format!("- 最大循环: {}", cycles));
        }
    }
    
    lines.join("\n")
}

pub struct ToolRegionContent {
    pub builtin_tools: String,
    pub dynamic_tools: String,
}

impl ToolRegionContent {
    pub fn new() -> Self {
        Self {
            builtin_tools: String::new(),
            dynamic_tools: String::new(),
        }
    }

    pub fn with_builtin(mut self, tools: &str) -> Self {
        self.builtin_tools = tools.to_string();
        self
    }

    pub fn with_dynamic(mut self, tools: &str) -> Self {
        self.dynamic_tools = tools.to_string();
        self
    }

    pub fn build(&self) -> String {
        let mut parts = Vec::new();
        
        if !self.builtin_tools.is_empty() {
            parts.push(format!("## 内置工具（固定）\n{}", self.builtin_tools));
        }
        
        if !self.dynamic_tools.is_empty() {
            parts.push(format!("## 动态工具（按需调整）\n{}", self.dynamic_tools));
        }
        
        parts.join("\n\n")
    }
}

impl Default for ToolRegionContent {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SystemPromptBuilder {
    regions: HashMap<SystemPromptRegion, String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
        }
    }

    pub fn set_region(&mut self, region: SystemPromptRegion, content: String) {
        self.regions.insert(region, content);
    }

    pub fn get_region(&self, region: &SystemPromptRegion) -> Option<&String> {
        self.regions.get(region)
    }

    pub fn clear_region(&mut self, region: &SystemPromptRegion) {
        self.regions.remove(region);
    }

    pub fn build(&self) -> String {
        let mut ordered_regions: Vec<(&SystemPromptRegion, &String)> = 
            self.regions.iter().collect();
        ordered_regions.sort_by_key(|(r, _)| r.order());

        let mut parts = Vec::new();
        for (region, content) in ordered_regions {
            if !content.is_empty() {
                parts.push(format!("{}\n\n{}", region.header(), content));
            }
        }
        parts.join("\n\n---\n\n")
    }

    pub fn build_with_emphasis(&self, emphasis_items: &[String]) -> String {
        let mut builder = self.clone();
        
        if !emphasis_items.is_empty() {
            let emphasis_content = emphasis_items
                .iter()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            builder.set_region(SystemPromptRegion::EmphasizedConstraints, emphasis_content);
        }
        
        builder.build()
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SystemPromptBuilder {
    fn clone(&self) -> Self {
        Self {
            regions: self.regions.clone(),
        }
    }
}

/// Build a constitution prompt text for a given agent role using the ConstitutionRegistry.
///
/// Produces the same format as the existing string constants (UNIVERSAL_BEHAVIORAL_POLICY
/// + role addendum), but driven by the structured registry for queryability.
///
/// Use this in agent_runner.rs to replace direct string concatenation.
pub fn build_constitution_prompt(role: crate::core::agent_instance::AgentRole) -> String {
    use crate::core::constitution::ConstitutionRole;
    let registry = crate::core::constitution::ConstitutionRegistry::new();
    let constitution_role = match role {
        crate::core::agent_instance::AgentRole::Plan => ConstitutionRole::Plan,
        crate::core::agent_instance::AgentRole::Do => ConstitutionRole::Do,
        crate::core::agent_instance::AgentRole::Check => ConstitutionRole::Check,
        crate::core::agent_instance::AgentRole::Act => ConstitutionRole::Act,
    };
    registry.build_prompt_for_role(constitution_role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_order() {
        assert!(SystemPromptRegion::RoleDefinition.order() < SystemPromptRegion::BehavioralPolicy.order());
        assert!(SystemPromptRegion::BehavioralPolicy.order() < SystemPromptRegion::FiveW2HConstraints.order());
        assert!(SystemPromptRegion::FiveW2HConstraints.order() < SystemPromptRegion::EmphasizedConstraints.order());
        assert!(SystemPromptRegion::EmphasizedConstraints.order() < SystemPromptRegion::OutputFormat.order());
        assert!(SystemPromptRegion::OutputFormat.order() < SystemPromptRegion::OutputManagement.order());
        assert!(SystemPromptRegion::OutputManagement.order() < SystemPromptRegion::Tools.order());
        assert!(SystemPromptRegion::Tools.order() < SystemPromptRegion::ExtractionPrompt.order());
    }

    #[test]
    fn test_build_system_prompt() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是计划Agent".to_string());
        builder.set_region(SystemPromptRegion::OutputFormat, "输出JSON格式".to_string());
        
        let result = builder.build();
        assert!(result.contains("# 角色"));
        assert!(result.contains("# 输出格式"));
        assert!(result.contains("你是计划Agent"));
    }

    #[test]
    fn test_build_with_emphasis() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是计划Agent".to_string());
        
        let emphasis = vec!["必须使用异步方式".to_string(), "注意错误处理".to_string()];
        let result = builder.build_with_emphasis(&emphasis);
        
        assert!(result.contains("重要约束"));
        assert!(result.contains("必须使用异步方式"));
        assert!(result.contains("注意错误处理"));
    }

    #[test]
    fn test_tool_region_content() {
        let tool_content = ToolRegionContent::new()
            .with_builtin("file_read: 读取文件\nfile_write: 写入文件")
            .with_dynamic("http_request: HTTP请求\ncode_execute: 执行代码");
        
        let result = tool_content.build();
        assert!(result.contains("内置工具（固定）"));
        assert!(result.contains("动态工具（按需调整）"));
        assert!(result.contains("file_read"));
        assert!(result.contains("http_request"));
    }

    #[test]
    fn test_build_with_tools() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是执行Agent".to_string());
        
        let tool_content = ToolRegionContent::new()
            .with_builtin("file_read: 读取文件")
            .with_dynamic("custom_tool: 自定义工具")
            .build();
        builder.set_region(SystemPromptRegion::Tools, tool_content);
        
        let result = builder.build();
        assert!(result.contains("# 工具"));
        assert!(result.contains("内置工具（固定）"));
        assert!(result.contains("动态工具（按需调整）"));
    }
}
