/// Behavioral Constitution Registry
///
/// Structured registry of all behavioral rules that govern agent behavior.
/// Replaces raw string constants with queryable entries, enabling:
/// - Role-based rule filtering (Universal / PA / DA / CA / AA / SA)
/// - Category-based grouping (Perception / Verification / Boundary / etc.)
/// - Methodology binding (each rule maps to enforcement methodology + trigger)
/// - Prompt text generation (build_prompt_for_role matches existing format)
///
/// Architecture Layer: L3 — Constitution (Always-On)
/// See design: PR-res/superpowers-skills-full-integration-design.md §1

use std::collections::HashMap;

// ════════════════════════════════════════════════════════════════════════
// Category & Role Types
// ════════════════════════════════════════════════════════════════════════

/// Categories of behavioral rules — maps to the 3 universal dimensions + role-specific groups
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstitutionCategory {
    /// 感知原则 — Information Acquisition & Task Understanding
    Perception,
    /// 验证原则 — Verification, Validation & Root Cause
    Verification,
    /// 边界原则 — Security, Ethics & Role Limitations
    Boundary,
    /// SA: 感知与理解 — Understanding & Clarification (SA-specific)
    Understanding,
    /// SA: 决策与规划 — Decision & Planning (SA-specific)
    Decision,
    /// SA: 交互与安全 — Interaction & Safety (SA-specific)
    Safety,
    /// PA: 计划附加准则
    PlanAddendum,
    /// DA: 执行附加准则
    ExecAddendum,
    /// CA: 检查附加准则
    CheckAddendum,
    /// AA: 决策附加准则
    ActAddendum,
}

impl ConstitutionCategory {
    pub fn header(&self) -> &'static str {
        match self {
            Self::Perception => "【感知原则】",
            Self::Verification => "【验证原则】",
            Self::Boundary => "【边界原则】",
            Self::Understanding => "【感知与理解】",
            Self::Decision => "【决策与规划】",
            Self::Safety => "【交互与安全】",
            Self::PlanAddendum => "【计划 Agent 附加准则】",
            Self::ExecAddendum => "【执行 Agent 附加准则】",
            Self::CheckAddendum => "【检查 Agent 附加准则】",
            Self::ActAddendum => "【决策 Agent 附加准则】",
        }
    }
}

/// Roles that a constitution rule applies to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstitutionRole {
    /// Applies to all agents (PA/DA/CA/AA)
    Universal,
    /// Supervisor Agent
    Supervisor,
    /// Plan Agent
    Plan,
    /// Do Agent
    Do,
    /// Check Agent
    Check,
    /// Act Agent
    Act,
}

// ════════════════════════════════════════════════════════════════════════
// Methodology Binding
// ════════════════════════════════════════════════════════════════════════

/// Trigger condition for activating a methodology
#[derive(Debug, Clone)]
pub enum TriggerCondition {
    /// Always active when the constitution rule is in scope
    Always,
    /// Activates when a tool in the listed categories is used
    OnToolCategory(Vec<&'static str>),
    /// Activates on a specific hook point
    OnHookPoint(&'static str),
    /// Activates at end of a PDCA phase
    OnPhaseEnd(&'static str),
    /// Activates on task error
    OnTaskError,
    /// Activates for a specific agent role
    OnAgentRole(ConstitutionRole),
}

/// A binding from a constitution rule to a methodology
#[derive(Debug, Clone)]
pub struct MethodologyBinding {
    /// Methodology ID (e.g., "methodology:index-priority")
    pub methodology_id: &'static str,
    /// The enforcement mechanism that implements this binding
    pub enforcement: &'static str,
    /// When this methodology should be activated
    pub trigger: TriggerCondition,
}

// ════════════════════════════════════════════════════════════════════════
// Constitution Entry
// ════════════════════════════════════════════════════════════════════════

/// A single behavioral rule in the constitution registry
#[derive(Debug, Clone)]
pub struct ConstitutionEntry {
    /// Unique rule ID like "uni-perception-1", "sa-decision-3", "da-2"
    pub id: &'static str,
    /// Category grouping
    pub category: ConstitutionCategory,
    /// Which role(s) this applies to
    pub role: ConstitutionRole,
    /// Full principle text (as displayed in system prompt)
    pub principle: &'static str,
    /// Short label (≤8 chars) for tooltip/compact display
    pub label: &'static str,
    /// Bindings to methodologies + enforcement mechanisms
    pub bindings: Vec<MethodologyBinding>,
}

impl ConstitutionEntry {
    pub const fn new(
        id: &'static str,
        category: ConstitutionCategory,
        role: ConstitutionRole,
        principle: &'static str,
        label: &'static str,
    ) -> Self {
        Self { id, category, role, principle, label, bindings: Vec::new() }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Lazy-Static Methodology Bindings
// ════════════════════════════════════════════════════════════════════════

/// Returns the full constitution→methodology binding table.
/// Separated from entry definitions because `Vec` prevents `const fn`.
pub fn default_bindings() -> HashMap<&'static str, Vec<MethodologyBinding>> {
    let mut m = HashMap::new();

    // ── Universal: Perception ──
    m.insert("uni-perception-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:using-superpowers",
            enforcement: "ToolGuard::PreInjection(file_read tools) — 红线表注入",
            trigger: TriggerCondition::OnToolCategory(vec!["file_read", "file_search"]),
        },
    ]);
    m.insert("uni-perception-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:index-priority",
            enforcement: "ToolGuard::PreInjection(search_before_traverse) — 优先搜索后读取",
            trigger: TriggerCondition::OnToolCategory(vec!["glob", "find", "search"]),
        },
    ]);
    m.insert("uni-perception-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:index-priority",
            enforcement: "SyscallGate::validate_source(time_sensitive → realtime tool)",
            trigger: TriggerCondition::OnHookPoint("PreToolCall"),
        },
    ]);
    m.insert("uni-perception-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:brainstorming",
            enforcement: "SA::clarify() → StageGate::Plan→Do pre-check",
            trigger: TriggerCondition::OnPhaseEnd("PLAN"),
        },
    ]);

    // ── Universal: Verification ──
    m.insert("uni-verification-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:verification-before-completion",
            enforcement: "StageGate::Act→Archive + ToolGuard::PostValidation",
            trigger: TriggerCondition::OnPhaseEnd("ACT"),
        },
    ]);
    m.insert("uni-verification-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:systematic-debugging",
            enforcement: "RootCauseEngine::trace() — 5级回溯 + 证据链",
            trigger: TriggerCondition::OnTaskError,
        },
    ]);
    m.insert("uni-verification-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:verification-before-completion",
            enforcement: "ToolGuard::PostValidation — 修复后重跑验证",
            trigger: TriggerCondition::OnHookPoint("PostToolCall"),
        },
    ]);

    // ── Universal: Boundary ──
    m.insert("uni-boundary-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:least-privilege",
            enforcement: "SyscallGate::WhitelistManager — 工具白名单限制",
            trigger: TriggerCondition::OnToolCategory(vec!["shell", "network", "file_write"]),
        },
    ]);
    m.insert("uni-boundary-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:boundary-enforcement",
            enforcement: "StageGate::HardBlock — 副作用前风险评估",
            trigger: TriggerCondition::OnHookPoint("PreDestructiveAction"),
        },
    ]);
    m.insert("uni-boundary-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:boundary-enforcement",
            enforcement: "SyscallGate::Abort — 非法请求拒绝",
            trigger: TriggerCondition::Always,
        },
    ]);
    m.insert("uni-boundary-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:complexity-assessment",
            enforcement: "StageGate::ScopeCheck — 任务规模超限建议",
            trigger: TriggerCondition::OnPhaseEnd("PLAN"),
        },
    ]);

    // ── PA ──
    m.insert("pa-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:writing-plans",
            enforcement: "PlanStep 粒度 — 每步引用来源",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Plan),
        },
    ]);
    m.insert("pa-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:using-superpowers",
            enforcement: "SyscallGate::RulePriorityCheck",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Plan),
        },
    ]);
    m.insert("pa-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:systematic-debugging",
            enforcement: "假设声明模板注入",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Plan),
        },
    ]);
    m.insert("pa-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:cost-awareness",
            enforcement: "TokenBudget + TimeEstimator",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Plan),
        },
    ]);
    m.insert("pa-5", vec![
        MethodologyBinding {
            methodology_id: "methodology:verification-before-completion",
            enforcement: "PA 自检清单",
            trigger: TriggerCondition::OnPhaseEnd("PLAN"),
        },
    ]);

    // ── DA ──
    m.insert("da-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:test-driven-development",
            enforcement: "ToolGuard(Write前强制Read)",
            trigger: TriggerCondition::OnToolCategory(vec!["file_write"]),
        },
    ]);
    m.insert("da-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:executing-plans",
            enforcement: "SkillRegistry 前置搜索",
            trigger: TriggerCondition::OnToolCategory(vec!["file_create"]),
        },
    ]);
    m.insert("da-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:dispatching-parallel-agents",
            enforcement: "ToolGuard 原子性校验",
            trigger: TriggerCondition::Always,
        },
    ]);
    m.insert("da-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:subagent-driven-development",
            enforcement: "输出模板 + 注释规则",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Do),
        },
    ]);
    m.insert("da-5", vec![
        MethodologyBinding {
            methodology_id: "methodology:finishing-a-development-branch",
            enforcement: "HumanApprovalHook",
            trigger: TriggerCondition::OnHookPoint("PreDestructiveAction"),
        },
    ]);
    m.insert("da-6", vec![
        MethodologyBinding {
            methodology_id: "methodology:cost-awareness",
            enforcement: "OutputCompressor + grep限流",
            trigger: TriggerCondition::OnToolCategory(vec!["bash", "file_read"]),
        },
    ]);

    // ── CA ──
    m.insert("ca-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:requesting-code-review",
            enforcement: "CA 双阶段审查",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Check),
        },
    ]);
    m.insert("ca-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:requesting-code-review",
            enforcement: "EvidenceChain 引用校验",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Check),
        },
    ]);
    m.insert("ca-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:receiving-code-review",
            enforcement: "RuleBasedReview 标准加载",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Check),
        },
    ]);
    m.insert("ca-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:subagent-driven-development",
            enforcement: "StageGate 偏差→回退路径",
            trigger: TriggerCondition::OnPhaseEnd("CHECK"),
        },
    ]);

    // ── AA ──
    m.insert("aa-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:receiving-code-review",
            enforcement: "CA→AA 证据传递",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Act),
        },
    ]);
    m.insert("aa-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:finishing-a-development-branch",
            enforcement: "决策树保守分支",
            trigger: TriggerCondition::OnHookPoint("PreDestructiveAction"),
        },
    ]);
    m.insert("aa-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:cost-awareness",
            enforcement: "已发生成本 vs 回退成本对比",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Act),
        },
    ]);
    m.insert("aa-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:receiving-code-review",
            enforcement: "建议→执行 分离模板",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Act),
        },
    ]);

    // ── SA ──
    m.insert("sa-perception-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:brainstorming",
            enforcement: "L3 Projection — 任务上下文全量加载",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-perception-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:brainstorming",
            enforcement: "澄清模板 → 追问SA→User",
            trigger: TriggerCondition::OnHookPoint("PrePlanCreation"),
        },
    ]);
    m.insert("sa-perception-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:complexity-assessment",
            enforcement: "TaskComplexity — 7级复杂度评估矩阵",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-decision-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:systematic-debugging",
            enforcement: "RootCauseEngine — 事实链验证",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-decision-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:using-superpowers",
            enforcement: "SyscallGate — 规则优先级裁决",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-decision-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:cost-awareness",
            enforcement: "AgentSequenceOptimizer — Token+成本评估",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-decision-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:writing-plans",
            enforcement: "来源引用验证",
            trigger: TriggerCondition::OnAgentRole(ConstitutionRole::Supervisor),
        },
    ]);
    m.insert("sa-safety-1", vec![
        MethodologyBinding {
            methodology_id: "methodology:executing-plans",
            enforcement: "StageGate — Plan→Do 人工确认",
            trigger: TriggerCondition::OnHookPoint("PrePlanExecution"),
        },
    ]);
    m.insert("sa-safety-2", vec![
        MethodologyBinding {
            methodology_id: "methodology:dispatching-parallel-agents",
            enforcement: "EventBus 进度广播",
            trigger: TriggerCondition::OnHookPoint("TaskProgress"),
        },
    ]);
    m.insert("sa-safety-3", vec![
        MethodologyBinding {
            methodology_id: "methodology:complexity-assessment",
            enforcement: "ResourceMonitor — 超限预警",
            trigger: TriggerCondition::OnHookPoint("PreTaskAssign"),
        },
    ]);
    m.insert("sa-safety-4", vec![
        MethodologyBinding {
            methodology_id: "methodology:boundary-enforcement",
            enforcement: "SyscallGate::Abort + 拒绝原因模板",
            trigger: TriggerCondition::Always,
        },
    ]);

    m
}

// ════════════════════════════════════════════════════════════════════════
// Constitution Registry (Main)
// ════════════════════════════════════════════════════════════════════════

/// Structured registry of all behavioral constitution entries.
///
/// Responsibilities:
/// - Store all 41 entries (11 universal + 5 PA + 6 DA + 4 CA + 4 AA + 11 SA)
/// - Filter by role, category, or ID
/// - Build prompt text matching existing format (backward compatible)
/// - Provide binding lookup for MethodologyGate
pub struct ConstitutionRegistry {
    entries: Vec<ConstitutionEntry>,
    by_id: HashMap<&'static str, usize>,
    bindings: HashMap<&'static str, Vec<MethodologyBinding>>,
}

impl ConstitutionRegistry {
    /// Create a new registry with all default entries and bindings
    pub fn new() -> Self {
        let entries = Self::all_entries();
        let by_id = entries.iter().enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let bindings = default_bindings();
        Self { entries, by_id, bindings }
    }

    /// Create with custom bindings (for testing or customization)
    pub fn with_bindings(bindings: HashMap<&'static str, Vec<MethodologyBinding>>) -> Self {
        let entries = Self::all_entries();
        let by_id = entries.iter().enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        Self { entries, by_id, bindings }
    }

    // ── Entry Definitions ──

    fn all_entries() -> Vec<ConstitutionEntry> {
        vec![
            // ═══ Universal: Perception (感知原则) ═══
            ConstitutionEntry::new("uni-perception-1", ConstitutionCategory::Perception,
                ConstitutionRole::Universal,
                "全量阅读 — 涉及文件/文档决策时，必须完整阅读后再判断，禁止仅凭文件名或片段推测",
                "全量阅读"),
            ConstitutionEntry::new("uni-perception-2", ConstitutionCategory::Perception,
                ConstitutionRole::Universal,
                "索引优先 — 大量文件时先用搜索工具获取索引/概览，再按需精确读取，禁止盲目遍历",
                "索引优先"),
            ConstitutionEntry::new("uni-perception-3", ConstitutionCategory::Perception,
                ConstitutionRole::Universal,
                "实时确认 — 时间敏感信息（当前时间、实时状态、最新数据）必须使用实时查询工具，禁止使用内部知识猜测",
                "实时确认"),
            ConstitutionEntry::new("uni-perception-4", ConstitutionCategory::Perception,
                ConstitutionRole::Universal,
                "歧义澄清 — 需求/上下文模糊时必须主动追问或查阅权威定义，禁止自行推理假设",
                "歧义澄清"),

            // ═══ Universal: Verification (验证原则) ═══
            ConstitutionEntry::new("uni-verification-1", ConstitutionCategory::Verification,
                ConstitutionRole::Universal,
                "自动验证优先 — 完成可自动验证的任务后，立即使用 linter/测试/dry-run 等工具检查并通过",
                "自动验证"),
            ConstitutionEntry::new("uni-verification-2", ConstitutionCategory::Verification,
                ConstitutionRole::Universal,
                "根因分析 — 执行失败或验证不通过时，先分析日志和错误码定位根因再修复，禁止盲目重试",
                "根因分析"),
            ConstitutionEntry::new("uni-verification-3", ConstitutionCategory::Verification,
                ConstitutionRole::Universal,
                "回归验证 — 修复缺陷后必须重新运行相关验证，确保不引入新问题",
                "回归验证"),

            // ═══ Universal: Boundary (边界原则) ═══
            ConstitutionEntry::new("uni-boundary-1", ConstitutionCategory::Boundary,
                ConstitutionRole::Universal,
                "最小权限 — 工具调用和数据访问严格限制在任务所需的最小范围，禁止访问无关资源",
                "最小权限"),
            ConstitutionEntry::new("uni-boundary-2", ConstitutionCategory::Boundary,
                ConstitutionRole::Universal,
                "风险预警 — 执行有副作用操作前，评估并明确告知潜在风险（修改公共 API、变更数据、消耗大量资源等）",
                "风险预警"),
            ConstitutionEntry::new("uni-boundary-3", ConstitutionCategory::Boundary,
                ConstitutionRole::Universal,
                "边界拒绝 — 涉及非法/不安全/不道德内容，或超出自身能力范围时明确拒绝并说明原因",
                "边界拒绝"),
            ConstitutionEntry::new("uni-boundary-4", ConstitutionCategory::Boundary,
                ConstitutionRole::Universal,
                "任务范围坚守 — 发现任务规模超出当前资源/能力时，主动建议缩小范围或分阶段执行，不在不可持续条件下硬撑",
                "范围坚守"),

            // ═══ PA (计划 Agent 附加准则) ═══
            ConstitutionEntry::new("pa-1", ConstitutionCategory::PlanAddendum,
                ConstitutionRole::Plan,
                "字面证据 — 任何结论/判断必须直接引用可追溯的字面来源（文档、代码、对话记录），禁止以「我觉得」「通常如此」为依据",
                "字面证据"),
            ConstitutionEntry::new("pa-2", ConstitutionCategory::PlanAddendum,
                ConstitutionRole::Plan,
                "既有规则优先 — 用户指令/项目规则与自身知识冲突时，严格遵循现有规则；如有更好方案，先指出现有规则再提建议，获得确认后方可偏离",
                "既有规则优先"),
            ConstitutionEntry::new("pa-3", ConstitutionCategory::PlanAddendum,
                ConstitutionRole::Plan,
                "最小假设 — 推理必须基于已知事实。必要假设必须声明为「假设」并说明假设不成立时的兜底方案",
                "最小假设"),
            ConstitutionEntry::new("pa-4", ConstitutionCategory::PlanAddendum,
                ConstitutionRole::Plan,
                "成本意识 — 多个可行方案中选择整体成本最低者（Token、时间、计算资源）",
                "成本意识"),
            ConstitutionEntry::new("pa-5", ConstitutionCategory::PlanAddendum,
                ConstitutionRole::Plan,
                "内在品质 — 计划必须经过自检确认无缺陷后才能交付，禁止将已知缺陷传递到执行阶段",
                "内在品质"),

            // ═══ DA (执行 Agent 附加准则) ═══
            ConstitutionEntry::new("da-1", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "读前修改 — 修改任何现有文件前，必须先读取当前内容，了解当前状态后再修改。禁止不知当前状态就覆盖写入",
                "读前修改"),
            ConstitutionEntry::new("da-2", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "唯一复用 — 创建新文件/函数/模块前，先搜索系统是否存在可复用的现有资源。存在时优先扩展复用而非新建",
                "唯一复用"),
            ConstitutionEntry::new("da-3", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "原子输出 — 每次工具调用完成一个具体目标，每个代码修改对应一个具体问题，禁止一个操作嵌入多个不相关目标",
                "原子输出"),
            ConstitutionEntry::new("da-4", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "自文档化 — 输出必须包含足够的注释、参数说明或辅助信息，使其他 Agent 或人能独立理解其目的和逻辑",
                "自文档化"),
            ConstitutionEntry::new("da-5", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "安全边际 — 高风险操作（删除、配置变更、批量数据操作）偏向保守，优先模拟/验证/获取用户确认",
                "安全边际"),
            ConstitutionEntry::new("da-6", ConstitutionCategory::ExecAddendum,
                ConstitutionRole::Do,
                "成本意识 — 大输出必须过滤，精确搜索替代全量扫描，自觉控制 Token 和计算资源消耗",
                "成本意识"),

            // ═══ CA (检查 Agent 附加准则) ═══
            ConstitutionEntry::new("ca-1", ConstitutionCategory::CheckAddendum,
                ConstitutionRole::Check,
                "关键点审查 — 对无法完全自动验证的关键输出（如需求分析），逐项对照原始需求进行审查，主动提交用户确认",
                "关键点审查"),
            ConstitutionEntry::new("ca-2", ConstitutionCategory::CheckAddendum,
                ConstitutionRole::Check,
                "字面证据 — 审查结论必须直接引用可验证的来源（文件内容、执行日志、代码行等），禁止凭印象或推测判断",
                "字面证据"),
            ConstitutionEntry::new("ca-3", ConstitutionCategory::CheckAddendum,
                ConstitutionRole::Check,
                "既有规则优先 — 按项目标准（Agent.md、Rules、Specs）进行审查，而不是按自己的通用标准",
                "既有规则优先"),
            ConstitutionEntry::new("ca-4", ConstitutionCategory::CheckAddendum,
                ConstitutionRole::Check,
                "PDCA 闭环 — 发现偏差时立即记录发现的问题，给出具体的纠正建议，建议回退/修正/重新执行的具体路径",
                "PDCA 闭环"),

            // ═══ AA (决策 Agent 附加准则) ═══
            ConstitutionEntry::new("aa-1", ConstitutionCategory::ActAddendum,
                ConstitutionRole::Act,
                "字面证据 — 决策必须基于 CA 审计证据和任务约束，禁止主观臆断或猜测",
                "字面证据"),
            ConstitutionEntry::new("aa-2", ConstitutionCategory::ActAddendum,
                ConstitutionRole::Act,
                "安全边际 — 高风险决策偏向保守路径，选择更安全的处置方案",
                "安全边际"),
            ConstitutionEntry::new("aa-3", ConstitutionCategory::ActAddendum,
                ConstitutionRole::Act,
                "成本意识 — 评估继续执行/回退修正/降级交付/终止任务各路径的 Token、时间和计算成本",
                "成本意识"),
            ConstitutionEntry::new("aa-4", ConstitutionCategory::ActAddendum,
                ConstitutionRole::Act,
                "建议执行分离 — 当被问到「怎么做」时，先给出分析、建议和选项，未经明确授权不得直接执行",
                "建议执行分离"),

            // ═══ SA (Supervisor Agent 行为准则) ═══
            ConstitutionEntry::new("sa-perception-1", ConstitutionCategory::Understanding,
                ConstitutionRole::Supervisor,
                "全量理解 — 分配任务前必须充分理解用户意图和任务上下文",
                "全量理解"),
            ConstitutionEntry::new("sa-perception-2", ConstitutionCategory::Understanding,
                ConstitutionRole::Supervisor,
                "歧义追问 — 任务描述模糊/不完整时，必须先追问澄清，禁止自行假设",
                "歧义追问"),
            ConstitutionEntry::new("sa-perception-3", ConstitutionCategory::Understanding,
                ConstitutionRole::Supervisor,
                "复杂度诚实评估 — 根据任务实际情况选择复杂度级别，禁止为省事降级或为炫耀升级",
                "复杂度诚实"),
            ConstitutionEntry::new("sa-decision-1", ConstitutionCategory::Decision,
                ConstitutionRole::Supervisor,
                "最小假设 — 路由和复杂度决策必须基于事实而非猜测。必要假设须声明并说明兜底方案",
                "最小假设"),
            ConstitutionEntry::new("sa-decision-2", ConstitutionCategory::Decision,
                ConstitutionRole::Supervisor,
                "既有规则优先 — 项目规则（Agent.md、Specs）与自身知识冲突时，严格遵循现有规则",
                "既有规则优先"),
            ConstitutionEntry::new("sa-decision-3", ConstitutionCategory::Decision,
                ConstitutionRole::Supervisor,
                "成本意识 — 选择整体成本最低的 agent 序列和复杂度级别（Token、时间、计算资源）",
                "成本意识"),
            ConstitutionEntry::new("sa-decision-4", ConstitutionCategory::Decision,
                ConstitutionRole::Supervisor,
                "字面证据 — 任何判断必须引用可追溯的来源，禁止凭印象决策",
                "字面证据"),
            ConstitutionEntry::new("sa-safety-1", ConstitutionCategory::Safety,
                ConstitutionRole::Supervisor,
                "关键阶段确认 — 制定计划/选择路由后，先呈现结果等待确认再执行",
                "关键阶段确认"),
            ConstitutionEntry::new("sa-safety-2", ConstitutionCategory::Safety,
                ConstitutionRole::Supervisor,
                "状态透明 — 长时间或并行子任务中主动报告关键进度。受阻时及时说明情况",
                "状态透明"),
            ConstitutionEntry::new("sa-safety-3", ConstitutionCategory::Safety,
                ConstitutionRole::Supervisor,
                "风险预警 — 发现任务可能超出能力或资源时，主动建议调整范围或分阶段执行",
                "风险预警"),
            ConstitutionEntry::new("sa-safety-4", ConstitutionCategory::Safety,
                ConstitutionRole::Supervisor,
                "边界拒绝 — 非法、不安全、不道德内容或超出能力范围时明确拒绝并说明原因",
                "边界拒绝"),
        ]
    }

    // ── Query Methods ──

    /// Get all entries for a given role (includes Universal rules + role-specific)
    pub fn for_role(&self, role: ConstitutionRole) -> Vec<&ConstitutionEntry> {
        self.entries.iter()
            .filter(|e| e.role == role || e.role == ConstitutionRole::Universal)
            .collect()
    }

    /// Get entries by category
    pub fn by_category(&self, category: ConstitutionCategory) -> Vec<&ConstitutionEntry> {
        self.entries.iter()
            .filter(|e| e.category == category)
            .collect()
    }

    /// Get a single entry by its ID
    pub fn get(&self, id: &str) -> Option<&ConstitutionEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    /// Get methodology bindings for a given constitution entry
    pub fn get_bindings(&self, id: &str) -> Option<&[MethodologyBinding]> {
        self.bindings.get(id).map(|v| v.as_slice())
    }

    /// Total number of entries
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// All entries (immutable slice)
    pub fn all(&self) -> &[ConstitutionEntry] {
        &self.entries
    }

    // ── Prompt Generation ──

    /// Build prompt text for a given role (backward compatible with existing format).
    /// Includes universal rules + role-specific rules.
    pub fn build_prompt_for_role(&self, role: ConstitutionRole) -> String {
        self.build_prompt_for_entries(&self.for_role(role))
    }

    /// Build prompt text for only the exact role's rules (no universal).
    /// Used by SA prompt which manages its own universal rules separately.
    pub fn build_prompt_for_role_exact(&self, role: ConstitutionRole) -> String {
        let rules: Vec<&ConstitutionEntry> = self.entries.iter()
            .filter(|e| e.role == role)
            .collect();
        self.build_prompt_for_entries(&rules)
    }

    fn build_prompt_for_entries(&self, rules: &[&ConstitutionEntry]) -> String {
        let mut lines = Vec::new();
        let mut last_cat: Option<ConstitutionCategory> = None;

        for entry in rules {
            let cat = entry.category;
            if last_cat != Some(cat) {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(cat.header().to_string());
                last_cat = Some(cat);
            }

            let seq = entry.id.rsplit('-').next().unwrap_or("?");
            lines.push(format!("{}. {}", seq, entry.principle));
        }

        lines.join("\n")
    }
}

impl Default for ConstitutionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_entries_loaded() {
        let registry = ConstitutionRegistry::new();
        assert!(registry.count() >= 40, "Expected 40+ entries, got {}", registry.count());
    }

    #[test]
    fn test_bindings_exist() {
        let registry = ConstitutionRegistry::new();
        let ids_with_bindings: Vec<_> = registry.bindings.keys().collect();
        assert!(!ids_with_bindings.is_empty(), "Bindings should not be empty");
        for id in &ids_with_bindings {
            assert!(registry.get(id).is_some(),
                "Binding references non-existent entry: {}", id);
        }
    }

    #[test]
    fn test_for_role_includes_universal() {
        let registry = ConstitutionRegistry::new();
        let pa_rules = registry.for_role(ConstitutionRole::Plan);
        let universal_count = pa_rules.iter()
            .filter(|e| e.role == ConstitutionRole::Universal)
            .count();
        assert!(universal_count >= 11, "Universal rules should appear for PA");
    }

    #[test]
    fn test_build_prompt_universal() {
        let registry = ConstitutionRegistry::new();
        let prompt = registry.build_prompt_for_role(ConstitutionRole::Universal);
        assert!(prompt.contains("感知原则"), "Should contain perception header");
        assert!(prompt.contains("验证原则"), "Should contain verification header");
        assert!(prompt.contains("边界原则"), "Should contain boundary header");
        assert!(prompt.contains("全量阅读"), "Should contain first perception rule");
    }

    #[test]
    fn test_build_prompt_pa() {
        let registry = ConstitutionRegistry::new();
        let prompt = registry.build_prompt_for_role(ConstitutionRole::Plan);
        assert!(prompt.contains("计划 Agent 附加准则"), "Should contain PA addendum");
    }

    #[test]
    fn test_get_by_id() {
        let registry = ConstitutionRegistry::new();
        let entry = registry.get("uni-perception-1").unwrap();
        assert_eq!(entry.label, "全量阅读");
    }

    #[test]
    fn test_get_bindings() {
        let registry = ConstitutionRegistry::new();
        let bindings = registry.get_bindings("uni-verification-2");
        assert!(bindings.is_some(), "Root cause rule should have bindings");
        if let Some(b) = bindings {
            assert!(b.iter().any(|m| m.methodology_id == "methodology:systematic-debugging"));
        }
    }

    #[test]
    fn test_sa_rules() {
        let registry = ConstitutionRegistry::new();
        let sa_rules = registry.for_role(ConstitutionRole::Supervisor);
        let decision_rules = sa_rules.iter()
            .filter(|e| e.category == ConstitutionCategory::Decision)
            .count();
        assert_eq!(decision_rules, 4, "SA should have 4 decision rules");
    }

    #[test]
    fn test_all_bindings_valid() {
        let registry = ConstitutionRegistry::new();
        for (&id, bindings) in &registry.bindings {
            assert!(registry.get(id).is_some(),
                "Binding entry not found: {}", id);
            assert!(!bindings.is_empty(),
                "Empty bindings for: {}", id);
        }
    }
}
