use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum AccessLevel {
    Read,
    Write,
    Admin,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhyDetail {
    pub description: String,
    pub success_criteria: Vec<String>,
    pub priority: Priority,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhoDetail {
    pub requestor: Option<String>,
    pub assignees: Vec<String>,
    pub stakeholders: Vec<String>,
    pub required_role: Option<String>,
    pub access_level: Option<AccessLevel>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhenDetail {
    pub deadline: Option<DateTime<Utc>>,
    pub start_after: Option<DateTime<Utc>>,
    pub estimated_duration: Option<String>,
    pub timezone: Option<String>,
    pub reminder_before: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhereDetail {
    pub data_sources: Vec<String>,
    pub execution_environment: Option<String>,
    pub target_repository: Option<String>,
    pub target_branch: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HowDetail {
    pub plan_iri: Option<String>,
    pub preferred_skills: Vec<String>,
    pub forbidden_tools: Vec<String>,
    pub required_steps: Option<String>,
    pub dependencies: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HowMuchDetail {
    pub token_budget: Option<u64>,
    pub max_sub_agents: Option<u32>,
    pub max_pdca_cycles: Option<u32>,
    pub expected_quality: Option<f32>,
    pub actual_cost: Option<ActualCost>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActualCost {
    pub tokens_used: u64,
    pub cycles_used: u32,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FillStage {
    Create,
    Plan,
    Do,
    Check,
    Act,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionMeta {
    pub fill_stage: FillStage,
    pub filled_by: Option<String>,
    pub filled_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task5W2H {
    pub what: String,
    pub why: WhyDetail,
    pub who: Option<WhoDetail>,
    pub when: Option<WhenDetail>,
    #[serde(rename = "where")]
    pub where_: Option<WhereDetail>,
    pub how: Option<HowDetail>,
    pub how_much: Option<HowMuchDetail>,
    pub dimension_meta: HashMap<String, DimensionMeta>,
    pub frozen: bool,
}

impl Task5W2H {
    pub fn new(what: &str, why_description: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let mut dimension_meta = HashMap::new();
        dimension_meta.insert("what".to_string(), DimensionMeta {
            fill_stage: FillStage::Create,
            filled_by: Some("SA".to_string()),
            filled_at: Some(now.clone()),
        });
        dimension_meta.insert("why".to_string(), DimensionMeta {
            fill_stage: FillStage::Create,
            filled_by: Some("SA".to_string()),
            filled_at: Some(now),
        });
        Self {
            what: what.to_string(),
            why: WhyDetail {
                description: why_description.to_string(),
                success_criteria: Vec::new(),
                priority: Priority::Medium,
            },
            who: None,
            when: None,
            where_: None,
            how: None,
            how_much: None,
            dimension_meta,
            frozen: false,
        }
    }

    pub fn with_who(mut self, who: WhoDetail) -> Self {
        self.who = Some(who);
        self
    }

    pub fn with_when(mut self, when: WhenDetail) -> Self {
        self.when = Some(when);
        self
    }

    pub fn with_where(mut self, where_: WhereDetail) -> Self {
        self.where_ = Some(where_);
        self
    }

    pub fn with_how(mut self, how: HowDetail) -> Self {
        self.how = Some(how);
        self
    }

    pub fn with_how_much(mut self, how_much: HowMuchDetail) -> Self {
        self.how_much = Some(how_much);
        self
    }

    pub fn record_fill(&mut self, dimension: &str, stage: FillStage, filled_by: &str) {
        self.dimension_meta.insert(dimension.to_string(), DimensionMeta {
            fill_stage: stage,
            filled_by: Some(filled_by.to_string()),
            filled_at: Some(chrono::Utc::now().to_rfc3339()),
        });
    }

    pub fn check_completeness(&self, task_level: &str) -> Vec<String> {
        let all_dims = vec!["what", "why", "who", "when", "where", "how", "how_much"];
        let required: Vec<&str> = match task_level {
            "Instant" => vec!["what"],
            "Simple" => vec!["what", "why"],
            "Standard" | "Complex" => all_dims.clone(),
            _ => vec!["what", "why"],
        };
        required.into_iter().filter(|d| !self.dimension_meta.contains_key(*d)).map(String::from).collect()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_minimal_set_ready(&self) -> bool {
        !self.what.is_empty() && !self.why.description.is_empty()
    }

    pub fn derive_objective(&self) -> String {
        self.what.clone()
    }

    pub fn to_json_ld(&self, task_iri: &str) -> Result<Value, String> {
        let stripped = task_iri.trim_start_matches("iri://task/");
        let id = format!("iri://task/{}/5w2h", stripped);

        let mut result = json!({
            "@context": {
                "task": "https://pdca-agent.org/ontology/task#"
            },
            "@id": id,
            "@type": "task:5W2H",
            "task:what": self.what,
            "task:why": {
                "task:description": self.why.description,
                "task:successCriteria": self.why.success_criteria,
                "task:priority": format!("{:?}", self.why.priority)
            }
        });

        let map = result.as_object_mut()
            .expect("json!({}) always creates an object");

        if let Some(ref who) = self.who {
            map.insert(
                "task:who".to_string(),
                json!({
                    "task:requestor": who.requestor,
                    "task:assignees": who.assignees,
                    "task:stakeholders": who.stakeholders,
                    "task:requiredRole": who.required_role,
                    "task:accessLevel": who.access_level.as_ref().map(|al| format!("{:?}", al))
                }),
            );
        }

        if let Some(ref when) = self.when {
            map.insert(
                "task:when".to_string(),
                json!({
                    "task:deadline": when.deadline.map(|dt| dt.to_rfc3339()),
                    "task:startAfter": when.start_after.map(|dt| dt.to_rfc3339()),
                    "task:estimatedDuration": when.estimated_duration,
                    "task:timezone": when.timezone,
                    "task:reminderBefore": when.reminder_before
                }),
            );
        }

        if let Some(ref where_) = self.where_ {
            map.insert(
                "task:where".to_string(),
                json!({
                    "task:dataSources": where_.data_sources,
                    "task:executionEnvironment": where_.execution_environment,
                    "task:targetRepository": where_.target_repository,
                    "task:targetBranch": where_.target_branch
                }),
            );
        }

        if let Some(ref how) = self.how {
            map.insert(
                "task:how".to_string(),
                json!({
                    "task:planIri": how.plan_iri,
                    "task:preferredSkills": how.preferred_skills,
                    "task:forbiddenTools": how.forbidden_tools,
                    "task:requiredSteps": how.required_steps,
                    "task:dependencies": how.dependencies
                }),
            );
        }

        if let Some(ref how_much) = self.how_much {
            let mut hm = json!({
                "task:tokenBudget": how_much.token_budget,
                "task:maxSubAgents": how_much.max_sub_agents,
                "task:maxPdcaCycles": how_much.max_pdca_cycles,
                "task:expectedQuality": how_much.expected_quality
            });
            if let Some(ref cost) = how_much.actual_cost {
                hm.as_object_mut()
                    .expect("json!({}) always creates an object")
                    .insert(
                    "task:actualCost".to_string(),
                    json!({
                        "task:tokensUsed": cost.tokens_used,
                        "task:cyclesUsed": cost.cycles_used,
                        "task:durationSecs": cost.duration_secs
                    }),
                );
            }
            map.insert("task:howMuch".to_string(), hm);
        }

        map.insert(
            "task:frozen".to_string(),
            json!(self.frozen),
        );

        let mut meta_map = serde_json::Map::new();
        for (dim, meta) in &self.dimension_meta {
            meta_map.insert(dim.clone(), json!({
                "task:fillStage": format!("{:?}", meta.fill_stage),
                "task:filledBy": meta.filled_by,
                "task:filledAt": meta.filled_at
            }));
        }
        map.insert(
            "task:dimensionMeta".to_string(),
            Value::Object(meta_map),
        );

        Ok(result)
    }

    pub fn from_json_ld(value: &Value) -> Result<Self, String> {
        let obj = value
            .as_object()
            .ok_or("JSON-LD value is not an object")?;

        let what = obj
            .get("task:what")
            .and_then(|v| v.as_str())
            .ok_or("Missing task:what")?
            .to_string();

        let why_obj = obj
            .get("task:why")
            .and_then(|v| v.as_object())
            .ok_or("Missing task:why")?;

        let why = WhyDetail {
            description: why_obj
                .get("task:description")
                .and_then(|v| v.as_str())
                .ok_or("Missing task:description in why")?
                .to_string(),
            success_criteria: why_obj
                .get("task:successCriteria")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            priority: why_obj
                .get("task:priority")
                .and_then(|v| v.as_str())
                .and_then(|s| match s {
                    "High" => Some(Priority::High),
                    "Medium" => Some(Priority::Medium),
                    "Low" => Some(Priority::Low),
                    _ => None,
                })
                .ok_or("Invalid task:priority")?,
        };

        let who = obj
            .get("task:who")
            .and_then(|v| v.as_object())
            .map(|who_obj| WhoDetail {
                requestor: who_obj
                    .get("task:requestor")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                assignees: who_obj
                    .get("task:assignees")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                stakeholders: who_obj
                    .get("task:stakeholders")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                required_role: who_obj
                    .get("task:requiredRole")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                access_level: who_obj
                    .get("task:accessLevel")
                    .and_then(|v| v.as_str())
                    .and_then(|s| match s {
                        "Read" => Some(AccessLevel::Read),
                        "Write" => Some(AccessLevel::Write),
                        "Admin" => Some(AccessLevel::Admin),
                        _ => None,
                    }),
            });

        let when = obj
            .get("task:when")
            .and_then(|v| v.as_object())
            .map(|when_obj| WhenDetail {
                deadline: when_obj
                    .get("task:deadline")
                    .and_then(|v| v.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                start_after: when_obj
                    .get("task:startAfter")
                    .and_then(|v| v.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                estimated_duration: when_obj
                    .get("task:estimatedDuration")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                timezone: when_obj
                    .get("task:timezone")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                reminder_before: when_obj
                    .get("task:reminderBefore")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

        let where_ = obj
            .get("task:where")
            .and_then(|v| v.as_object())
            .map(|where_obj| WhereDetail {
                data_sources: where_obj
                    .get("task:dataSources")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                execution_environment: where_obj
                    .get("task:executionEnvironment")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                target_repository: where_obj
                    .get("task:targetRepository")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                target_branch: where_obj
                    .get("task:targetBranch")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

        let how = obj
            .get("task:how")
            .and_then(|v| v.as_object())
            .map(|how_obj| HowDetail {
                plan_iri: how_obj
                    .get("task:planIri")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                preferred_skills: how_obj
                    .get("task:preferredSkills")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                forbidden_tools: how_obj
                    .get("task:forbiddenTools")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                required_steps: how_obj
                    .get("task:requiredSteps")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                dependencies: how_obj
                    .get("task:dependencies")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            });

        let how_much = obj
            .get("task:howMuch")
            .and_then(|v| v.as_object())
            .map(|hm_obj| HowMuchDetail {
                token_budget: hm_obj
                    .get("task:tokenBudget")
                    .and_then(|v| v.as_u64()),
                max_sub_agents: hm_obj
                    .get("task:maxSubAgents")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                max_pdca_cycles: hm_obj
                    .get("task:maxPdcaCycles")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                expected_quality: hm_obj
                    .get("task:expectedQuality")
                    .and_then(|v| v.as_f64())
                    .map(|n| n as f32),
                actual_cost: hm_obj
                    .get("task:actualCost")
                    .and_then(|v| v.as_object())
                    .map(|cost_obj| ActualCost {
                        tokens_used: cost_obj
                            .get("task:tokensUsed")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cycles_used: cost_obj
                            .get("task:cyclesUsed")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32,
                        duration_secs: cost_obj
                            .get("task:durationSecs")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0),
                    }),
            });

        let frozen = obj
            .get("task:frozen")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let dimension_meta = obj
            .get("task:dimensionMeta")
            .and_then(|v| v.as_object())
            .map(|meta_obj| {
                let mut map = HashMap::new();
                for (dim, val) in meta_obj {
                    if let Some(meta_val) = val.as_object() {
                        let fill_stage = meta_val
                            .get("task:fillStage")
                            .and_then(|v| v.as_str())
                            .and_then(|s| match s {
                                "Create" => Some(FillStage::Create),
                                "Plan" => Some(FillStage::Plan),
                                "Do" => Some(FillStage::Do),
                                "Check" => Some(FillStage::Check),
                                "Act" => Some(FillStage::Act),
                                _ => None,
                            })
                            .unwrap_or(FillStage::Create);
                        let filled_by = meta_val
                            .get("task:filledBy")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let filled_at = meta_val
                            .get("task:filledAt")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        map.insert(dim.clone(), DimensionMeta {
                            fill_stage,
                            filled_by,
                            filled_at,
                        });
                    }
                }
                map
            })
            .unwrap_or_default();

        Ok(Self {
            what,
            why,
            who,
            when,
            where_,
            how,
            how_much,
            dimension_meta,
            frozen,
        })
    }
}

impl Default for Task5W2H {
    fn default() -> Self {
        Self {
            what: String::new(),
            why: WhyDetail {
                description: String::new(),
                success_criteria: Vec::new(),
                priority: Priority::Medium,
            },
            who: None,
            when: None,
            where_: None,
            how: None,
            how_much: None,
            dimension_meta: HashMap::new(),
            frozen: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_5w2h_lifecycle_progressive_filling() {
        let mut w2h = Task5W2H::new("创建Web服务", "提供REST API");
        assert!(w2h.is_minimal_set_ready());
        assert!(w2h.how.is_none());
        assert!(w2h.where_.is_none());

        w2h = w2h.with_how(HowDetail {
            plan_iri: Some("iri://plan/web-service".to_string()),
            preferred_skills: vec!["file_read".to_string(), "file_write".to_string()],
            forbidden_tools: vec!["bash".to_string()],
            required_steps: Some("1.设计API 2.实现路由 3.测试".to_string()),
            dependencies: vec![],
        }).with_where(WhereDetail {
            data_sources: vec!["src/api/".to_string()],
            execution_environment: Some("sandbox".to_string()),
            target_repository: None,
            target_branch: None,
        });
        assert!(w2h.how.is_some());
        assert!(w2h.where_.is_some());

        w2h = w2h.with_how_much(HowMuchDetail {
            token_budget: Some(100000),
            max_sub_agents: None,
            max_pdca_cycles: Some(3),
            expected_quality: Some(0.85),
            actual_cost: Some(ActualCost {
                tokens_used: 45000,
                cycles_used: 1,
                duration_secs: 120.0,
            }),
        });
        assert!(w2h.how_much.is_some());
        let actual = w2h.how_much.as_ref().unwrap().actual_cost.as_ref().unwrap();
        assert_eq!(actual.tokens_used, 45000);
    }

    #[test]
    fn test_5w2h_json_ld_with_all_dimensions() {
        let w2h = Task5W2H::new("完整测试", "验证全量5W2H")
            .with_who(WhoDetail {
                requestor: Some("user:test".to_string()),
                assignees: vec!["agent:pa".to_string(), "agent:da".to_string()],
                stakeholders: vec!["user:pm".to_string()],
                required_role: Some("Do".to_string()),
                access_level: Some(AccessLevel::Write),
            })
            .with_when(WhenDetail {
                deadline: Some("2026-12-31T23:59:59Z".parse().unwrap()),
                start_after: Some("2026-06-01T00:00:00Z".parse().unwrap()),
                estimated_duration: Some("30d".to_string()),
                timezone: Some("Asia/Shanghai".to_string()),
                reminder_before: None,
            })
            .with_where(WhereDetail {
                data_sources: vec!["db/main".to_string(), "api/v1".to_string()],
                execution_environment: Some("production".to_string()),
                target_repository: Some("https://github.com/test/repo".to_string()),
                target_branch: Some("main".to_string()),
            })
            .with_how(HowDetail {
                plan_iri: Some("iri://plan/full-test".to_string()),
                preferred_skills: vec!["file_read".to_string(), "code_execute".to_string()],
                forbidden_tools: vec!["bash".to_string()],
                required_steps: Some("步骤1 步骤2 步骤3".to_string()),
                dependencies: vec!["dep1".to_string()],
            })
            .with_how_much(HowMuchDetail {
                token_budget: Some(200000),
                max_sub_agents: Some(5),
                max_pdca_cycles: Some(4),
                expected_quality: Some(0.95),
                actual_cost: Some(ActualCost {
                    tokens_used: 80000,
                    cycles_used: 2,
                    duration_secs: 360.0,
                }),
            });

        let json_ld = w2h.to_json_ld("full-test-001").unwrap();
        assert_eq!(json_ld["@type"], "task:5W2H");
        assert_eq!(json_ld["@id"], "iri://task/full-test-001/5w2h");
        assert!(json_ld.get("task:who").is_some());
        assert!(json_ld.get("task:when").is_some());
        assert!(json_ld.get("task:where").is_some());
        assert!(json_ld.get("task:how").is_some());
        assert!(json_ld.get("task:howMuch").is_some());
        assert!(json_ld.get("task:frozen").is_some());
        assert!(json_ld.get("task:dimensionMeta").is_some());

        let restored = Task5W2H::from_json_ld(&json_ld).unwrap();
        assert_eq!(restored.what, "完整测试");
        assert_eq!(restored.who.unwrap().access_level, Some(AccessLevel::Write));
        assert_eq!(restored.how_much.unwrap().actual_cost.unwrap().tokens_used, 80000);
        assert!(!restored.frozen);
        assert!(restored.dimension_meta.contains_key("what"));
        assert!(restored.dimension_meta.contains_key("why"));
    }

    #[test]
    fn test_task5w2h_new_minimal_set() {
        let w2h = Task5W2H::new("创建Rust项目", "学习Rust语言");
        assert_eq!(w2h.what, "创建Rust项目");
        assert_eq!(w2h.why.description, "学习Rust语言");
        assert!(w2h.is_minimal_set_ready());
        assert_eq!(w2h.derive_objective(), "创建Rust项目");
        assert!(w2h.who.is_none());
        assert!(w2h.when.is_none());
        assert!(w2h.where_.is_none());
        assert!(w2h.how.is_none());
        assert!(w2h.how_much.is_none());
        assert!(!w2h.frozen);
        assert!(w2h.dimension_meta.contains_key("what"));
        assert!(w2h.dimension_meta.contains_key("why"));
        assert_eq!(w2h.dimension_meta.get("what").unwrap().fill_stage, FillStage::Create);
    }

    #[test]
    fn test_task5w2h_builder_pattern() {
        let w2h = Task5W2H::new("重构模块", "提高代码质量")
            .with_who(WhoDetail {
                requestor: Some("user:1".to_string()),
                assignees: vec!["agent:1".to_string()],
                stakeholders: vec![],
                required_role: Some("Do".to_string()),
                access_level: Some(AccessLevel::Write),
            })
            .with_when(WhenDetail {
                deadline: Some("2026-06-01T00:00:00Z".parse().unwrap()),
                start_after: None,
                estimated_duration: Some("2h".to_string()),
                timezone: Some("UTC".to_string()),
                reminder_before: None,
            })
            .with_where(WhereDetail {
                data_sources: vec!["src/".to_string()],
                execution_environment: Some("sandbox".to_string()),
                target_repository: None,
                target_branch: None,
            })
            .with_how(HowDetail {
                plan_iri: Some("iri://plan/1".to_string()),
                preferred_skills: vec!["file_read".to_string()],
                forbidden_tools: vec!["bash".to_string()],
                required_steps: Some("1.读取 2.分析 3.重构".to_string()),
                dependencies: vec![],
            })
            .with_how_much(HowMuchDetail {
                token_budget: Some(50000),
                max_sub_agents: Some(3),
                max_pdca_cycles: Some(2),
                expected_quality: Some(0.9),
                actual_cost: None,
            });
        assert!(w2h.who.is_some());
        assert!(w2h.when.is_some());
        assert!(w2h.where_.is_some());
        assert!(w2h.how.is_some());
        assert!(w2h.how_much.is_some());
        assert_eq!(w2h.how.as_ref().unwrap().forbidden_tools, vec!["bash"]);
        assert_eq!(w2h.how_much.as_ref().unwrap().token_budget, Some(50000));
    }

    #[test]
    fn test_task5w2h_json_ld_roundtrip() {
        let original = Task5W2H::new("测试任务", "验证JSON-LD转换")
            .with_when(WhenDetail {
                deadline: Some("2026-06-01T00:00:00Z".parse().unwrap()),
                start_after: None,
                estimated_duration: None,
                timezone: None,
                reminder_before: None,
            })
            .with_how_much(HowMuchDetail {
                token_budget: Some(100000),
                max_sub_agents: None,
                max_pdca_cycles: Some(3),
                expected_quality: None,
                actual_cost: None,
            });
        let json_ld = original.to_json_ld("test-123").unwrap();
        assert_eq!(json_ld["@type"], "task:5W2H");
        assert_eq!(json_ld["task:what"], "测试任务");
        assert_eq!(json_ld["@id"], "iri://task/test-123/5w2h");
        let restored = Task5W2H::from_json_ld(&json_ld).unwrap();
        assert_eq!(restored.what, original.what);
        assert_eq!(restored.why.description, original.why.description);
        assert!(restored.when.is_some());
        assert!(restored.how_much.is_some());
    }

    #[test]
    fn test_task5w2h_minimal_set_validation() {
        let valid = Task5W2H::new("任务", "意图");
        assert!(valid.is_minimal_set_ready());
        let mut empty_what = valid.clone();
        empty_what.what = String::new();
        assert!(!empty_what.is_minimal_set_ready());
        let mut empty_why = valid.clone();
        empty_why.why.description = String::new();
        assert!(!empty_why.is_minimal_set_ready());
    }

    #[test]
    fn test_priority_access_level_serialization() {
        assert_eq!(serde_json::to_string(&Priority::High).unwrap(), "\"High\"");
        assert_eq!(serde_json::to_string(&Priority::Medium).unwrap(), "\"Medium\"");
        assert_eq!(serde_json::to_string(&Priority::Low).unwrap(), "\"Low\"");
        assert_eq!(serde_json::to_string(&AccessLevel::Read).unwrap(), "\"Read\"");
        assert_eq!(serde_json::to_string(&AccessLevel::Write).unwrap(), "\"Write\"");
        assert_eq!(serde_json::to_string(&AccessLevel::Admin).unwrap(), "\"Admin\"");
    }

    #[test]
    fn test_actual_cost_structure() {
        let cost = ActualCost {
            tokens_used: 15000,
            cycles_used: 2,
            duration_secs: 45.5,
        };
        let json = serde_json::to_string(&cost).unwrap();
        let restored: ActualCost = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.tokens_used, 15000);
        assert_eq!(restored.cycles_used, 2);
        assert!((restored.duration_secs - 45.5).abs() < 0.001);
    }

    #[test]
    fn test_record_fill() {
        let mut w2h = Task5W2H::new("任务", "原因");
        assert_eq!(w2h.dimension_meta.len(), 2);
        w2h.record_fill("who", FillStage::Plan, "PA");
        assert!(w2h.dimension_meta.contains_key("who"));
        assert_eq!(w2h.dimension_meta.get("who").unwrap().fill_stage, FillStage::Plan);
        assert_eq!(w2h.dimension_meta.get("who").unwrap().filled_by, Some("PA".to_string()));
        assert!(w2h.dimension_meta.get("who").unwrap().filled_at.is_some());
    }

    #[test]
    fn test_check_completeness() {
        let w2h = Task5W2H::new("任务", "原因");
        let missing_instant = w2h.check_completeness("Instant");
        assert!(missing_instant.is_empty());
        let missing_simple = w2h.check_completeness("Simple");
        assert!(missing_simple.is_empty());
        let missing_standard = w2h.check_completeness("Standard");
        assert!(missing_standard.contains(&"who".to_string()));
        assert!(missing_standard.contains(&"when".to_string()));
        assert!(missing_standard.contains(&"where".to_string()));
        assert!(missing_standard.contains(&"how".to_string()));
        assert!(missing_standard.contains(&"how_much".to_string()));
    }

    #[test]
    fn test_freeze() {
        let mut w2h = Task5W2H::new("任务", "原因");
        assert!(!w2h.frozen);
        w2h.freeze();
        assert!(w2h.frozen);
    }

    #[test]
    fn test_reminder_before_json_ld_roundtrip() {
        let w2h = Task5W2H::new("提醒测试", "验证提醒")
            .with_when(WhenDetail {
                deadline: Some("2026-12-31T23:59:59Z".parse().unwrap()),
                start_after: None,
                estimated_duration: None,
                timezone: None,
                reminder_before: Some("1h".to_string()),
            });
        let json_ld = w2h.to_json_ld("reminder-001").unwrap();
        let when_obj = json_ld.get("task:when").unwrap().as_object().unwrap();
        assert_eq!(when_obj.get("task:reminderBefore").unwrap().as_str(), Some("1h"));
        let restored = Task5W2H::from_json_ld(&json_ld).unwrap();
        assert_eq!(restored.when.unwrap().reminder_before, Some("1h".to_string()));
    }
}
