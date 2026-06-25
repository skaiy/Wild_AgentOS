/// DefenseInDepthManager — generates defense recommendations from trace analysis.
///
/// Maps the defense-in-depth methodology to concrete recommendations:
/// Layer 1: EntryValidation — Prevent errors at entry point
/// Layer 2: BusinessLogic — Validate business logic assumptions
/// Layer 3: EnvironmentGuard — Guard against environmental issues
/// Layer 4: Instrumentation — Instrument for observability
///
/// Corresponds to: superpowers-main/skills/systematic-debugging/defense-in-depth.md
/// Architecture Layer: L1 — Enforcement (RootCauseEngine)

use super::types::{
    DefenseLayer, DefenseRecommendation, RootCauseConfig, TraceChain,
};

/// DefenseInDepthManager converts root cause analysis into actionable defense recommendations.
pub struct DefenseInDepthManager {
    config: RootCauseConfig,
}

impl DefenseInDepthManager {
    pub fn new(config: RootCauseConfig) -> Self {
        Self { config }
    }

    /// Generate defense recommendations from a trace chain's root cause.
    pub fn generate_recommendations(&self, chain: &TraceChain) -> Vec<DefenseRecommendation> {
        let root = match chain.root_level() {
            Some(r) => r,
            None => return vec![],
        };

        let mut recommendations = Vec::new();

        // Layer 1: Entry validation — validate inputs/preconditions at entry point
        recommendations.push(DefenseRecommendation {
            layer: DefenseLayer::EntryValidation,
            title: "入口参数校验".to_string(),
            description: format!(
                "在根因位置 '{}' 添加输入参数校验和前置条件检查，确保调用时参数在预期范围内",
                root.source_location
            ),
            priority: 1,
        });

        // Layer 2: Business logic — add defensive checks in business logic
        recommendations.push(DefenseRecommendation {
            layer: DefenseLayer::BusinessLogic,
            title: "业务逻辑防御".to_string(),
            description: format!(
                "围绕 '{}' 添加业务逻辑层防御性检查，包括空值保护、边界校验和状态验证",
                root.description.chars().take(50).collect::<String>()
            ),
            priority: 2,
        });

        // Layer 3: Environment guard — add guard clauses for environmental issues
        recommendations.push(DefenseRecommendation {
            layer: DefenseLayer::EnvironmentGuard,
            title: "环境异常防护".to_string(),
            description: "添加环境检查保护（磁盘空间、网络连通性、服务健康检查等），在操作前验证环境就绪".to_string(),
            priority: 3,
        });

        // Layer 4: Instrumentation — add logging and monitoring
        recommendations.push(DefenseRecommendation {
            layer: DefenseLayer::Instrumentation,
            title: "关键路径监控".to_string(),
            description: format!(
                "在根因路径 '{}' 添加追踪日志和性能监控，便于未来快速定位类似问题",
                root.source_location
            ),
            priority: 4,
        });

        recommendations
    }

    /// Generate targeted recommendations based on the specific root cause label
    pub fn targeted_recommendations(&self, chain: &TraceChain) -> Vec<DefenseRecommendation> {
        let root = match chain.root_level() {
            Some(r) => r,
            None => return vec![],
        };

        let label = root.evidence.value.get("root_cause_label")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match label {
            "network_error" => vec![
                DefenseRecommendation { layer: DefenseLayer::EntryValidation, title: "连接超时重试".into(), description: "添加指数退避重试机制".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::Instrumentation, title: "网络探测".into(), description: "添加连接前的网络健康检查".into(), priority: 2 },
            ],
            "resource_not_found" => vec![
                DefenseRecommendation { layer: DefenseLayer::EntryValidation, title: "路径校验".into(), description: "访问资源前验证路径/URL存在".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::BusinessLogic, title: "降级处理".into(), description: "资源不存在时提供降级替代方案".into(), priority: 2 },
            ],
            "permission_error" => vec![
                DefenseRecommendation { layer: DefenseLayer::BusinessLogic, title: "权限预检".into(), description: "操作前检查当前权限是否满足需求".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::EnvironmentGuard, title: "提权提示".into(), description: "权限不足时给出明确的提权建议".into(), priority: 2 },
            ],
            "null_reference" => vec![
                DefenseRecommendation { layer: DefenseLayer::EntryValidation, title: "空值保护".into(), description: "在解引用前检查是否为 null/None".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::BusinessLogic, title: "缺省值".into(), description: "为可能为空的字段提供安全的缺省值".into(), priority: 2 },
            ],
            "resource_exhausted" => vec![
                DefenseRecommendation { layer: DefenseLayer::EnvironmentGuard, title: "资源监控".into(), description: "操作前检查资源使用率，超过阈值时预警".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::BusinessLogic, title: "限流熔断".into(), description: "添加限流和熔断机制防止雪崩".into(), priority: 2 },
            ],
            "invalid_input" => vec![
                DefenseRecommendation { layer: DefenseLayer::EntryValidation, title: "参数校验".into(), description: "在入口处对参数做完整校验".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::Instrumentation, title: "参数日志".into(), description: "记录关键参数值便于调试".into(), priority: 2 },
            ],
            "syntax_error" => vec![
                DefenseRecommendation { layer: DefenseLayer::EntryValidation, title: "格式校验".into(), description: "在解析前验证输入格式".into(), priority: 1 },
                DefenseRecommendation { layer: DefenseLayer::BusinessLogic, title: "格式规范".into(), description: "输入格式不符合预期时给出明确错误信息".into(), priority: 2 },
            ],
            _ => self.generate_recommendations(chain),
        }
    }

    /// Prioritize recommendations based on chain confidence
    pub fn prioritize(&self, recommendations: &mut [DefenseRecommendation], chain: &TraceChain) {
        let confidence = chain.root_level()
            .map(|r| r.evidence.confidence)
            .unwrap_or(0.5);
        for rec in recommendations.iter_mut() {
            // Lower priority number = more urgent
            if confidence < 0.6 {
                rec.priority = rec.priority.saturating_sub(1);
            }
        }
        recommendations.sort_by_key(|r| r.priority);
    }

    /// Summary of all recommendations
    pub fn summary(&self, recommendations: &[DefenseRecommendation]) -> String {
        if recommendations.is_empty() {
            return "无防御建议".to_string();
        }
        let mut s = String::from("防御深度建议:\n");
        for rec in recommendations {
            s.push_str(&format!(
                "  [{}.{}] {}: {}\n",
                rec.layer.name(), rec.priority, rec.title, rec.description
            ));
        }
        s
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_chain_with_root(root_label: &str) -> TraceChain {
        let mut chain = TraceChain::new("defense_test", "agent_1");
        chain.add_level(super::super::types::TraceLevel {
            level: 1, label: "symptom".into(),
            description: "error".into(),
            source_location: "src/main.rs:1".into(),
            is_root_cause: false,
            evidence: super::super::types::Evidence::new("src/main.rs:1", json!("error"), 0.9),
        });
        chain.add_level(super::super::types::TraceLevel {
            level: 5, label: "root_cause".into(),
            description: "network error".into(),
            source_location: "src/net.rs:50".into(),
            is_root_cause: true,
            evidence: super::super::types::Evidence::new(
                "src/net.rs:50",
                json!({"root_cause_label": root_label, "error": "test"}),
                0.85,
            ),
        });
        chain
    }

    #[test]
    fn test_generates_4_layers() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = make_chain_with_root("unknown");
        let recs = manager.generate_recommendations(&chain);
        assert_eq!(recs.len(), 4, "Should generate 4 defense layers");
        // Verify all 4 layers present
        let layers: Vec<_> = recs.iter().map(|r| r.layer).collect();
        assert!(layers.contains(&DefenseLayer::EntryValidation));
        assert!(layers.contains(&DefenseLayer::BusinessLogic));
        assert!(layers.contains(&DefenseLayer::EnvironmentGuard));
        assert!(layers.contains(&DefenseLayer::Instrumentation));
    }

    #[test]
    fn test_targeted_network_error() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = make_chain_with_root("network_error");
        let recs = manager.targeted_recommendations(&chain);
        assert_eq!(recs.len(), 2);
        assert!(recs[0].title.contains("重试"));
    }

    #[test]
    fn test_targeted_null_reference() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = make_chain_with_root("null_reference");
        let recs = manager.targeted_recommendations(&chain);
        assert_eq!(recs.len(), 2);
        assert!(recs[0].title.contains("空值保护"));
    }

    #[test]
    fn test_empty_chain_returns_empty() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = TraceChain::new("empty", "agent_1");
        let recs = manager.generate_recommendations(&chain);
        assert!(recs.is_empty());
    }

    #[test]
    fn test_prioritize_sorts_by_priority() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = make_chain_with_root("unknown");
        let mut recs = manager.generate_recommendations(&chain);
        manager.prioritize(&mut recs, &chain);
        for i in 1..recs.len() {
            assert!(recs[i-1].priority <= recs[i].priority,
                "Priorities should be sorted ascending");
        }
    }

    #[test]
    fn test_summary_format() {
        let manager = DefenseInDepthManager::new(RootCauseConfig::default());
        let chain = make_chain_with_root("network_error");
        let recs = manager.generate_recommendations(&chain);
        let summary = manager.summary(&recs);
        assert!(summary.contains("防御深度建议"));
        assert!(summary.contains("入口校验"));
    }
}
