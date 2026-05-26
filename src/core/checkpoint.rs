use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointData {
    pub checkpoint_iri: String,
    pub task_iri: String,
    pub name: String,
    pub node_count: i32,
    pub total_size_bytes: i32,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub nodes_json: String,
    pub session_messages_json: String,
    pub agent_state_json: String,
}

pub struct CheckpointManager {
    checkpoints: DashMap<String, CheckpointData>,
    task_checkpoints: RwLock<HashMap<String, Vec<String>>>,
    counter: AtomicU64,
}

impl CheckpointManager {
    pub fn new() -> Self {
        Self {
            checkpoints: DashMap::new(),
            task_checkpoints: RwLock::new(HashMap::new()),
            counter: AtomicU64::new(0),
        }
    }

    pub fn create(
        &self,
        task_iri: &str,
        name: &str,
        nodes_json: &str,
        session_messages_json: &str,
        agent_state_json: &str,
        tags: &[String],
    ) -> Result<CheckpointData, CoreError> {
        let seq = self.counter.fetch_add(1, Ordering::SeqCst);
        let checkpoint_iri = format!("iri://checkpoint/{}/seq_{}", task_iri.strip_prefix("iri://").unwrap_or(task_iri), seq);

        let nodes: Vec<serde_json::Value> = serde_json::from_str(nodes_json).unwrap_or_default();
        let node_count = nodes.len() as i32;
        let total_size_bytes = nodes_json.len() as i32 + session_messages_json.len() as i32 + agent_state_json.len() as i32;

        let checkpoint = CheckpointData {
            checkpoint_iri: checkpoint_iri.clone(),
            task_iri: task_iri.to_string(),
            name: name.to_string(),
            node_count,
            total_size_bytes,
            created_at: Utc::now(),
            tags: tags.to_vec(),
            nodes_json: nodes_json.to_string(),
            session_messages_json: session_messages_json.to_string(),
            agent_state_json: agent_state_json.to_string(),
        };

        self.checkpoints.insert(checkpoint_iri.clone(), checkpoint.clone());

        {
            let mut task_cps = self.task_checkpoints.write();
            task_cps
                .entry(task_iri.to_string())
                .or_insert_with(Vec::new)
                .push(checkpoint_iri.clone());
        }

        Ok(checkpoint)
    }

    pub fn restore(&self, checkpoint_iri: &str) -> Result<CheckpointData, CoreError> {
        self.checkpoints
            .get(checkpoint_iri)
            .map(|c| c.clone())
            .ok_or_else(|| CoreError::Internal {
                message: format!("Checkpoint not found: {}", checkpoint_iri),
            })
    }

    pub fn list(&self, task_iri: &str, limit: i32) -> Vec<CheckpointData> {
        let task_cps = self.task_checkpoints.read();
        if let Some(cp_iris) = task_cps.get(task_iri) {
            let mut results: Vec<CheckpointData> = cp_iris
                .iter()
                .filter_map(|iri| self.checkpoints.get(iri).map(|c| c.clone()))
                .collect();
            results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            results.truncate(limit as usize);
            results
        } else {
            Vec::new()
        }
    }

    pub fn delete(&self, checkpoint_iri: &str) -> Result<(), CoreError> {
        if let Some((_, cp)) = self.checkpoints.remove(checkpoint_iri) {
            let mut task_cps = self.task_checkpoints.write();
            if let Some(iris) = task_cps.get_mut(&cp.task_iri) {
                iris.retain(|iri| iri != checkpoint_iri);
            }
            Ok(())
        } else {
            Err(CoreError::Internal {
                message: format!("Checkpoint not found: {}", checkpoint_iri),
            })
        }
    }

    pub fn checkpoint_count(&self) -> u64 {
        self.checkpoints.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_checkpoint() {
        let manager = CheckpointManager::new();
        
        let checkpoint = manager.create(
            "iri://task/123",
            "test_checkpoint",
            r#"[{"@id":"iri://node/1"}]"#,
            r#"[{"role":"user","content":"test"}]"#,
            r#"{"status":"running"}"#,
            &["important".to_string()],
        ).unwrap();
        
        assert!(checkpoint.checkpoint_iri.starts_with("iri://checkpoint/"));
        assert_eq!(checkpoint.task_iri, "iri://task/123");
        assert_eq!(checkpoint.name, "test_checkpoint");
        assert_eq!(checkpoint.node_count, 1);
        assert_eq!(checkpoint.tags, vec!["important"]);
    }

    #[test]
    fn test_restore_checkpoint() {
        let manager = CheckpointManager::new();
        
        let checkpoint = manager.create(
            "iri://task/456",
            "restore_test",
            "[]",
            "[]",
            "{}",
            &[],
        ).unwrap();
        
        let restored = manager.restore(&checkpoint.checkpoint_iri).unwrap();
        assert_eq!(restored.checkpoint_iri, checkpoint.checkpoint_iri);
        assert_eq!(restored.name, "restore_test");
    }

    #[test]
    fn test_list_checkpoints() {
        let manager = CheckpointManager::new();
        
        manager.create("iri://task/789", "cp1", "[]", "[]", "{}", &[]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.create("iri://task/789", "cp2", "[]", "[]", "{}", &[]).unwrap();
        
        let list = manager.list("iri://task/789", 10);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "cp2");
        assert_eq!(list[1].name, "cp1");
    }

    #[test]
    fn test_delete_checkpoint() {
        let manager = CheckpointManager::new();
        
        let checkpoint = manager.create(
            "iri://task/delete",
            "to_delete",
            "[]",
            "[]",
            "{}",
            &[],
        ).unwrap();
        
        assert_eq!(manager.checkpoint_count(), 1);
        
        manager.delete(&checkpoint.checkpoint_iri).unwrap();
        assert_eq!(manager.checkpoint_count(), 0);
        
        let result = manager.restore(&checkpoint.checkpoint_iri);
        assert!(result.is_err());
    }
}
