/// Methodology Layer (L2) — Methodology definitions, registry, and activation conditions.
///
/// A methodology is an on-demand behavioral protocol extracted from Superpowers skills.
/// Methodologies sit between the Constitution (L3, always-on rules) and Enforcement (L1, code-level gates).
///
/// Architecture Layer: L2 — Methodology (On-Demand)
/// See design: PR-res/superpowers-skills-full-integration-design.md §0

pub mod gate;
pub mod integration;
pub mod evolution;

use crate::core::constitution::{ConstitutionRegistry, ConstitutionRole};

// ════════════════════════════════════════════════════════════════════════
// Methodology Types
// ════════════════════════════════════════════════════════════════════════

/// The nature of a methodology — determines how it's communicated to the agent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodologyType {
    /// Hard rules with authority language (YOU MUST, Never) — for TDD, verification, etc.
    Discipline,
    /// Guidance with collaborative framing — for brainstorming, reviews, etc.
    Guidance,
    /// Reference information — for tool mappings, skill descriptions
    Reference,
    /// Process flows — for multi-step workflows (plan→execute→review)
    Process,
}

/// When a methodology is automatically activated
#[derive(Debug, Clone)]
pub enum ActivationCondition {
    /// Always active for a given role
    Always,
    /// Active when a specific tool category is used
    OnToolCategory(&'static [&'static str]),
    /// Active at a specific hook point
    OnHookPoint(&'static str),
    /// Active at end of a phase
    OnPhaseEnd(&'static str),
    /// Active on task error
    OnTaskError,
    /// Active only for specific roles
    OnAgentRole(&'static [ConstitutionRole]),
}

/// A red flag entry — pattern the methodology should watch for
#[derive(Debug, Clone)]
pub struct RedFlagEntry {
    pub pattern: &'static str,
    pub severity: RedFlagSeverity,
    pub rationalization_check: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedFlagSeverity {
    /// Always blocks — must be addressed before proceeding
    Critical,
    /// Should be addressed but not blocking
    Warning,
    /// Advisory — best practice reminder
    Info,
}

/// An anti-pattern entry — with gate function for enforcement
#[derive(Debug, Clone)]
pub struct AntiPatternEntry {
    pub name: &'static str,
    pub description: &'static str,
    /// The action BEFORE which the gate check triggers
    pub gate_before: &'static str,
    /// The question the agent must ask itself
    pub gate_ask: &'static str,
    /// What happens if the anti-pattern is detected
    pub gate_action: &'static str,
}

/// Persuasion profile — how to frame the methodology in system prompts
#[derive(Debug, Clone)]
pub struct PersuasionProfile {
    /// Primary persuasion principles to use
    pub principles: &'static [&'static str],
    /// Example phrasing (evaluated at prompt build time)
    pub phrasing_examples: &'static [&'static str],
}

/// A full methodology definition
#[derive(Debug, Clone)]
pub struct MethodologyDefinition {
    /// Unique identifier like "methodology:index-priority"
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// One-line description
    pub description: &'static str,
    /// Methodology type (determines framing)
    pub methodology_type: MethodologyType,
    /// Domain this applies to ("general", "programming", "debugging", etc.)
    pub domain: &'static str,
    /// Source skill file in superpowers-main
    pub source: &'static str,
    /// Red flags to watch for
    pub red_flags: &'static [RedFlagEntry],
    /// Anti-patterns with gate functions
    pub anti_patterns: &'static [AntiPatternEntry],
    /// Persuasion profile for injection
    pub persuasion: PersuasionProfile,
    /// When to auto-activate
    pub activation: ActivationCondition,
    /// Related methodology IDs
    pub related: &'static [&'static str],
}

// ════════════════════════════════════════════════════════════════════════
// Methodology Registry
// ════════════════════════════════════════════════════════════════════════

/// Registry of all available methodologies.
///
/// Can load built-in definitions or be populated dynamically.
pub struct MethodologyRegistry {
    entries: Vec<MethodologyDefinition>,
}

impl MethodologyRegistry {
    /// Create registry with all built-in methodology definitions
    pub fn new() -> Self {
        Self { entries: builtin_methodologies() }
    }

    /// Create with custom set
    pub fn with_entries(entries: Vec<MethodologyDefinition>) -> Self {
        Self { entries }
    }

    /// Get a methodology by its ID
    pub fn get(&self, id: &str) -> Option<&MethodologyDefinition> {
        self.entries.iter().find(|m| m.id == id)
    }

    /// Get all methodologies
    pub fn all(&self) -> &[MethodologyDefinition] {
        &self.entries
    }

    /// Find methodologies matching an activation condition
    pub fn for_activation(&self, condition: &ActivationCondition) -> Vec<&MethodologyDefinition> {
        self.entries.iter()
            .filter(|m| std::mem::discriminant(&m.activation) == std::mem::discriminant(condition))
            .collect()
    }

    /// Get methodologies for a specific domain
    pub fn for_domain(&self, domain: &str) -> Vec<&MethodologyDefinition> {
        self.entries.iter()
            .filter(|m| m.domain == domain || m.domain == "general")
            .collect()
    }

    /// Number of registered methodologies
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for MethodologyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ════════════════════════════════════════════════════════════════════════
// Built-in Methodology Definitions
// ════════════════════════════════════════════════════════════════════════

/// Returns all built-in methodology definitions.
///
/// These correspond to the 14 superpowers-main skills plus 5 new methodologies
/// that fill gaps identified during Constitution analysis:
/// - Index-Priority (行为准则 "索引优先" had no Superpowers equivalent)
/// - Cost-Awareness (行为准则 "成本意识" was only implicit in DA/PA)
/// - Least-Privilege (行为准则 "最小权限" had no formal protocol)
/// - Complexity-Assessment (行为准则 "复杂度诚实评估" had no Superpowers equivalent)
/// - Boundary-Enforcement (行为准则 "边界拒绝" + "边界原则" aggregation)
pub fn builtin_methodologies() -> Vec<MethodologyDefinition> {
    vec![
        // ── 1. Index-Priority (NEW) ──
        MethodologyDefinition {
            id: "methodology:index-priority",
            name: "索引优先策略",
            description: "面对大量文件或数据时，先用搜索工具获取索引/概览，再按需精确读取",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "new — constitution gap fill for uni-perception-2",
            red_flags: &[
                RedFlagEntry {
                    pattern: "盲目遍历目录",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『目录不大，直接看完更快』— 先搜索再读取总是更优"),
                },
                RedFlagEntry {
                    pattern: "基于文件名猜测内容",
                    severity: RedFlagSeverity::Warning,
                    rationalization_check: Some("『看名字就知道是什么』— 实际内容可能与文件名不符"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "全量遍历",
                    description: "使用 ls -R / find . 遍历整个目录而非精确搜索",
                    gate_before: "执行目录遍历工具前",
                    gate_ask: "能否用 grep/glob 缩小范围后再遍历?",
                    gate_action: "STOP — 先搜索索引，再按需读取",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "commitment"],
                phrasing_examples: &["YOU MUST search before traverse", "Always get the index first"],
            },
            activation: ActivationCondition::OnToolCategory(&["file_search", "directory_list"]),
            related: &["methodology:using-superpowers", "methodology:cost-awareness"],
        },

        // ── 2. Cost-Awareness (NEW) ──
        MethodologyDefinition {
            id: "methodology:cost-awareness",
            name: "成本意识协议",
            description: "在所有决策中显式评估 Token、时间、计算资源的成本，选择整体成本最低的路径",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "new — constitution gap fill for pa-4, da-6, aa-3, sa-decision-3, uni-perception-3",
            red_flags: &[
                RedFlagEntry {
                    pattern: "不必要的大输出",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『全都看看比较保险』— 用 grep 过滤出需要的信息即可"),
                },
                RedFlagEntry {
                    pattern: "忽略 Token 预算",
                    severity: RedFlagSeverity::Warning,
                    rationalization_check: None,
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "盲目全量",
                    description: "不指定范围、不加 grep 过滤的大输出工具调用",
                    gate_before: "执行 bash / file_read 等可能产生大输出的工具前",
                    gate_ask: "输出可能超过 100 行吗? 能否用 | head / | grep 限制?",
                    gate_action: "STOP — 精确搜索替代全量扫描",
                },
                AntiPatternEntry {
                    name: "无比较方案",
                    description: "提出方案时只给一个选项，没有成本对比",
                    gate_before: "提交计划/决策前",
                    gate_ask: "有没有成本更低的替代方案?",
                    gate_action: "STOP — 至少提供 2 个选项及其成本对比",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "social_proof"],
                phrasing_examples: &["YOU MUST control token usage", "Always prefer the lowest-cost path"],
            },
            activation: ActivationCondition::OnHookPoint("PreToolCall"),
            related: &["methodology:index-priority", "methodology:verification-before-completion"],
        },

        // ── 3. Least-Privilege (NEW) ──
        MethodologyDefinition {
            id: "methodology:least-privilege",
            name: "最小权限协议",
            description: "工具调用和数据访问严格限制在任务所需的最小范围，禁止访问无关资源",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "new — constitution gap fill for uni-boundary-1",
            red_flags: &[
                RedFlagEntry {
                    pattern: "访问无关目录/文件",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『看一眼不会有什么』— 权限应当最小化，与任务无关即禁止"),
                },
                RedFlagEntry {
                    pattern: "使用危险命令",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『我就用一次』— 高风险操作必须走审批流程"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "权限越界",
                    description: "执行与当前任务目标无关的工具调用或数据访问",
                    gate_before: "执行任何工具前",
                    gate_ask: "这个工具/数据对当前任务目标是必要的吗?",
                    gate_action: "STOP — 移除非必要操作",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "security"],
                phrasing_examples: &["YOU MUST restrict to task scope", "No access outside task boundary"],
            },
            activation: ActivationCondition::OnToolCategory(&["shell", "file_write", "network"]),
            related: &["methodology:boundary-enforcement"],
        },

        // ── 4. Complexity-Assessment (NEW) ──
        MethodologyDefinition {
            id: "methodology:complexity-assessment",
            name: "复杂度诚实评估",
            description: "根据任务实际情况客观选择复杂度级别，禁止为省事降级或为炫耀升级",
            methodology_type: MethodologyType::Guidance,
            domain: "general",
            source: "new — constitution gap fill for sa-perception-3, uni-boundary-4",
            red_flags: &[
                RedFlagEntry {
                    pattern: "省事降级",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『这个很容易，简单处理就行』— 复杂度应基于任务客观特征"),
                },
                RedFlagEntry {
                    pattern: "炫耀升级",
                    severity: RedFlagSeverity::Warning,
                    rationalization_check: Some("『用高级功能才显得专业』— 最简单能解决问题的方案最优"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "复杂度偏差",
                    description: "选择的复杂度级别与任务实际需求不匹配",
                    gate_before: "SA 选择复杂度级别前",
                    gate_ask: "评估因素: 1) 目标明确度 2) 步骤数量 3) 风险等级 4) 资源约束",
                    gate_action: "STOP — 用 complexity_matrix 重新评估",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["social_proof", "commitment"],
                phrasing_examples: &["Always match complexity to facts", "Be honest about difficulty"],
            },
            activation: ActivationCondition::OnAgentRole(&[ConstitutionRole::Supervisor, ConstitutionRole::Plan]),
            related: &["methodology:cost-awareness", "methodology:boundary-enforcement"],
        },

        // ── 5. Boundary-Enforcement (NEW) ──
        MethodologyDefinition {
            id: "methodology:boundary-enforcement",
            name: "边界强制执行",
            description: "在遇到安全边界、能力边界或伦理边界时，必须拒绝、预警或退出",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "new — constitution gap fill for uni-boundary-2, uni-boundary-3, sa-safety-4",
            red_flags: &[
                RedFlagEntry {
                    pattern: "在能力范围内硬撑",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『再试一次可能就行了』— 超过能力边界应及时求助"),
                },
                RedFlagEntry {
                    pattern: "忽略风险",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『应该没问题』— 必须有风险评估"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "非法请求",
                    description: "涉及非法、不安全、不道德内容的请求",
                    gate_before: "响应任何请求前",
                    gate_ask: "这是安全/合法/道德的请求吗?",
                    gate_action: "ABORT — 明确拒绝并说明原因",
                },
                AntiPatternEntry {
                    name: "越界执行",
                    description: "执行超出自身能力范围或任务授权的操作",
                    gate_before: "执行可能越界的操作前",
                    gate_ask: "我有权/有能力执行这个操作吗?",
                    gate_action: "STOP — 建议缩小范围或申请授权",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "unity"],
                phrasing_examples: &["YOU MUST refuse unsafe requests", "We are responsible for safety"],
            },
            activation: ActivationCondition::Always,
            related: &["methodology:least-privilege", "methodology:complexity-assessment"],
        },

        // ── 6. Using-Superpowers (existing skill → methodology) ──
        MethodologyDefinition {
            id: "methodology:using-superpowers",
            name: "技能使用方法论",
            description: "在任何响应或操作之前调用相关的技能，检查红线表防止常见错误",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "superpowers-main/skills/using-superpowers/SKILL.md",
            red_flags: &[
                RedFlagEntry {
                    pattern: "跳过技能检查",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『这只是一个简单的问题』— 问题就是任务，检查方法论"),
                },
                RedFlagEntry {
                    pattern: "基于文件名猜测内容",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『我知道那是什么意思』— 知道概念 ≠ 不使用方法论"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "假设即答案",
                    description: "仅凭文件名或部分信息推测内容",
                    gate_before: "对文件/代码做判断前",
                    gate_ask: "已经完整阅读了吗?",
                    gate_action: "STOP — 先 Read，后判断",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "commitment", "social_proof"],
                phrasing_examples: &["Always check methodology first", "Never skip the checklist"],
            },
            activation: ActivationCondition::Always,
            related: &["methodology:index-priority"],
        },

        // ── 7. Brainstorming ──
        MethodologyDefinition {
            id: "methodology:brainstorming",
            name: "头脑风暴方法论",
            description: "在任何创造性工作之前必须使用——先探索用户意图、需求和设计",
            methodology_type: MethodologyType::Process,
            domain: "general",
            source: "superpowers-main/skills/brainstorming/SKILL.md",
            red_flags: &[
                RedFlagEntry {
                    pattern: "设计前不探索",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『需求很清楚，不需要探索』— 先探索再设计"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "跳过澄清",
                    description: "需求模糊时直接实现而非先追问",
                    gate_before: "进入实现前",
                    gate_ask: "所有需求都已澄清无疑问?",
                    gate_action: "STOP — 先澄清再实现",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["unity", "commitment"],
                phrasing_examples: &["Let's explore before building", "We are colleagues working together"],
            },
            activation: ActivationCondition::OnHookPoint("PrePlanCreation"),
            related: &["methodology:writing-plans", "methodology:complexity-assessment"],
        },

        // ── 8. TDD ──
        MethodologyDefinition {
            id: "methodology:test-driven-development",
            name: "测试驱动开发",
            description: "在实现任何功能或修复 bug 时，先编写测试再编写实现代码",
            methodology_type: MethodologyType::Discipline,
            domain: "programming",
            source: "superpowers-main/skills/test-driven-development/SKILL.md",
            red_flags: &[
                RedFlagEntry {
                    pattern: "先实现后测试",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『先写代码，测试后面补』— 必须先写测试"),
                },
                RedFlagEntry {
                    pattern: "删除测试通过",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『测试写得不对，删了重来』— 先看测试失败原因"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "实现先行",
                    description: "不先写测试就开始编写实现代码",
                    gate_before: "编写任何实现代码前",
                    gate_ask: "测试写了吗? 测试通过了吗（红）?",
                    gate_action: "STOP — 先写测试，看到红，再写实现",
                },
                AntiPatternEntry {
                    name: "Mock 代替真实行为",
                    description: "用 mock 替代了真实行为的验证",
                    gate_before: "对 mock 断言前",
                    gate_ask: "测试的是真实行为还是 mock 行为?",
                    gate_action: "STOP — 测试真实行为而非 mock",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "commitment"],
                phrasing_examples: &["YOU MUST test before code", "Never skip the RED phase"],
            },
            activation: ActivationCondition::OnToolCategory(&["file_write", "code_generation"]),
            related: &["methodology:verification-before-completion", "methodology:systematic-debugging"],
        },

        // ── 9. Systematic Debugging ──
        MethodologyDefinition {
            id: "methodology:systematic-debugging",
            name: "系统化调试",
            description: "遇到任何 bug、测试失败或异常行为时，先系统化分析根因再修复",
            methodology_type: MethodologyType::Process,
            domain: "programming",
            source: "superpowers-main/skills/systematic-debugging/SKILL.md",
            red_flags: &[
                RedFlagEntry {
                    pattern: "盲目重试",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『再试一次可能就好了』— 先分析日志定位根因"),
                },
                RedFlagEntry {
                    pattern: "随机修改",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『改改这里试试』— 一次改一个变量，验证再改"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "Shotgun Debugging",
                    description: "同时修改多处，期望某处修复问题",
                    gate_before: "同时修改多个文件/变量前",
                    gate_ask: "每个修改有针对根因吗? 一次只改一处?",
                    gate_action: "STOP — 一次改一处，验证后再改下一处",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "discipline"],
                phrasing_examples: &["Always find root cause first", "One change at a time, verify each"],
            },
            activation: ActivationCondition::OnTaskError,
            related: &["methodology:test-driven-development", "methodology:verification-before-completion"],
        },

        // ── 10. Verification Before Completion ──
        MethodologyDefinition {
            id: "methodology:verification-before-completion",
            name: "完成前验证",
            description: "在宣称工作完成前，必须运行验证命令并确认输出后才能声称成功",
            methodology_type: MethodologyType::Discipline,
            domain: "general",
            source: "superpowers-main/skills/verification-before-completion/SKILL.md",
            red_flags: &[
                RedFlagEntry {
                    pattern: "『测试通过』但未运行",
                    severity: RedFlagSeverity::Critical,
                    rationalization_check: Some("『代码很简单，肯定没问题』— 必须运行验证"),
                },
            ],
            anti_patterns: &[
                AntiPatternEntry {
                    name: "无证据声明",
                    description: "宣称工作完成但没有提供任何验证证据",
                    gate_before: "报告任务完成前",
                    gate_ask: "运行了验证命令? 输出了什么? 诊断通过了?",
                    gate_action: "STOP — 运行验证并附上输出证据",
                },
            ],
            persuasion: PersuasionProfile {
                principles: &["authority", "commitment"],
                phrasing_examples: &["No evidence = not complete", "Always verify before claiming done"],
            },
            activation: ActivationCondition::OnPhaseEnd("ACT"),
            related: &["methodology:test-driven-development"],
        },
    ]
}

// ════════════════════════════════════════════════════════════════════════
// Constitution → Methodology Binding Resolver
// ════════════════════════════════════════════════════════════════════════

/// Resolves which methodologies to activate based on context.
///
/// Combines constitution registry bindings with methodology activation conditions.
pub struct MethodologyResolver {
    methodologies: MethodologyRegistry,
    constitution: ConstitutionRegistry,
}

impl MethodologyResolver {
    pub fn new(methodologies: MethodologyRegistry, constitution: ConstitutionRegistry) -> Self {
        Self { methodologies, constitution }
    }

    /// Get all constitutions that bind to a given methodology ID
    pub fn constitutions_for_methodology(&self, methodology_id: &str) -> Vec<String> {
        let mut result = Vec::new();
        for entry in self.constitution.all() {
            if let Some(bindings) = self.constitution.get_bindings(entry.id) {
                if bindings.iter().any(|b| b.methodology_id == methodology_id) {
                    result.push(entry.id.to_string());
                }
            }
        }
        result
    }

    /// Get all methodologies that a constitution rule binds to
    pub fn methodologies_for_constitution(&self, constitution_id: &str) -> Vec<&MethodologyDefinition> {
        self.constitution.get_bindings(constitution_id)
            .map(|bindings| {
                bindings.iter()
                    .filter_map(|b| self.methodologies.get(b.methodology_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count of constitution-methodology mappings
    pub fn binding_count(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for entry in self.constitution.all() {
            if let Some(bindings) = self.constitution.get_bindings(entry.id) {
                for b in bindings {
                    seen.insert((entry.id, b.methodology_id));
                }
            }
        }
        seen.len()
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_methodologies_loaded() {
        let registry = MethodologyRegistry::new();
        assert!(registry.count() >= 10, "Expected 10+ built-in methodologies, got {}", registry.count());
    }

    #[test]
    fn test_get_methodology_by_id() {
        let registry = MethodologyRegistry::new();
        let idx = registry.get("methodology:index-priority").unwrap();
        assert_eq!(idx.name, "索引优先策略");
    }

    #[test]
    fn test_new_methodologies_present() {
        let registry = MethodologyRegistry::new();
        for id in &[
            "methodology:index-priority",
            "methodology:cost-awareness",
            "methodology:least-privilege",
            "methodology:complexity-assessment",
            "methodology:boundary-enforcement",
        ] {
            assert!(registry.get(id).is_some(), "Missing new methodology: {}", id);
        }
    }

    #[test]
    fn test_methodology_has_red_flags() {
        let registry = MethodologyRegistry::new();
        for method in registry.all() {
            assert!(!method.red_flags.is_empty(),
                "Methodology {} has no red flags", method.id);
        }
    }

    #[test]
    fn test_methodology_has_anti_patterns() {
        let registry = MethodologyRegistry::new();
        for method in registry.all() {
            assert!(!method.anti_patterns.is_empty(),
                "Methodology {} has no anti-patterns", method.id);
        }
    }

    #[test]
    fn test_resolver_bindings() {
        let resolver = MethodologyResolver::new(
            MethodologyRegistry::new(),
            ConstitutionRegistry::new(),
        );
        assert!(resolver.binding_count() > 30,
            "Expected 30+ constitution-methodology bindings, got {}", resolver.binding_count());
    }

    #[test]
    fn test_constitutions_for_methodology() {
        let resolver = MethodologyResolver::new(
            MethodologyRegistry::new(),
            ConstitutionRegistry::new(),
        );
        let cons = resolver.constitutions_for_methodology("methodology:systematic-debugging");
        assert!(cons.contains(&"uni-verification-2".to_string()),
            "Root cause rule should bind to systematic-debugging");
    }
}
