use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_bus::MemoryBus;
use crate::{CoreConfig, CoreError};

struct PrefetchTask {
    entity_iri: String,
    intent: String,
    priority: f64,
}

pub struct PrefetchEngine {
    memory_bus: Arc<MemoryBus>,
    blackboard: Arc<Blackboard>,
    projection: Arc<ProjectionEngine>,
    queue: RwLock<VecDeque<PrefetchTask>>,
    entity_graph: RwLock<HashMap<String, Vec<String>>>,
    semaphore: Arc<Semaphore>,
    max_hops: usize,
    top_k: usize,
}

impl PrefetchEngine {
    pub fn new(
        memory_bus: Arc<MemoryBus>,
        blackboard: Arc<Blackboard>,
        projection: Arc<ProjectionEngine>,
    ) -> Self {
        Self {
            memory_bus,
            blackboard,
            projection,
            queue: RwLock::new(VecDeque::new()),
            entity_graph: RwLock::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(3)),
            max_hops: 2,
            top_k: 5,
        }
    }

    pub async fn on_intent_change(&self, new_intent: &str, current_entities: &[String]) {
        let mut candidates: HashMap<String, f64> = HashMap::new();

        for entity_iri in current_entities {
            let related = self.get_related_entities(entity_iri, self.max_hops);
            for (related_iri, score) in related {
                if current_entities.contains(&related_iri) {
                    continue;
                }
                *candidates.entry(related_iri).or_default() += score;
            }
        }

        let mut sorted: Vec<(String, f64)> = candidates.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let top_candidates: Vec<(String, f64)> = sorted.into_iter().take(self.top_k).collect();

        for (entity_iri, priority) in &top_candidates {
            let task = PrefetchTask {
                entity_iri: entity_iri.clone(),
                intent: new_intent.to_string(),
                priority: *priority,
            };
            self.queue.write().push_back(task);

            self.memory_bus
                .emit_prefetch_request(entity_iri, new_intent)
                .await;
        }

        debug!(
            intent = %new_intent,
            candidates = top_candidates.len(),
            "意图变更触发预取"
        );
    }

    pub fn on_new_entity(&self, entity_iri: &str) {
        let task = PrefetchTask {
            entity_iri: entity_iri.to_string(),
            intent: "new_entity".to_string(),
            priority: 0.5,
        };
        self.queue.write().push_back(task);

        let has_relations = self.entity_graph.read().contains_key(entity_iri);
        if has_relations {
            let related = self.get_related_entities(entity_iri, 1);
            for (related_iri, _) in related {
                let sub_task = PrefetchTask {
                    entity_iri: related_iri,
                    intent: "new_entity_cascade".to_string(),
                    priority: 0.3,
                };
                self.queue.write().push_back(sub_task);
            }
        }

        debug!(entity_iri = %entity_iri, "新实体加入预取队列");
    }

    pub async fn execute_prefetch(&self) -> Result<usize, CoreError> {
        let tasks: Vec<PrefetchTask> = {
            let mut queue = self.queue.write();
            queue.drain(..).collect()
        };

        if tasks.is_empty() {
            return Ok(0);
        }

        info!(task_count = tasks.len(), "开始执行预取");

        let config = CoreConfig::default();
        let mut handles = Vec::new();

        for task in tasks {
            let semaphore = self.semaphore.clone();
            let blackboard = self.blackboard.clone();
            let projection = self.projection.clone();
            let config = config.clone();

            let handle = tokio::spawn(async move {
                let _permit = match semaphore.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return false,
                };

                let result = projection
                    .project(&task.entity_iri, "reference_only", HashMap::new())
                    .await;

                match result {
                    Ok(projection_json) => {
                        if let Ok(parsed) =
                            serde_json::from_str::<serde_json::Value>(&projection_json)
                        {
                            if let Some(artifacts) =
                                parsed.get("artifacts").and_then(|a| a.as_array())
                            {
                                for artifact in artifacts {
                                    if let Some(iri) =
                                        artifact.get("@id").and_then(|v| v.as_str())
                                    {
                                        let node_json =
                                            serde_json::to_string(artifact).unwrap_or_default();
                                        if !node_json.is_empty() {
                                            let _ = blackboard.write_node(iri, &node_json, &config);
                                        }
                                    }
                                }
                            }
                        }
                        true
                    }
                    Err(e) => {
                        warn!(entity_iri = %task.entity_iri, error = %e, "预取投影失败");
                        false
                    }
                }
            });

            handles.push(handle);
        }

        let mut prefetched = 0usize;
        for handle in handles {
            if let Ok(true) = handle.await {
                prefetched += 1;
            }
        }

        info!(prefetched = prefetched, "预取完成");
        Ok(prefetched)
    }

    pub fn update_entity_graph(&self, entity_iri: &str, related: &[String]) {
        let mut graph = self.entity_graph.write();
        graph.insert(entity_iri.to_string(), related.to_vec());
        for rel in related {
            graph.entry(rel.clone()).or_insert_with(Vec::new);
            if let Some(neighbors) = graph.get_mut(rel) {
                if !neighbors.contains(&entity_iri.to_string()) {
                    neighbors.push(entity_iri.to_string());
                }
            }
        }
        debug!(entity_iri = %entity_iri, related_count = related.len(), "实体图已更新");
    }

    pub fn get_related_entities(&self, entity_iri: &str, max_hops: usize) -> Vec<(String, f64)> {
        let graph = self.entity_graph.read();
        let mut visited: HashMap<String, f64> = HashMap::new();
        let mut current_hop: Vec<String> = vec![entity_iri.to_string()];
        visited.insert(entity_iri.to_string(), 1.0);

        for hop in 1..=max_hops {
            let decay = 0.5_f64.powi(hop as i32);
            let mut next_hop = Vec::new();

            for node in &current_hop {
                if let Some(neighbors) = graph.get(node) {
                    for neighbor in neighbors {
                        if !visited.contains_key(neighbor) {
                            visited.insert(neighbor.clone(), decay);
                            next_hop.push(neighbor.clone());
                        }
                    }
                }
            }

            current_hop = next_hop;
        }

        visited.remove(entity_iri);

        let mut result: Vec<(String, f64)> = visited.into_iter().collect();
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        result
    }

    pub fn queue_len(&self) -> usize {
        self.queue.read().len()
    }
}
