//! Default controlled vocabularies for batch knowledge extraction.
//!
//! These define the entity types, relation types, and intent types
//! used by Batch Agents when extracting structured knowledge from
//! user conversations. Each BatchAgentConfig can override these with
//! custom domain-specific vocabularies.

use serde::{Deserialize, Serialize};

// ============================================================
// Entity type config
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTypeConfig {
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub custom: Vec<String>,
}

impl EntityTypeConfig {
    pub fn effective_types(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(self.vocabulary.iter().cloned());
        all.extend(self.custom.iter().cloned());
        all
    }
}

impl Default for EntityTypeConfig {
    fn default() -> Self {
        Self {
            vocabulary: vec![
                "onto:BusinessDomain".into(),
                "onto:BusinessEntity".into(),
                "onto:BusinessRule".into(),
                "onto:TechStack".into(),
                "onto:DesignDecision".into(),
                "onto:Constraint".into(),
                "onto:ExternalSystem".into(),
                "onto:UserRole".into(),
            ],
            custom: vec![],
        }
    }
}

// ============================================================
// Relation type config
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationTypeConfig {
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub custom: Vec<String>,
}

impl RelationTypeConfig {
    pub fn effective_types(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(self.vocabulary.iter().cloned());
        all.extend(self.custom.iter().cloned());
        all
    }
}

impl Default for RelationTypeConfig {
    fn default() -> Self {
        Self {
            vocabulary: vec![
                "converts_to".into(),
                "depends_on".into(),
                "triggers".into(),
                "belongs_to".into(),
                "validates".into(),
                "notifies".into(),
            ],
            custom: vec![],
        }
    }
}

// ============================================================
// Intent type config
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentTypeConfig {
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub custom: Vec<String>,
}

impl IntentTypeConfig {
    pub fn effective_types(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(self.vocabulary.iter().cloned());
        all.extend(self.custom.iter().cloned());
        all
    }
}

impl Default for IntentTypeConfig {
    fn default() -> Self {
        Self {
            vocabulary: vec![
                "Discussing_Business_Logic".into(),
                "Discussing_Tech_Stack".into(),
                "Defining_Constraint".into(),
                "Making_Decision".into(),
                "Identifying_Stakeholder".into(),
            ],
            custom: vec![],
        }
    }
}
