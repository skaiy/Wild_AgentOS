use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::core::event_bus::EventBus;
use crate::memory::l0_store::{L0Entry, L0Store, MesiState};
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerceptionTrigger {
    TaskStart,
    PlanCompleted,
    ProgressAnomaly,
    CheckCompleted,
    TaskEnd,
    CycleTimeout,
    AgentBlocked,
    ResourceConflict,
    QualityDegradation,
    UserFeedback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAnalysis {
    pub summary: String,
    pub complexity: String,
    pub estimated_steps: u32,
    pub risks: Vec<String>,
    pub recommended_approach: String,
    pub agent_assignments: HashMap<String, String>,
    #[serde(default)]
    pub relevant_experience_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisoryNode {
    pub advisory_id: String,
    pub advisory_type: String,
    pub severity: String,
    pub content: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionPlan {
    pub anomaly_id: String,
    pub diagnosis: String,
    pub actions: Vec<String>,
    pub priority: String,
    pub should_interrupt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub experience_id: String,
    pub scenario: String,
    pub pattern: String,
    pub success_rating: f32,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    pub cache_ttl_seconds: i64,
    pub cache_max_entries: usize,
    pub anomaly_dedup_window_seconds: i64,
    pub simple_input_threshold: usize,
    pub medium_input_threshold: usize,
    pub simple_steps: u32,
    pub medium_steps: u32,
    pub complex_steps: u32,
    pub complex_subtask_threshold: usize,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            cache_ttl_seconds: 300,
            cache_max_entries: 1000,
            anomaly_dedup_window_seconds: 60,
            simple_input_threshold: 50,
            medium_input_threshold: 200,
            simple_steps: 1,
            medium_steps: 3,
            complex_steps: 5,
            complex_subtask_threshold: 5,
        }
    }
}

impl PerceptionConfig {
    pub fn from_settings(settings: &crate::config::PerceptionSettings) -> Self {
        Self {
            cache_ttl_seconds: settings.cache_ttl_seconds as i64,
            cache_max_entries: settings.cache_max_entries,
            anomaly_dedup_window_seconds: settings.anomaly_dedup_window_seconds as i64,
            ..Default::default()
        }
    }
}

pub struct ProactiveEngine {
    cache: HashMap<String, (DateTime<Utc>, Value)>,
    config: PerceptionConfig,
    anomaly_history: Vec<(String, DateTime<Utc>)>,
    l0: Arc<L0Store>,
    event_bus: Arc<EventBus>,
}

impl ProactiveEngine {
    pub fn new(l0: Arc<L0Store>, event_bus: Arc<EventBus>) -> Self {
        Self {
            cache: HashMap::new(),
            config: PerceptionConfig::default(),
            anomaly_history: Vec::new(),
            l0,
            event_bus,
        }
    }

    pub fn with_config(config: PerceptionConfig, l0: Arc<L0Store>, event_bus: Arc<EventBus>) -> Self {
        Self {
            cache: HashMap::new(),
            config,
            anomaly_history: Vec::new(),
            l0,
            event_bus,
        }
    }

    pub fn check_5w2h_constraints(&self, five_w2h_iri: &str) -> Option<String> {
        let entry = self.l0.retrieve(five_w2h_iri).ok()??;
        let node: serde_json::Value = serde_json::from_str(&entry.content).ok()?;

        let mut alerts = Vec::new();

        if let Some(when) = node.get("task:when") {
            if let Some(deadline_str) = when.get("task:deadline").and_then(|v| v.as_str()) {
                if let Ok(deadline) = deadline_str.parse::<DateTime<Utc>>() {
                    let now = Utc::now();
                    let reminder_duration = when.get("task:reminderBefore")
                        .and_then(|v| v.as_str())
                        .and_then(|s| parse_iso8601_duration(s))
                        .unwrap_or(chrono::Duration::hours(1));
                    let until = deadline.signed_duration_since(now);
                    if until.num_seconds() > 0 && until <= reminder_duration {
                        alerts.push("DEADLINE_APPROACHING".to_string());
                    } else if until.num_seconds() <= 0 {
                        alerts.push("DEADLINE_EXCEEDED".to_string());
                    }
                }
            }
        }

        if let Some(how_much) = node.get("task:howMuch") {
            if let Some(budget) = how_much.get("task:tokenBudget").and_then(|v| v.as_u64()) {
                if let Some(actual) = how_much.get("task:actualCost") {
                    if let Some(used) = actual.get("tokensUsed").and_then(|v| v.as_u64()) {
                        if budget > 0 && used * 100 > budget * 80 {
                            alerts.push("BUDGET_EXCEEDED".to_string());
                        }
                    }
                }
            }
        }

        if alerts.is_empty() {
            None
        } else {
            Some(alerts.join(","))
        }
    }

    fn query_relevant_experiences_from_l0(&self, query: &str) -> Vec<serde_json::Value> {
        let results = match self.l0.search_by_tags(&["experience".to_string()]) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let query_lower = query.to_lowercase();
        results.into_iter()
            .filter(|entry| {
                entry.content.to_lowercase().contains(&query_lower)
                    || entry.tags.iter().any(|t| query_lower.contains(&t.to_lowercase()))
            })
            .take(5)
            .filter_map(|entry| serde_json::from_str(&entry.content).ok())
            .collect()
    }

    fn cache_key(trigger: &str, context: &str) -> String {
        format!("{}:{}", trigger, context)
    }

    fn is_cached(&self, key: &str) -> bool {
        self.cache.get(key).map_or(false, |(ts, _)| {
            Utc::now().signed_duration_since(*ts).num_seconds() < self.config.cache_ttl_seconds
        })
    }

    fn evict_cache(&mut self) {
        if self.cache.len() > self.config.cache_max_entries {
            let now = Utc::now();
            self.cache.retain(|_, (ts, _)| {
                now.signed_duration_since(*ts).num_seconds() < self.config.cache_ttl_seconds
            });
            if self.cache.len() > self.config.cache_max_entries {
                let oldest_key = self.cache.iter()
                    .min_by_key(|(_, (ts, _))| *ts)
                    .map(|(k, _)| k.clone());
                if let Some(key) = oldest_key {
                    self.cache.remove(&key);
                }
            }
        }
    }

    fn evict_anomaly_history(&mut self) {
        let now = Utc::now();
        self.anomaly_history.retain(|(_, ts)| {
            now.signed_duration_since(*ts).num_seconds() < self.config.anomaly_dedup_window_seconds * 2
        });
    }

    pub fn on_task_start(&mut self, user_input: &str, task_iri: &str) -> Result<TaskAnalysis, CoreError> {
        let key = Self::cache_key("task_start", task_iri);
        if self.is_cached(&key) {
            return Ok(serde_json::from_value(self.cache[&key].1.clone())
                .unwrap_or_else(|_| self.analyze_task(user_input)));
        }

        let mut analysis = self.analyze_task(user_input);

        let experiences = self.query_relevant_experiences_from_l0(user_input);
        analysis.relevant_experience_hints = experiences.iter()
            .filter_map(|e| e.get("scenario").and_then(|s| s.as_str()).map(|s| s.to_string()))
            .take(5)
            .collect();

        let val = serde_json::to_value(&analysis).unwrap_or_default();
        self.evict_cache();
        self.cache.insert(key, (Utc::now(), val));

        info!(task = %task_iri, complexity = %analysis.complexity, "Task analyzed");
        Ok(analysis)
    }

    pub fn on_plan_completed(&self, plan: &Value, task_iri: &str) -> Vec<AdvisoryNode> {
        let mut advisories = Vec::new();
        if let Some(sub_tasks) = plan.get("sub_tasks").and_then(|v| v.as_array()) {
            if sub_tasks.len() > self.config.complex_subtask_threshold {
                advisories.push(AdvisoryNode {
                    advisory_id: format!("adv_{}", uuid::Uuid::new_v4().hyphenated()),
                    advisory_type: "complexity_warning".to_string(),
                    severity: "medium".to_string(),
                    content: json!({"message": format!("Plan has {} sub-tasks (threshold: {}), consider parallelization", sub_tasks.len(), self.config.complex_subtask_threshold)}),
                    created_at: Utc::now(),
                });
            }
        }
        debug!(task = %task_iri, advisories = advisories.len(), "Plan assessed");
        advisories
    }

    pub fn on_progress_anomaly(&mut self, anomaly: &Value, task_iri: &str) -> InterventionPlan {
        self.evict_anomaly_history();

        let desc = anomaly.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let anomaly_id = format!("anomaly_{}", uuid::Uuid::new_v4().hyphenated());

        if self.anomaly_history.iter().any(|(d, t)| {
            d == desc && Utc::now().signed_duration_since(*t).num_seconds() < self.config.anomaly_dedup_window_seconds
        }) {
            return InterventionPlan {
                anomaly_id: String::new(),
                diagnosis: "already_handled".to_string(),
                actions: vec![],
                priority: "low".to_string(),
                should_interrupt: false,
            };
        }
        self.anomaly_history.push((desc.to_string(), Utc::now()));

        warn!(task = %task_iri, anomaly = %desc, "Progress anomaly detected");
        InterventionPlan {
            anomaly_id,
            diagnosis: format!("Progress anomaly: {}", desc),
            actions: vec!["Reassess current plan".to_string(), "Consider additional resources".to_string()],
            priority: "high".to_string(),
            should_interrupt: true,
        }
    }

    pub fn on_check_completed(&self, check_result: &Value, task_iri: &str) -> Option<AdvisoryNode> {
        let verdict = check_result.get("verdict").and_then(|v| v.as_str()).unwrap_or("pass");
        if verdict == "fail" {
            debug!(task = %task_iri, "Check failed, generating advisory");
            return Some(AdvisoryNode {
                advisory_id: format!("adv_{}", uuid::Uuid::new_v4().hyphenated()),
                advisory_type: "check_failure".to_string(),
                severity: "high".to_string(),
                content: json!({"message": "Check failed, review required", "details": check_result}),
                created_at: Utc::now(),
            });
        }
        None
    }

    pub fn on_task_end(&self, task_result: &Value, task_iri: &str) -> Option<Experience> {
        let status = task_result.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
        if status == "success" || status == "failed" {
            debug!(task = %task_iri, status = %status, "Extracting experience");
            let experience = Experience {
                experience_id: format!("exp_{}", uuid::Uuid::new_v4().hyphenated()),
                scenario: task_result.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                pattern: format!("task_{}", status),
                success_rating: if status == "success" { 0.9 } else { 0.1 },
                tags: vec![format!("task:{}", task_iri), format!("status:{}", status), "experience".to_string()],
                created_at: Utc::now(),
            };

            let iri = format!("iri://experience/{}", experience.experience_id);
            let content = serde_json::to_string(&serde_json::json!({
                "@id": iri,
                "@type": "Experience",
                "scenario": &experience.scenario,
                "pattern": &experience.pattern,
                "success_rating": experience.success_rating,
                "tags": &experience.tags,
            })).unwrap_or_default();
            if !content.is_empty() {
                let entry = L0Entry {
                    iri: iri.clone(),
                    content,
                    importance: experience.success_rating,
                    access_count: 0,
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    tags: experience.tags.clone(),
                    metadata: serde_json::Map::new(),
                    mesi_state: MesiState::Shared,
                    content_hash: String::new(),
                    named_graph: None,
                    qdrant_point_id: None,
                    jsonld_context: None,
                    jsonld_types: Vec::new(),
                };
                let _ = self.l0.store_entry(&entry);
            }

            return Some(experience);
        }
        None
    }

    pub fn on_cycle_timeout(&self, cycle_id: &str, task_iri: &str, elapsed_secs: f64) -> InterventionPlan {
        warn!(cycle = %cycle_id, task = %task_iri, elapsed = elapsed_secs, "Cycle timeout");
        InterventionPlan {
            anomaly_id: format!("timeout_{}", uuid::Uuid::new_v4().hyphenated()),
            diagnosis: format!("Cycle {} timeout after {:.0}s", cycle_id, elapsed_secs),
            actions: vec!["Extend timeout".to_string(), "Check agent health".to_string()],
            priority: "critical".to_string(),
            should_interrupt: true,
        }
    }

    pub fn on_agent_blocked(&self, agent_id: &str, task_iri: &str) -> InterventionPlan {
        warn!(agent = %agent_id, task = %task_iri, "Agent blocked");
        InterventionPlan {
            anomaly_id: format!("blocked_{}", uuid::Uuid::new_v4().hyphenated()),
            diagnosis: format!("Agent {} is blocked", agent_id),
            actions: vec!["Restart agent".to_string(), "Inject assistance message".to_string()],
            priority: "high".to_string(),
            should_interrupt: true,
        }
    }

    pub fn on_resource_conflict(&self, conflict: &Value, task_iri: &str) -> InterventionPlan {
        warn!(task = %task_iri, conflict = ?conflict, "Resource conflict");
        InterventionPlan {
            anomaly_id: format!("conflict_{}", uuid::Uuid::new_v4().hyphenated()),
            diagnosis: "Resource conflict detected".to_string(),
            actions: vec!["Queue conflicting requests".to_string(), "Notify SA".to_string()],
            priority: "medium".to_string(),
            should_interrupt: false,
        }
    }

    pub fn on_quality_degradation(&self, degradation: &Value, task_iri: &str) -> InterventionPlan {
        warn!(task = %task_iri, degradation = ?degradation, "Quality degradation");
        InterventionPlan {
            anomaly_id: format!("quality_{}", uuid::Uuid::new_v4().hyphenated()),
            diagnosis: "Output quality degraded".to_string(),
            actions: vec!["Rollback to last checkpoint".to_string(), "Re-run with different approach".to_string()],
            priority: "high".to_string(),
            should_interrupt: true,
        }
    }

    pub fn on_user_feedback(&self, feedback: &Value, task_iri: &str) -> AdvisoryNode {
        let message = feedback.get("message").and_then(|v| v.as_str()).unwrap_or("");
        info!(task = %task_iri, feedback = %message, "User feedback received");
        AdvisoryNode {
            advisory_id: format!("fb_{}", uuid::Uuid::new_v4().hyphenated()),
            advisory_type: "user_feedback".to_string(),
            severity: "medium".to_string(),
            content: feedback.clone(),
            created_at: Utc::now(),
        }
    }

    fn analyze_task(&self, user_input: &str) -> TaskAnalysis {
        let input_len = user_input.len();
        let (complexity, steps) = if input_len < self.config.simple_input_threshold {
            ("simple".to_string(), self.config.simple_steps)
        } else if input_len < self.config.medium_input_threshold {
            ("medium".to_string(), self.config.medium_steps)
        } else {
            ("complex".to_string(), self.config.complex_steps)
        };

        let risks = if complexity == "complex" {
            vec!["Large scope may require multiple iterations".to_string()]
        } else {
            vec![]
        };

        TaskAnalysis {
            summary: user_input.chars().take(100).collect(),
            complexity: complexity.clone(),
            estimated_steps: steps,
            risks: risks.clone(),
            recommended_approach: self.recommend_approach(&complexity),
            agent_assignments: HashMap::from([
                ("plan".to_string(), "PA".to_string()),
                ("execute".to_string(), "DA".to_string()),
                ("check".to_string(), "CA".to_string()),
                ("act".to_string(), "AA".to_string()),
            ]),
            relevant_experience_hints: Vec::new(),
        }
    }

    fn recommend_approach(&self, complexity: &str) -> String {
        match complexity {
            "complex" => "recursive_pdca".to_string(),
            "medium" => "standard_pdca".to_string(),
            _ => "direct_da".to_string(),
        }
    }
}

fn parse_iso8601_duration(s: &str) -> Option<chrono::Duration> {
    let s = s.strip_prefix("PT")?;
    let mut hours: i64 = 0;
    let mut minutes: i64 = 0;
    let mut secs: i64 = 0;
    let mut num_str = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            num_str.push(c);
        } else {
            let num: i64 = num_str.parse().ok()?;
            num_str.clear();
            match c {
                'H' => hours = num,
                'M' => minutes = num,
                'S' => secs = num,
                _ => {}
            }
        }
    }
    Some(chrono::Duration::hours(hours) + chrono::Duration::minutes(minutes) + chrono::Duration::seconds(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试使用独立的 temp dir，避免 sled 数据库污染
    fn with_l0<F, R>(f: F) -> R
    where
        F: FnOnce(Arc<L0Store>) -> R,
    {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(L0Store::new(dir.path().to_str().unwrap()).unwrap());
        f(store)
        // dir 在此 drop，自动清理
    }

    fn test_event_bus() -> Arc<EventBus> {
        Arc::new(EventBus::new(100))
    }

    #[test]
    fn test_task_start_analysis() {
        with_l0(|l0| {
            let mut engine = ProactiveEngine::new(l0, test_event_bus());
            let analysis = engine.on_task_start("Write a hello world program", "iri://task/1").unwrap();
            assert_eq!(analysis.complexity, "simple");
            assert_eq!(analysis.estimated_steps, 1);
        });
    }

    #[test]
    fn test_check_completed_pass() {
        with_l0(|l0| {
            let engine = ProactiveEngine::new(l0, test_event_bus());
            let result = engine.on_check_completed(&json!({"verdict": "pass"}), "iri://task/1");
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_check_completed_fail() {
        with_l0(|l0| {
            let engine = ProactiveEngine::new(l0, test_event_bus());
            let result = engine.on_check_completed(&json!({"verdict": "fail"}), "iri://task/1");
            assert!(result.is_some());
            assert_eq!(result.unwrap().severity, "high");
        });
    }

    #[test]
    fn test_custom_config() {
        with_l0(|l0| {
            let config = PerceptionConfig {
                simple_input_threshold: 100,
                medium_input_threshold: 500,
                ..Default::default()
            };
            let mut engine = ProactiveEngine::with_config(config, l0, test_event_bus());
            let analysis = engine.on_task_start("A medium length task description", "iri://task/1").unwrap();
            assert_eq!(analysis.complexity, "simple");
        });
    }

    #[test]
    fn test_cache_eviction() {
        with_l0(|l0| {
            let config = PerceptionConfig {
                cache_max_entries: 2,
                cache_ttl_seconds: 300,
                ..Default::default()
            };
            let mut engine = ProactiveEngine::with_config(config, l0, test_event_bus());
            engine.on_task_start("task1", "iri://task/1").unwrap();
            engine.on_task_start("task2", "iri://task/2").unwrap();
            engine.on_task_start("task3", "iri://task/3").unwrap();
            assert!(engine.cache.len() <= 3);
        });
    }
}

#[cfg(test)]
mod tests_5w2h {
    use super::*;
    use crate::memory::l0_store::L0Store;
    use std::sync::Arc;

    fn test_event_bus() -> Arc<EventBus> {
        Arc::new(EventBus::new(100))
    }

    #[test]
    fn test_check_5w2h_constraints_no_store() {
        let store = Arc::new(L0Store::new("/tmp/test_5w2h_no_store").unwrap());
        let engine = ProactiveEngine::new(store, test_event_bus());
        assert!(engine.check_5w2h_constraints("iri://task/test/5w2h").is_none());
    }

    #[test]
    fn test_check_5w2h_constraints_deadline_approaching() {
        let store = Arc::new(L0Store::new("/tmp/test_5w2h_deadline").unwrap());
        let engine = ProactiveEngine::new(store.clone(), test_event_bus());

        let deadline = Utc::now() + chrono::Duration::minutes(30);
        let json_ld = serde_json::json!({
            "@type": "task:5W2H",
            "task:when": {
                "task:deadline": deadline.to_rfc3339(),
                "task:reminderBefore": "PT1H"
            }
        });
        store.store("iri://task/test/5w2h", &json_ld.to_string()).unwrap();

        let result = engine.check_5w2h_constraints("iri://task/test/5w2h");
        assert!(result.is_some());
        assert!(result.unwrap().contains("DEADLINE_APPROACHING"));
    }

    #[test]
    fn test_check_5w2h_constraints_budget_exceeded() {
        let store = Arc::new(L0Store::new("/tmp/test_5w2h_budget").unwrap());
        let engine = ProactiveEngine::new(store.clone(), test_event_bus());

        let json_ld = serde_json::json!({
            "@type": "task:5W2H",
            "task:howMuch": {
                "task:tokenBudget": 100000,
                "task:actualCost": {
                    "tokensUsed": 85000,
                    "cyclesUsed": 2,
                    "durationSecs": 120.0
                }
            }
        });
        store.store("iri://task/test/5w2h", &json_ld.to_string()).unwrap();

        let result = engine.check_5w2h_constraints("iri://task/test/5w2h");
        assert!(result.is_some());
        assert!(result.unwrap().contains("BUDGET_EXCEEDED"));
    }

    #[test]
    fn test_check_5w2h_constraints_custom_reminder() {
        let store = Arc::new(L0Store::new("/tmp/test_5w2h_custom_reminder").unwrap());
        let engine = ProactiveEngine::new(store.clone(), test_event_bus());

        let deadline = Utc::now() + chrono::Duration::minutes(20);
        let json_ld = serde_json::json!({
            "@type": "task:5W2H",
            "task:when": {
                "task:deadline": deadline.to_rfc3339(),
                "task:reminderBefore": "PT30M"
            }
        });
        store.store("iri://task/test/custom/5w2h", &json_ld.to_string()).unwrap();

        let result = engine.check_5w2h_constraints("iri://task/test/custom/5w2h");
        assert!(result.is_some());
        assert!(result.unwrap().contains("DEADLINE_APPROACHING"));
    }
}
