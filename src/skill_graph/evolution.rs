use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::skill_graph::graph_store::SkillGraphStore;
use crate::skill_graph::types::*;
use crate::CoreError;

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub skill_iri: String,
    pub task_iri: String,
    pub agent_id: String,
    pub success: bool,
    pub token_consumption: u32,
    pub duration_seconds: u32,
    pub error_message: Option<String>,
    pub context_tags: Vec<String>,
}

impl UsageRecord {
    pub fn new(skill_iri: &str, task_iri: &str, agent_id: &str, success: bool) -> Self {
        Self {
            skill_iri: skill_iri.to_string(),
            task_iri: task_iri.to_string(),
            agent_id: agent_id.to_string(),
            success,
            token_consumption: 0,
            duration_seconds: 0,
            error_message: None,
            context_tags: Vec::new(),
        }
    }

    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.token_consumption = tokens;
        self
    }

    pub fn with_duration(mut self, seconds: u32) -> Self {
        self.duration_seconds = seconds;
        self
    }

    pub fn with_error(mut self, error: &str) -> Self {
        self.error_message = Some(error.to_string());
        self
    }

    pub fn with_context_tag(mut self, tag: &str) -> Self {
        self.context_tags.push(tag.to_string());
        self
    }
}

#[derive(Debug, Clone)]
pub struct EvolutionSuggestion {
    pub suggestion_type: EvolutionSuggestionType,
    pub skill_iri: String,
    pub description: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub enum EvolutionSuggestionType {
    AddLink,
    UpdateSuccessRate,
    CreateFragment,
    Deprecate,
    Merge,
    Split,
}

pub struct SkillEvolutionEngine {
    graph_store: Arc<SkillGraphStore>,
    usage_history: Vec<UsageRecord>,
    pending_suggestions: Vec<EvolutionSuggestion>,
}

impl SkillEvolutionEngine {
    pub fn new(graph_store: Arc<SkillGraphStore>) -> Self {
        Self {
            graph_store,
            usage_history: Vec::new(),
            pending_suggestions: Vec::new(),
        }
    }

    pub fn record_usage(&mut self, record: UsageRecord) -> Result<(), CoreError> {
        info!(
            "记录技能使用: {} (success={}, tokens={})",
            record.skill_iri, record.success, record.token_consumption
        );

        self.graph_store.record_skill_usage(&record.skill_iri, record.success)?;

        if let Some(skill) = self.graph_store.get_skill(&record.skill_iri) {
            let mut skill = skill;
            let total_tokens = skill.graph_meta.avg_token_consumption * (skill.graph_meta.usage_count - 1)
                + record.token_consumption;
            skill.graph_meta.avg_token_consumption = total_tokens / skill.graph_meta.usage_count;
            
            self.graph_store.update_skill(skill)?;
        }

        if !record.success {
            if let Some(ref error) = record.error_message {
                self.analyze_failure(&record.skill_iri, error, &record.task_iri, &record.agent_id);
            }
        }

        self.usage_history.push(record);

        Ok(())
    }

    fn analyze_failure(
        &mut self,
        skill_iri: &str,
        error: &str,
        _task_iri: &str,
        _agent_id: &str,
    ) {
        debug!("分析技能失败: {} - {}", skill_iri, error);

        if let Some(skill) = self.graph_store.get_skill(skill_iri) {
            let similar_failures: Vec<_> = skill
                .graph_meta
                .known_failure_modes
                .iter()
                .filter(|fm| error.contains(&fm.mode))
                .collect();

            if similar_failures.is_empty() {
                self.pending_suggestions.push(EvolutionSuggestion {
                    suggestion_type: EvolutionSuggestionType::CreateFragment,
                    skill_iri: skill_iri.to_string(),
                    description: format!("新失败模式: {}", error),
                    confidence: 0.7,
                });
            }
        }
    }

    pub fn create_fragment(
        &self,
        skill_iri: &str,
        problem: &str,
        recommendation: &str,
        discoverer: &str,
    ) -> Result<KnowledgeFragment, CoreError> {
        info!("创建知识碎片: {} -> {}", skill_iri, problem);

        let fragment_count = self.graph_store.get_fragments_for_skill(skill_iri).len();
        let fragment_iri = format!("{}#fragment_{}", skill_iri, fragment_count + 1);

        self.graph_store.create_fragment(
            &fragment_iri,
            skill_iri,
            problem,
            recommendation,
            Some(discoverer),
        )
    }

    pub fn suggest_link(
        &mut self,
        source_iri: &str,
        target_iri: &str,
        link_type: SkillLinkType,
        description: &str,
    ) -> Result<(), CoreError> {
        info!("建议链接: {} -> {} ({:?})", source_iri, target_iri, link_type);

        if self.graph_store.get_skill(source_iri).is_none() {
            return Err(CoreError::SkillNotFound {
                iri: format!("Source skill not found: {}", source_iri),
            });
        }

        if self.graph_store.get_skill(target_iri).is_none() {
            return Err(CoreError::SkillNotFound {
                iri: format!("Target skill not found: {}", target_iri),
            });
        }

        self.pending_suggestions.push(EvolutionSuggestion {
            suggestion_type: EvolutionSuggestionType::AddLink,
            skill_iri: source_iri.to_string(),
            description: format!("{} -> {} ({:?}): {}", source_iri, target_iri, link_type, description),
            confidence: 0.8,
        });

        Ok(())
    }

    pub fn apply_suggestion(&mut self, suggestion: &EvolutionSuggestion) -> Result<(), CoreError> {
        info!("应用演化建议: {:?}", suggestion.suggestion_type);

        match suggestion.suggestion_type {
            EvolutionSuggestionType::AddLink => {
                let parts: Vec<&str> = suggestion.description.split(" -> ").collect();
                if parts.len() >= 2 {
                    let source = parts[0];
                    let rest = parts[1];
                    let target_end = rest.find(" (").unwrap_or(rest.len());
                    let target = &rest[..target_end];
                    
                    self.graph_store.add_link(
                        source,
                        target,
                        SkillLinkType::Related,
                        LinkStrength::Recommended,
                        &suggestion.description,
                    )?;
                }
            }
            EvolutionSuggestionType::UpdateSuccessRate => {
                debug!("成功率更新建议已自动处理");
            }
            EvolutionSuggestionType::CreateFragment => {
                debug!("知识碎片创建建议需要人工确认");
            }
            EvolutionSuggestionType::Deprecate => {
                warn!("技能弃用建议需要人工确认: {}", suggestion.skill_iri);
            }
            EvolutionSuggestionType::Merge | EvolutionSuggestionType::Split => {
                warn!("技能合并/拆分建议需要人工确认: {}", suggestion.skill_iri);
            }
        }

        Ok(())
    }

    pub fn get_pending_suggestions(&self) -> &[EvolutionSuggestion] {
        &self.pending_suggestions
    }

    pub fn clear_suggestions(&mut self) {
        self.pending_suggestions.clear();
    }

    pub fn analyze_skill_health(&self, skill_iri: &str) -> SkillHealthReport {
        let skill = self.graph_store.get_skill(skill_iri);
        
        if let Some(skill) = skill {
            let usage_count = skill.graph_meta.usage_count;
            let success_rate = skill.graph_meta.success_rate;
            let failure_modes = skill.graph_meta.known_failure_modes.len();
            let fragment_count = self.graph_store.get_fragments_for_skill(skill_iri).len();
            
            let health_score = if usage_count == 0 {
                0.5
            } else {
                let success_component = success_rate * 0.5;
                let usage_component = (usage_count as f32).min(10.0) / 10.0 * 0.3;
                let failure_penalty = (failure_modes as f32 * 0.05).min(0.2);
                (success_component + usage_component - failure_penalty).max(0.0).min(1.0)
            };

            let status = if health_score >= 0.8 {
                HealthStatus::Healthy
            } else if health_score >= 0.5 {
                HealthStatus::NeedsAttention
            } else {
                HealthStatus::Unhealthy
            };

            SkillHealthReport {
                skill_iri: skill_iri.to_string(),
                health_score,
                status,
                usage_count,
                success_rate,
                failure_modes,
                fragment_count,
                recommendations: self.generate_health_recommendations(&skill),
            }
        } else {
            SkillHealthReport {
                skill_iri: skill_iri.to_string(),
                health_score: 0.0,
                status: HealthStatus::NotFound,
                usage_count: 0,
                success_rate: 0.0,
                failure_modes: 0,
                fragment_count: 0,
                recommendations: vec!["技能未找到".to_string()],
            }
        }
    }

    fn generate_health_recommendations(&self, skill: &SkillGraphNode) -> Vec<String> {
        let mut recommendations = Vec::new();

        if skill.graph_meta.usage_count == 0 {
            recommendations.push("技能尚未被使用，考虑在合适场景中测试".to_string());
        }

        if skill.graph_meta.success_rate < 0.7 && skill.graph_meta.usage_count > 5 {
            recommendations.push("成功率较低，建议审查技能实现或添加知识碎片".to_string());
        }

        if skill.links.is_empty() {
            recommendations.push("技能没有链接，考虑添加相关技能或前置依赖".to_string());
        }

        if skill.graph_meta.known_failure_modes.len() > 3 {
            recommendations.push("已知失败模式较多，考虑拆分技能或更新实现".to_string());
        }

        recommendations
    }

    pub fn get_usage_stats(&self, skill_iri: &str) -> SkillUsageStats {
        let records: Vec<_> = self
            .usage_history
            .iter()
            .filter(|r| r.skill_iri == skill_iri)
            .collect();

        let total_usage = records.len() as u32;
        let successful = records.iter().filter(|r| r.success).count() as u32;
        let failed = total_usage - successful;
        let avg_tokens = if total_usage > 0 {
            records.iter().map(|r| r.token_consumption).sum::<u32>() / total_usage
        } else {
            0
        };
        let avg_duration = if total_usage > 0 {
            records.iter().map(|r| r.duration_seconds).sum::<u32>() / total_usage
        } else {
            0
        };

        SkillUsageStats {
            skill_iri: skill_iri.to_string(),
            total_usage,
            successful,
            failed,
            success_rate: if total_usage > 0 {
                successful as f32 / total_usage as f32
            } else {
                0.0
            },
            avg_tokens,
            avg_duration_seconds: avg_duration,
        }
    }

    pub fn suggest_improvements(&mut self) -> Vec<EvolutionSuggestion> {
        let mut suggestions = Vec::new();

        for skill in self.graph_store.list_all_skills() {
            let health = self.analyze_skill_health(&skill.skill_iri);
            
            if health.status == HealthStatus::Unhealthy {
                suggestions.push(EvolutionSuggestion {
                    suggestion_type: EvolutionSuggestionType::Deprecate,
                    skill_iri: skill.skill_iri.clone(),
                    description: format!("技能健康度低 ({:.2})，考虑弃用或重构", health.health_score),
                    confidence: 0.6,
                });
            }

            let link_suggestions = self.graph_store.suggest_links(&skill.skill_iri);
            for (target, link_type, confidence) in link_suggestions {
                if confidence > 0.5 {
                    suggestions.push(EvolutionSuggestion {
                        suggestion_type: EvolutionSuggestionType::AddLink,
                        skill_iri: skill.skill_iri.clone(),
                        description: format!("建议添加链接到 {} ({:?})", target, link_type),
                        confidence,
                    });
                }
            }
        }

        suggestions
    }
}

#[derive(Debug, Clone)]
pub struct SkillHealthReport {
    pub skill_iri: String,
    pub health_score: f32,
    pub status: HealthStatus,
    pub usage_count: u32,
    pub success_rate: f32,
    pub failure_modes: usize,
    pub fragment_count: usize,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    NeedsAttention,
    Unhealthy,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct SkillUsageStats {
    pub skill_iri: String,
    pub total_usage: u32,
    pub successful: u32,
    pub failed: u32,
    pub success_rate: f32,
    pub avg_tokens: u32,
    pub avg_duration_seconds: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_store() -> Arc<SkillGraphStore> {
        let store = Arc::new(SkillGraphStore::new());
        
        let skill = SkillGraphNode::new(
            "iri://skills/test-skill",
            "Test Skill",
            "A test skill",
        );
        
        store.register_skill(skill).unwrap();
        store
    }

    #[test]
    fn test_record_usage() {
        let store = setup_test_store();
        let mut engine = SkillEvolutionEngine::new(store);
        
        let record = UsageRecord::new(
            "iri://skills/test-skill",
            "iri://task/001",
            "agent:da/001",
            true,
        ).with_tokens(1500);
        
        engine.record_usage(record).unwrap();
        
        let stats = engine.get_usage_stats("iri://skills/test-skill");
        assert_eq!(stats.total_usage, 1);
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.avg_tokens, 1500);
    }

    #[test]
    fn test_record_failure() {
        let store = setup_test_store();
        let mut engine = SkillEvolutionEngine::new(store);
        
        let record = UsageRecord::new(
            "iri://skills/test-skill",
            "iri://task/001",
            "agent:da/001",
            false,
        ).with_error("Token expired");
        
        engine.record_usage(record).unwrap();
        
        let stats = engine.get_usage_stats("iri://skills/test-skill");
        assert_eq!(stats.failed, 1);
        assert!(!engine.pending_suggestions.is_empty());
    }

    #[test]
    fn test_create_fragment() {
        let store = setup_test_store();
        let engine = SkillEvolutionEngine::new(store);
        
        let fragment = engine.create_fragment(
            "iri://skills/test-skill",
            "Token expiration",
            "Use refresh tokens",
            "agent:ca/001",
        ).unwrap();
        
        assert_eq!(fragment.problem, "Token expiration");
        assert_eq!(fragment.recommendation, "Use refresh tokens");
    }

    #[test]
    fn test_analyze_skill_health() {
        let store = setup_test_store();
        let mut engine = SkillEvolutionEngine::new(store);
        
        for _ in 0..10 {
            let record = UsageRecord::new(
                "iri://skills/test-skill",
                "iri://task/001",
                "agent:da/001",
                true,
            ).with_tokens(1000);
            engine.record_usage(record).unwrap();
        }
        
        let health = engine.analyze_skill_health("iri://skills/test-skill");
        
        assert!(health.health_score > 0.0);
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_suggest_link() {
        let store = setup_test_store();
        
        let skill2 = SkillGraphNode::new(
            "iri://skills/related-skill",
            "Related Skill",
            "A related skill",
        );
        store.register_skill(skill2).unwrap();
        
        let mut engine = SkillEvolutionEngine::new(store);
        
        engine.suggest_link(
            "iri://skills/test-skill",
            "iri://skills/related-skill",
            SkillLinkType::Related,
            "Often used together",
        ).unwrap();
        
        assert!(!engine.pending_suggestions.is_empty());
    }

    #[test]
    fn test_suggest_improvements() {
        let store = setup_test_store();
        let mut engine = SkillEvolutionEngine::new(store);
        
        for _ in 0..20 {
            let record = UsageRecord::new(
                "iri://skills/test-skill",
                "iri://task/001",
                "agent:da/001",
                false,
            ).with_error("Consistent failure").with_tokens(100);
            engine.record_usage(record).unwrap();
        }
        
        let suggestions = engine.suggest_improvements();
        
        assert!(!suggestions.is_empty());
    }
}
