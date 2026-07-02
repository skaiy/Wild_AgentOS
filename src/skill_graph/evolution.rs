#![allow(deprecated)]

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::causal::engine::CausalEngine;
use crate::causal::types::CausalObservation;
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
    // P1-3: Causal failure analysis (legacy)
    causal_model: SkillCausalModel,
    event_history: VecDeque<CausalEvent>,
    max_events: usize,
    /// Optional CausalEngine for graph-backend-based root cause inference.
    /// When set, `analyze_failure()` delegates to `CausalEngine.infer_root_cause()`
    /// instead of the inline prerequisite-link scan.
    causal_engine: Option<CausalEngine>,
}

impl SkillEvolutionEngine {
    pub fn new(graph_store: Arc<SkillGraphStore>) -> Self {
        Self {
            graph_store,
            usage_history: Vec::new(),
            pending_suggestions: Vec::new(),
            causal_model: SkillCausalModel::new(),
            event_history: VecDeque::new(),
            max_events: 10_000,
            causal_engine: None,
        }
    }

    /// Enable causal analysis with configurable event history size.
    pub fn with_causal_analysis(mut self, max_events: usize) -> Self {
        self.max_events = max_events;
        self
    }

    /// Attach a CausalEngine for graph-backend-based root cause inference.
    pub fn with_causal_engine(mut self, engine: CausalEngine) -> Self {
        self.causal_engine = Some(engine);
        self
    }

    pub fn record_usage(&mut self, record: UsageRecord) -> Result<(), CoreError> {
        info!(
            "Recording skill usage: {} (success={}, tokens={})",
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

    /// P1-3: Causal failure analysis.
    ///
    /// When a `CausalEngine` is attached (via `with_causal_engine()`), delegates
    /// to its graph-backend-based traversal for root cause inference. Otherwise
    /// falls back to the legacy inline prerequisite-link scan.
    fn analyze_failure(
        &mut self,
        skill_iri: &str,
        error: &str,
        task_iri: &str,
        agent_id: &str,
    ) {
        debug!("Analyzing skill failure (causal): {} - {}", skill_iri, error);

        let error_hash = self.compute_error_signature(error);
        let error_class = self.classify_error(error);

        let event_id = format!("event:{}", uuid::Uuid::new_v4());

        // ── Path A: CausalEngine delegate ──
        if let Some(ref ce) = self.causal_engine {
            let obs = CausalObservation::new(&event_id, skill_iri, &error_class, &error_hash)
                .with_context("task_iri", task_iri)
                .with_context("agent_id", agent_id);
            ce.record_observation(obs.clone());

            let root_cause = ce.infer_root_cause(&[obs], 1);
            if let Some(inference) = root_cause.first() {
                let propagation_from = inference
                    .propagation_paths
                    .first()
                    .and_then(|path| path.hops.first())
                    .filter(|hop| hop.skill_iri != skill_iri)
                    .map(|hop| hop.skill_iri.clone());

                let prop_ref = propagation_from.clone();

                let event = CausalEvent {
                    event_id,
                    timestamp: Utc::now(),
                    skill_iri: skill_iri.to_string(),
                    error_class: error_class.clone(),
                    error_signature: error_hash.clone(),
                    context: {
                        let mut ctx = HashMap::new();
                        ctx.insert("task_iri".to_string(), task_iri.to_string());
                        ctx.insert("agent_id".to_string(), agent_id.to_string());
                        ctx
                    },
                    propagation_from: propagation_from,
                };

                if let Some(ref prop) = prop_ref {
                    self.causal_model.record_propagation(prop, skill_iri);
                } else {
                    self.causal_model.record_failure(skill_iri, &error_hash);
                }
                self.push_event(event);

                self.pending_suggestions.push(EvolutionSuggestion {
                    suggestion_type: EvolutionSuggestionType::CreateFragment,
                    skill_iri: skill_iri.to_string(),
                    description: format!(
                        "Causal failure in {}: {} (class={}, conf={:.2})",
                        skill_iri, error, error_class, inference.confidence
                    ),
                    confidence: inference.confidence,
                });
                return;
            }
        }

        // ── Path B: Legacy inline analysis (fallback) ──
        let event = CausalEvent {
            event_id,
            timestamp: Utc::now(),
            skill_iri: skill_iri.to_string(),
            error_class: error_class.clone(),
            error_signature: error_hash.clone(),
            context: {
                let mut ctx = HashMap::new();
                ctx.insert("task_iri".to_string(), task_iri.to_string());
                ctx.insert("agent_id".to_string(), agent_id.to_string());
                ctx
            },
            propagation_from: None,
        };

        // Check if any dependency failed recently (within 60 seconds)
        if let Some(skill) = self.graph_store.get_skill(skill_iri) {
            for link in &skill.links {
                if link.link_type == SkillLinkType::Prerequisite {
                    for past_event in self.event_history.iter().rev() {
                        if past_event.skill_iri == link.target_iri
                            && (Utc::now() - past_event.timestamp).num_seconds() < 60
                        {
                            self.causal_model
                                .record_propagation(&link.target_iri, skill_iri);
                            let mut propagated = event.clone();
                            propagated.propagation_from =
                                Some(past_event.event_id.clone());
                            self.push_event(propagated);
                            return;
                        }
                    }
                }
            }
        }

        // No propagation found — treat as potential root cause
        self.causal_model.record_failure(skill_iri, &error_hash);
        self.push_event(event);

        self.pending_suggestions.push(EvolutionSuggestion {
            suggestion_type: EvolutionSuggestionType::CreateFragment,
            skill_iri: skill_iri.to_string(),
            description: format!("Causal failure in {}: {} (class={})", skill_iri, error, error_class),
            confidence: 0.7,
        });
    }

    fn compute_error_signature(&self, error: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(error.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn classify_error(&self, error: &str) -> String {
        let lower = error.to_lowercase();
        if lower.contains("timeout") || lower.contains("timed out") {
            "timeout".to_string()
        } else if lower.contains("permission") || lower.contains("denied") || lower.contains("forbidden") {
            "permission".to_string()
        } else if lower.contains("not found") || lower.contains("404") {
            "not_found".to_string()
        } else if lower.contains("network") || lower.contains("connection") {
            "network".to_string()
        } else if lower.contains("parse") || lower.contains("syntax") || lower.contains("invalid") {
            "validation".to_string()
        } else if lower.contains("rate") || lower.contains("limit") || lower.contains("quota") {
            "rate_limit".to_string()
        } else {
            "unknown".to_string()
        }
    }

    fn push_event(&mut self, event: CausalEvent) {
        if self.event_history.len() >= self.max_events {
            self.event_history.pop_front();
        }
        self.event_history.push_back(event);
    }

    /// Trace back from an event to find the root cause chain.
    pub fn find_root_cause(&self, event_id: &str) -> Option<CausalChain> {
        let event = self.event_history.iter().find(|e| e.event_id == event_id)?;

        let mut path = vec![event.clone()];
        let mut current = event;

        while let Some(ref from_id) = current.propagation_from {
            if let Some(parent) = self.event_history.iter().find(|e| e.event_id == *from_id) {
                path.push(parent.clone());
                current = parent;
            } else {
                break;
            }
        }

        path.reverse();
        let root = path.remove(0);
        let confidence = if path.len() <= 2 {
            0.9
        } else {
            0.9 - ((path.len() as f32 - 2.0) * 0.1).max(0.0)
        };

        Some(CausalChain {
            root_cause: root,
            propagation_path: path,
            confidence,
        })
    }

    /// Recommend preventive actions for a skill based on its causal history.
    pub fn suggest_preventive_action(&self, skill_iri: &str) -> Vec<String> {
        let mut actions = Vec::new();

        // Check error profiles
        if let Some(profiles) = self.causal_model.error_profiles.get(skill_iri) {
            let total: u32 = profiles.values().sum();
            if total > 5 {
                actions.push(format!(
                    "Skill {} has {} recorded failures — consider adding knowledge fragments",
                    skill_iri, total
                ));
            }

            for (error_sig, count) in profiles.iter() {
                if *count > 3 {
                    let display = &error_sig[..error_sig.len().min(16)];
                    actions.push(format!(
                        "Frequent error pattern {} ({}) detected — investigate root cause",
                        display, count
                    ));
                }
            }
        }

        // Check propagation patterns
        let propagated_to: Vec<String> = self
            .event_history
            .iter()
            .filter(|e| {
                e.propagation_from
                    .as_ref()
                    .and_then(|from| {
                        self.event_history
                            .iter()
                            .find(|pe| pe.event_id == *from)
                    })
                    .map_or(false, |pe| pe.skill_iri == skill_iri)
            })
            .map(|e| e.skill_iri.clone())
            .collect();

        if !propagated_to.is_empty() {
            actions.push(format!(
                "Failures in {} propagate to {:?} — add guards before depending skills",
                skill_iri, propagated_to
            ));
        }

        actions
    }

    pub fn create_fragment(
        &self,
        skill_iri: &str,
        problem: &str,
        recommendation: &str,
        discoverer: &str,
    ) -> Result<KnowledgeFragment, CoreError> {
        info!("Creating knowledge fragment: {} -> {}", skill_iri, problem);

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
        info!("Suggested link: {} -> {} ({:?})", source_iri, target_iri, link_type);

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
        info!("Applying evolution suggestion: {:?}", suggestion.suggestion_type);

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
                debug!("Success rate update suggestion auto-processed");
            }
            EvolutionSuggestionType::CreateFragment => {
                debug!("Knowledge fragment creation suggestion requires manual confirmation");
            }
            EvolutionSuggestionType::Deprecate => {
                warn!("Skill deprecation suggestion requires manual confirmation: {}", suggestion.skill_iri);
            }
            EvolutionSuggestionType::Merge | EvolutionSuggestionType::Split => {
                warn!("Skill merge/split suggestion requires manual confirmation: {}", suggestion.skill_iri);
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
                recommendations: vec!["Skill not found".to_string()],
            }
        }
    }

    fn generate_health_recommendations(&self, skill: &SkillGraphNode) -> Vec<String> {
        let mut recommendations = Vec::new();

        if skill.graph_meta.usage_count == 0 {
            recommendations.push("Skill has not been used yet, consider testing it in a suitable scenario".to_string());
        }

        if skill.graph_meta.success_rate < 0.7 && skill.graph_meta.usage_count > 5 {
            recommendations.push("Success rate is low, consider reviewing skill implementation or adding knowledge fragments".to_string());
        }

        if skill.links.is_empty() {
            recommendations.push("Skill has no links, consider adding related skills or prerequisite dependencies".to_string());
        }

        if skill.graph_meta.known_failure_modes.len() > 3 {
            recommendations.push("Many known failure modes, consider splitting the skill or updating the implementation".to_string());
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

    pub async fn suggest_improvements(&mut self) -> Vec<EvolutionSuggestion> {
        let mut suggestions = Vec::new();

        for skill in self.graph_store.list_all_skills() {
            let health = self.analyze_skill_health(&skill.skill_iri);
            
            if health.status == HealthStatus::Unhealthy {
                suggestions.push(EvolutionSuggestion {
                    suggestion_type: EvolutionSuggestionType::Deprecate,
                    skill_iri: skill.skill_iri.clone(),
                    description: format!("Low skill health ({:.2}), consider deprecating or refactoring", health.health_score),
                    confidence: 0.6,
                });
            }

            let link_suggestions = self.graph_store.suggest_links(&skill.skill_iri, None).await;
            for (target, link_type, confidence) in link_suggestions {
                if confidence > 0.5 {
                    suggestions.push(EvolutionSuggestion {
                        suggestion_type: EvolutionSuggestionType::AddLink,
                        skill_iri: skill.skill_iri.clone(),
                        description: format!("Consider adding a link to {} ({:?})", target, link_type),
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

    #[tokio::test]
    async fn test_suggest_improvements() {
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
        
        let suggestions = engine.suggest_improvements().await;
        
        assert!(!suggestions.is_empty());
    }
}
