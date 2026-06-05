/// Methodology Integration Layer (Phase 3) — Role-specific prompt addendums.
///
/// Generates methodology-aware prompt sections for each agent role:
/// - PA: Plan-review gate + Granularity check
/// - CA: Dual-stage audit + Anti-pattern detection  
/// - AA: Pressure testing + Meta-test protocol
/// - SA: Methodology auto-trigger awareness
///
/// These addendums are injected into agent system prompts alongside the
/// constitution behavioral policy, instructing agents to apply specific
/// methodology disciplines during execution.
///
/// Architecture Layer: L2/L1 boundary — Methodology → Agent Prompt
/// See design: PR-res/superpowers-skills-full-integration-design.md §3

use crate::core::agent_instance::AgentRole;
use crate::methodology::MethodologyRegistry;

/// Generates role-specific methodology prompt addendums.
///
/// Stateless — creates a fresh registry each time, following the same
/// pattern as `build_constitution_prompt` in `system_prompt.rs`.
pub struct MethodologyPromptInjector;

impl MethodologyPromptInjector {
    /// Build the full methodology addendum for an agent role.
    ///
    /// Returns `None` for roles that only need the constitution baseline (DA).
    pub fn build_for_role(role: AgentRole) -> Option<String> {
        let registry = MethodologyRegistry::new();
        match role {
            AgentRole::Plan => Some(Self::pa_plan_review_gate(&registry)),
            AgentRole::Check => Some(Self::ca_dual_stage_audit(&registry)),
            AgentRole::Act => Some(Self::aa_pressure_test(&registry)),
            AgentRole::Do => None,
        }
    }

    /// Build methodology addendum for SA (used in sa.rs prompt assembly).
    pub fn build_for_sa() -> String {
        let registry = MethodologyRegistry::new();
        Self::sa_methodology_awareness(&registry)
    }

    // ─── PA: Plan-Review Gate + Granularity Check ───

    fn pa_plan_review_gate(registry: &MethodologyRegistry) -> String {
        let complexity = registry.get("methodology:complexity-assessment");
        let boundary = registry.get("methodology:boundary-enforcement");

        let mut sections = vec![
            "\n## 📋 方法论纪律 — 计划审查闸门".to_string(),
            "作为计划Agent，你必须遵守以下方法论纪律：".to_string(),
        ];

        sections.push("\n### 1. 步骤粒度检查".to_string());
        sections.push(
            "对每个计划步骤，必须评估其粒度是否合适：\n\
            - ✅ 过粗：一个步骤包含多个不相关的操作 → 拆解\n\
            - ✅ 过细：一个步骤只包含原子操作 → 合并\n\
            - ✅ 标准：一个步骤包含一组相关的操作，有明确的输入和产出\n\
            示例：'创建文件' 和 '修改文件' 不应分为两步，应合并为 '实现功能X'"
                .to_string(),
        );

        if let Some(c) = complexity {
            let reds: Vec<&str> = c.red_flags.iter().map(|r| r.pattern).collect();
            sections.push("\n### 2. 复杂度匹配".to_string());
            sections.push(format!(
                "所选复杂度级别必须与任务实际需求匹配，禁止：\n\
                - {}：为省事选择低于实际需求的级别\n\
                - {}：为炫技选择高于实际需求的级别",
                reds.first().unwrap_or(&"降级"),
                reds.get(1).unwrap_or(&"升级"),
            ));
        }

        if let Some(b) = boundary {
            let aps: Vec<&str> = b.anti_patterns.iter().map(|ap| ap.name).collect();
            sections.push("\n### 3. 边界检查".to_string());
            sections.push(format!(
                "计划中不得包含越界操作：\n\
                - 检查每个步骤的职责是否在 Agent 权限范围内\n\
                - 禁止计划中包含 {} 类操作\n\
                - 如果发现越界，必须标记并建议修正",
                aps.first().unwrap_or(&"越界执行"),
            ));
        }

        sections.join("\n")
    }

    // ─── CA: Dual-Stage Audit + Anti-Pattern Detection ───

    fn ca_dual_stage_audit(registry: &MethodologyRegistry) -> String {
        let debugging = registry.get("methodology:systematic-debugging");
        let verification = registry.get("methodology:verification-before-completion");

        let mut sections = vec![
            "\n## 📋 方法论纪律 — 双阶段审计 + 反模式检测".to_string(),
            "作为检查Agent，你必须执行双阶段审计：".to_string(),
        ];

        sections.push("\n### 阶段一：输出验证（Output Verification）".to_string());
        sections.push(
            "验证执行结果是否符合任务要求：\n\
            - 检查产物是否完整、正确\n\
            - 验证是否满足成功标准\n\
            - 确认所有步骤是否都已完成\n\
            - 检查文件是否真实存在（不要仅依赖 summary）"
                .to_string(),
        );

        sections.push("\n### 阶段二：方法论合规检查（Methodology Compliance）".to_string());
        sections.push(
            "检查执行过程是否符合方法论要求：\n\
            - 是否先验证再宣称完成？（Verification-Before-Completion）\n\
            - 是否存在已知反模式？（如盲目重试、随机修改、全量遍历等）\n\
            - 是否遵循了成本意识？（避免不必要的大输出、无比较方案）\n\
            - 是否在权限范围内操作？（Least-Privilege / Boundary-Enforcement）"
                .to_string(),
        );

        if let Some(d) = debugging {
            let aps: Vec<&str> = d.anti_patterns.iter().map(|ap| ap.name).collect();
            sections.push("\n### 反模式检测".to_string());
            sections.push(format!(
                "在审计过程中，重点排查以下反模式：\n\
                - {}：是否同时修改多处而不验证每处？\n\
                - 无证据声明：是否宣称完成但没有验证证据？\n\
                - 实现先行：是否先写代码后写测试？",
                aps.first().unwrap_or(&"Shotgun Debugging"),
            ));
        }

        if let Some(v) = verification {
            let reds: Vec<&str> = v.red_flags.iter().map(|r| r.pattern).collect();
            sections.push("\n### 证据要求".to_string());
            sections.push(format!(
                "CA 自身的审计结论也需要提供验证证据：\n\
                - 不要出现『{}』的情况\n\
                - 每个 PASS/FAIL 结论都要附上实际检查结果\n\
                - 使用工具验证后再下结论",
                reds.first().unwrap_or(&"「测试通过」但未运行"),
            ));
        }

        sections.join("\n")
    }

    // ─── AA: Pressure Testing + Meta-Test Protocol ───

    fn aa_pressure_test(registry: &MethodologyRegistry) -> String {
        let _debugging = registry.get("methodology:systematic-debugging");
        let _boundary = registry.get("methodology:boundary-enforcement");

        vec![
            "\n## 📋 方法论纪律 — 压力测试 + 元测试协议".to_string(),
            "作为决策Agent，你必须执行压力测试：".to_string(),
            "".to_string(),
            "### 1. 压力测试（Pressure Testing）".to_string(),
            "在批准 CA 的审计结论之前，必须主动寻找漏洞：".to_string(),
            "- 假设 CA 可能遗漏了问题 → 尝试找到 CA 没发现的问题".to_string(),
            "- 从反面角度审视：'如果这个结论是错的，可能错在哪里？'".to_string(),
            "- 检查 CA 是否只验证了表面结果，没有深入检查".to_string(),
            "- 对 CA 的 PASS 结论提出至少一个质疑并验证".to_string(),
            "".to_string(),
            "### 2. 元测试协议（Meta-Test Protocol）".to_string(),
            "验证 CA 自身的验证过程是否可靠：".to_string(),
            "- CA 是否使用了工具验证？（还是仅依赖 summary？）".to_string(),
            "- CA 是否检查了所有相关维度？（5W2H 是否全覆盖？）".to_string(),
            "- CA 的审计结论是否有明确的证据支撑？".to_string(),
            "- 如果发现 CA 的验证有漏洞 → 标记并要求补充检查".to_string(),
            "".to_string(),
            "### 3. 决策责任".to_string(),
            "最终决策权在你，你要为决策后果负责：".to_string(),
            "- 不要做 CA 的『橡皮图章』——独立判断".to_string(),
            "- 综合考虑任务目标、实际结果和风险".to_string(),
            "- 如果需要回退，给出具体的回退建议".to_string(),
        ]
        .join("\n")
    }

    // ─── SA: Methodology Auto-Trigger Awareness ───

    fn sa_methodology_awareness(registry: &MethodologyRegistry) -> String {
        let always_on: Vec<&str> = registry
            .all()
            .iter()
            .filter(|m| matches!(m.activation, crate::methodology::ActivationCondition::Always))
            .map(|m| m.name)
            .collect();

        let role_triggers: Vec<&str> = registry
            .all()
            .iter()
            .filter(|m| {
                matches!(
                    m.activation,
                    crate::methodology::ActivationCondition::OnAgentRole(_)
                )
            })
            .map(|m| m.name)
            .collect();

        let mut sections = vec![
            "\n## 📋 方法论纪律 — 自动触发协议".to_string(),
            "作为 Supervisor Agent，你在制定计划时需考虑以下方法论：".to_string(),
        ];

        if !always_on.is_empty() {
            sections.push("\n### 始终激活的方法论".to_string());
            sections.push(format!(
                "以下方法论始终有效，在计划中必须体现：\n- {}",
                always_on.join("\n- ")
            ));
        }

        if !role_triggers.is_empty() {
            sections.push("\n### 角色触发的方法论".to_string());
            sections.push(format!(
                "以下方法论会在特定 Agent 角色执行时自动触发：\n- {}",
                role_triggers.join("\n- ")
            ));
        }

        sections.push("\n### 计划生成要求".to_string());
        sections.push(
            "在生成计划时，确保：\n\
            - 每个步骤的职责与相应方法论匹配\n\
            - 为 DA 步骤预留足够的验证空间\n\
            - CA 步骤包含双阶段审计（输出验证 + 方法论合规）\n\
            - AA 步骤包含压力测试要求\n\
            - 步骤粒度合理：一组相关操作合并为一个步骤"
                .to_string(),
        );

        sections.join("\n")
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pa_addendum_exists() {
        let addendum = MethodologyPromptInjector::build_for_role(AgentRole::Plan);
        assert!(addendum.is_some(), "PA should have a methodology addendum");
        let text = addendum.unwrap();
        assert!(text.contains("粒度检查"), "PA addendum should mention granularity");
        assert!(text.contains("复杂度匹配"), "PA addendum should mention complexity");
        assert!(text.contains("边界检查"), "PA addendum should mention boundary");
    }

    #[test]
    fn test_ca_addendum_exists() {
        let addendum = MethodologyPromptInjector::build_for_role(AgentRole::Check);
        assert!(addendum.is_some(), "CA should have a methodology addendum");
        let text = addendum.unwrap();
        assert!(text.contains("双阶段审计"), "CA addendum should mention dual-stage audit");
        assert!(text.contains("阶段一"), "CA addendum should have Stage 1");
        assert!(text.contains("阶段二"), "CA addendum should have Stage 2");
        assert!(text.contains("反模式检测"), "CA addendum should mention anti-pattern detection");
    }

    #[test]
    fn test_aa_addendum_exists() {
        let addendum = MethodologyPromptInjector::build_for_role(AgentRole::Act);
        assert!(addendum.is_some(), "AA should have a methodology addendum");
        let text = addendum.unwrap();
        assert!(text.contains("压力测试"), "AA addendum should mention pressure testing");
        assert!(text.contains("元测试"), "AA addendum should mention meta-test");
        assert!(text.contains("橡皮图章"), "AA addendum should warn against rubber-stamping");
    }

    #[test]
    fn test_da_addendum_none() {
        let addendum = MethodologyPromptInjector::build_for_role(AgentRole::Do);
        assert!(addendum.is_none(), "DA should not have a methodology addendum");
    }

    #[test]
    fn test_sa_addendum_exists() {
        let text = MethodologyPromptInjector::build_for_sa();
        assert!(!text.is_empty(), "SA addendum should not be empty");
        assert!(text.contains("始终激活"), "SA addendum should mention always-on");
        assert!(text.contains("计划生成要求"), "SA addendum should have plan generation requirements");
    }

    #[test]
    fn test_addendum_includes_methodology_names() {
        let registry = MethodologyRegistry::new();
        let always_on_count = registry.all().iter()
            .filter(|m| matches!(m.activation, crate::methodology::ActivationCondition::Always))
            .count();

        let sa_text = MethodologyPromptInjector::build_for_sa();
        assert!(
            always_on_count == 0 || sa_text.contains("方法论"),
            "SA addendum should reference methodologies"
        );
    }

    #[test]
    fn test_pa_addendum_references_granularity_rules() {
        let text = MethodologyPromptInjector::build_for_role(AgentRole::Plan).unwrap();
        assert!(text.contains("过粗") || text.contains("过细"), "PA granularity check should define granularity levels");
    }

    #[test]
    fn test_ca_addendum_has_evidence_requirement() {
        let text = MethodologyPromptInjector::build_for_role(AgentRole::Check).unwrap();
        assert!(text.contains("证据"), "CA dual-stage audit should require evidence");
    }

    #[test]
    fn test_aa_addendum_mentions_decision_responsibility() {
        let text = MethodologyPromptInjector::build_for_role(AgentRole::Act).unwrap();
        assert!(text.contains("决策"), "AA pressure testing should mention decision responsibility");
    }

    #[test]
    fn test_all_roles_have_consistent_structure() {
        for role in &[AgentRole::Plan, AgentRole::Check, AgentRole::Act] {
            if let Some(text) = MethodologyPromptInjector::build_for_role(*role) {
                assert!(text.starts_with("\n##"), "{} addendum should start with a header", role);
                assert!(text.len() > 100, "{} addendum should be substantial", role);
            }
        }
    }
}
