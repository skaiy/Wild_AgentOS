use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, instrument, warn};

use crate::memory::l0_store::{L0Store, MesiState};
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_bus::MemoryBus;
use crate::{CoreConfig, CoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStrategy {
    WriteThrough,
    WriteBack,
}

pub struct ConsistencyEngine {
    memory_bus: Arc<MemoryBus>,
    l0_store: Arc<L0Store>,
    blackboard: Arc<Blackboard>,
    projection: Arc<ProjectionEngine>,
    critical_tags: RwLock<HashSet<String>>,
}

impl ConsistencyEngine {
    pub fn new(
        memory_bus: Arc<MemoryBus>,
        l0_store: Arc<L0Store>,
        blackboard: Arc<Blackboard>,
        projection: Arc<ProjectionEngine>,
    ) -> Self {
        let critical_tags = RwLock::new(HashSet::from([
            "emphasis".to_string(),
            "user_intent".to_string(),
            "confirmed_fact".to_string(),
        ]));

        Self {
            memory_bus,
            l0_store,
            blackboard,
            projection,
            critical_tags,
        }
    }

    #[instrument(skip(self, tags))]
    pub async fn on_l2_write(
        &self,
        node_iri: &str,
        task_iri: &str,
        tags: &[String],
    ) -> Result<(), CoreError> {
        let strategy = self.determine_write_strategy(tags);

        self.blackboard.mark_dirty(node_iri);
        debug!(node_iri = %node_iri, strategy = ?strategy, "L2 写入: 标记脏节点");

        if strategy == WriteStrategy::WriteThrough {
            let flushed = self.blackboard.flush_dirty_nodes(&self.l0_store)?;
            debug!(node_iri = %node_iri, flushed = flushed, "WriteThrough: 脏节点已写回 L0");
        }

        self.memory_bus.publish_invalidate(node_iri, task_iri).await;

        self.projection.invalidate_by_node(node_iri);

        Ok(())
    }

    #[instrument(skip(self))]
    pub fn on_l2_read(&self, node_iri: &str) -> Result<(), CoreError> {
        if let Some(node) = self.blackboard.read_node(node_iri)? {
            match node.mesi_state {
                MesiState::Modified => {
                    debug!(node_iri = %node_iri, "L2 读取: Modified 状态, 不改变");
                }
                MesiState::Invalid => {
                    debug!(node_iri = %node_iri, "L2 读取: Invalid 状态, 从 L0 重载");
                    self.blackboard.delete_node(node_iri)?;
                    if let Some(entry) = self.l0_store.retrieve(node_iri)? {
                        let config = CoreConfig::default();
                        self.blackboard.write_node(node_iri, &entry.content, &config)?;
                        debug!(node_iri = %node_iri, "L2 读取: 已从 L0 重载节点");
                    } else {
                        warn!(node_iri = %node_iri, "L2 读取: L0 中未找到对应条目");
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn on_l0_update(&self, iri: &str) -> Result<(), CoreError> {
        self.l0_store.update_mesi_state(iri, MesiState::Modified)?;
        debug!(iri = %iri, "L0 更新: MESI 状态设为 Modified");

        self.memory_bus.publish_invalidate(iri, iri).await;

        self.projection.invalidate_by_node(iri);

        Ok(())
    }

    pub fn determine_write_strategy(&self, tags: &[String]) -> WriteStrategy {
        let critical = self.critical_tags.read();
        let has_critical = tags.iter().any(|t| critical.contains(t));
        if has_critical {
            WriteStrategy::WriteThrough
        } else {
            WriteStrategy::WriteBack
        }
    }

    pub fn add_critical_tag(&self, tag: &str) {
        self.critical_tags.write().insert(tag.to_string());
    }

    pub fn remove_critical_tag(&self, tag: &str) {
        self.critical_tags.write().remove(tag);
    }
}
