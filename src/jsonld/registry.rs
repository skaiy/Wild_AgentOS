//! IRI 注册表 — 中心化的 @id 管理服务
//!
//! # 设计目标
//!
//! 解决系统中 29 个文件、57 处 IRI 生成调用之间缺乏协调的问题。
//!
//! 当前状态：各子系统独立生成 `iri://` 地址，互不知晓对方的存在。
//! IriRegistry 提供一个中心注册表，回答"这个 @id 的数据在哪里？"。
//!
//! # 存储
//!
//! - 权威数据：Oxigraph 的 `graph:registry` Named Graph（持久化）
//! - 读缓存：DashMap（避免每次 resolve 都查 SPARQL）
//!
//! # 有意简化
//!
//! - 非阻塞注册：失败不阻止主流程，注册是侧效（side effect）而非主路径
//! - 不处理分布式协调：单进程注册，未来可扩展到多进程

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use serde::{Deserialize, Serialize};

/// 实体所在位置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityLocation {
    pub iri: String,
    pub namespace: String,
    pub named_graph: Option<String>,
    pub storage_layer: StorageLayer,
    pub entity_type: Option<String>,
    pub created_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageLayer {
    L0Permanent,
    L1Session,
    L2Blackboard,
    L3Projection,
    KnowledgeGraph,
    External,
}

impl StorageLayer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::L0Permanent => "L0Permanent",
            Self::L1Session => "L1Session",
            Self::L2Blackboard => "L2Blackboard",
            Self::L3Projection => "L3Projection",
            Self::KnowledgeGraph => "KnowledgeGraph",
            Self::External => "External",
        }
    }
}

/// IRI 冲突检测结果
#[derive(Debug, Clone)]
pub struct IriConflict {
    pub iri: String,
    pub locations: Vec<EntityLocation>,
}

/// 注册结果
#[derive(Debug, Clone)]
pub struct RegistrationResult {
    pub is_new: bool,
    pub conflicts: Vec<IriConflict>,
}

/// 中心 IRI 注册表
///
/// 所有注册信息持久化在 Oxigraph 的 `graph:registry` Named Graph 中，
/// 同时使用 DashMap 提供 O(1) 的读缓存。
pub struct IriRegistry {
    store: Arc<Store>,
    registry_graph: String,
    local_cache: DashMap<String, Vec<EntityLocation>>,
}

impl IriRegistry {
    /// 使用共享 Oxigraph Store 初始化注册表
    pub fn with_shared_store(store: Arc<Store>) -> Self {
        Self {
            store,
            registry_graph: "graph:registry".to_string(),
            local_cache: DashMap::new(),
        }
    }

    /// 注册一个实体 IRI
    ///
    /// 如果 IRI 已在其他 Named Graph 中出现过，返回冲突信息。
    /// 注册是"尽力而为"的——失败不影响主流程。
    pub fn register(&self, iri: &str, location: EntityLocation) -> RegistrationResult {
        let existing = self.resolve_impl(iri);

        let conflicts: Vec<IriConflict> = if let Some(ref locations) = existing {
            let same_storage: Vec<EntityLocation> = locations
                .iter()
                .filter(|l| l.storage_layer == location.storage_layer)
                .cloned()
                .collect();
            if same_storage.is_empty() && locations.len() >= 1 {
                vec![IriConflict {
                    iri: iri.to_string(),
                    locations: locations.clone(),
                }]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let is_new = existing.is_none();

        // 写入 Oxigraph
        let _ = self.insert_to_store(iri, &location);

        // 写入本地缓存
        let mut entry = existing.unwrap_or_default();
        entry.push(location);
        self.local_cache.insert(iri.to_string(), entry);

        RegistrationResult { is_new, conflicts }
    }

    /// 通过 IRI 查询所有已知位置
    pub fn resolve(&self, iri: &str) -> Vec<EntityLocation> {
        self.resolve_impl(iri).unwrap_or_default()
    }

    fn resolve_impl(&self, iri: &str) -> Option<Vec<EntityLocation>> {
        // 先查本地缓存
        if let Some(cached) = self.local_cache.get(iri) {
            return Some(cached.clone());
        }

        // 查 SPARQL
        let results = self.query_store(iri).ok()?;
        if results.is_empty() {
            return None;
        }

        let locations: Vec<EntityLocation> = results;
        self.local_cache.insert(iri.to_string(), locations.clone());
        Some(locations)
    }

    /// 按命名空间查询所有注册的 IRI
    #[allow(deprecated)]
    pub fn resolve_by_namespace(&self, namespace: &str) -> Vec<EntityLocation> {
        let sparql = format!(
            "SELECT ?iri WHERE {{
                GRAPH <{}> {{
                    ?iri <https://pdca-agent.org/vocab#namespace> \"{}\" .
                }}
            }}",
            self.registry_graph,
            escape_sparql_string(namespace),
        );

        let results = self.store.query(&sparql);
        let mut locations = Vec::new();
        let solutions = match results {
            Ok(QueryResults::Solutions(s)) => s,
            _ => return locations,
        };
        for sol in solutions.flatten() {
            // Oxigraph 使用 SELECT 变量名查找（不带 ? 前缀）
            let iri_term = sol.get("iri").or_else(|| sol.get("?iri"));
            if let Some(iri) = iri_term {
                let iri_clean = iri
                    .to_string()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string();
                locations.push(EntityLocation {
                    iri: iri_clean,
                    namespace: namespace.to_string(),
                    named_graph: None,
                    storage_layer: StorageLayer::L2Blackboard,
                    entity_type: None,
                    created_at: Utc::now(),
                    metadata: HashMap::new(),
                });
            }
        }
        locations
    }

    /// 按实体类型查询
    #[allow(deprecated)]
    pub fn resolve_by_type(&self, entity_type: &str) -> Vec<EntityLocation> {
        let sparql = format!(
            "SELECT ?iri WHERE {{
                GRAPH <{}> {{
                    ?iri <https://pdca-agent.org/vocab#entityType> \"{}\" .
                }}
            }}",
            self.registry_graph,
            escape_sparql_string(entity_type),
        );

        let results = self.store.query(&sparql);
        let mut locations = Vec::new();
        let solutions = match results {
            Ok(QueryResults::Solutions(s)) => s,
            _ => return locations,
        };
        for sol in solutions.flatten() {
            let iri_term = sol.get("iri").or_else(|| sol.get("?iri"));
            if let Some(iri) = iri_term {
                let iri_clean = iri
                    .to_string()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string();
                locations.push(EntityLocation {
                    iri: iri_clean,
                    namespace: String::new(),
                    named_graph: None,
                    storage_layer: StorageLayer::L2Blackboard,
                    entity_type: Some(entity_type.to_string()),
                    created_at: Utc::now(),
                    metadata: HashMap::new(),
                });
            }
        }
        locations
    }

    /// 检测所有重复 IRI（同一 @id 出现在多个 Named Graph）
    #[allow(deprecated)]
    pub fn find_duplicates(&self) -> Vec<IriConflict> {
        let sparql = format!(
            "SELECT ?iri (COUNT(DISTINCT ?namedGraph) AS ?graphCount) WHERE {{
                GRAPH <{}> {{ ?iri <https://pdca-agent.org/vocab#namedGraph> ?namedGraph . }}
            }} GROUP BY ?iri HAVING (?graphCount > 1)",
            self.registry_graph
        );

        match self.store.query(&sparql) {
            Ok(QueryResults::Solutions(solutions)) => {
                let mut conflicts = Vec::new();
                for sol in solutions.flatten() {
                    if let Some(iri_val) = sol.get("?iri") {
                        let iri = iri_val.to_string().trim_start_matches('<').trim_end_matches('>').to_string();
                        let locations = self.resolve_impl(&iri).unwrap_or_default();
                        if locations.len() > 1 {
                            conflicts.push(IriConflict { iri, locations });
                        }
                    }
                }
                conflicts
            }
            _ => Vec::new(),
        }
    }

    /// 查询注册表大小
    pub fn size(&self) -> usize {
        self.local_cache.len()
    }

    // ─── 内部方法 ───

    fn insert_to_store(&self, iri: &str, location: &EntityLocation) -> Result<(), String> {
        let sparql = format!(
            "INSERT DATA {{
                GRAPH <{}> {{
                    <{}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://pdca-agent.org/vocab#RegisteredEntity> ;
                        <https://pdca-agent.org/vocab#namespace> \"{}\" ;
                        <https://pdca-agent.org/vocab#storageLayer> \"{}\" ;
                        <https://pdca-agent.org/vocab#namedGraph> \"{}\" ;
                        <https://pdca-agent.org/vocab#entityType> \"{}\" ;
                        <https://pdca-agent.org/vocab#createdAt> \"{}\"^^<http://www.w3.org/2001/XMLSchema#dateTime> .
                }}
            }}",
            self.registry_graph,
            iri,
            escape_sparql_string(&location.namespace),
            location.storage_layer.as_str(),
            location.named_graph.as_deref().unwrap_or(""),
            location.entity_type.as_deref().unwrap_or(""),
            location.created_at.to_rfc3339(),
        );

        self.store
            .update(&sparql)
            .map_err(|e| format!("SPARQL INSERT 失败: {}", e))
    }

    #[allow(deprecated)]
    fn query_store(&self, iri: &str) -> Result<Vec<EntityLocation>, String> {
        let sparql = format!(
            "SELECT ?namespace ?namedGraph ?storageLayer ?entityType ?createdAt WHERE {{
                GRAPH <{}> {{ <{}> <https://pdca-agent.org/vocab#namespace> ?namespace ;
                    <https://pdca-agent.org/vocab#storageLayer> ?storageLayer .
                OPTIONAL {{ <{}> <https://pdca-agent.org/vocab#namedGraph> ?namedGraph . }}
                OPTIONAL {{ <{}> <https://pdca-agent.org/vocab#entityType> ?entityType . }}
                OPTIONAL {{ <{}> <https://pdca-agent.org/vocab#createdAt> ?createdAt . }}
                }}
            }}",
            self.registry_graph,
            iri, iri, iri, iri
        );

        let results = self
            .store
            .query(&sparql)
            .map_err(|e| format!("SPARQL 查询失败: {}", e))?;

        let mut locations = Vec::new();
        if let QueryResults::Solutions(solutions) = results {
            for sol in solutions.flatten() {
                locations.push(EntityLocation {
                    iri: iri.to_string(),
                    namespace: sol
                        .get("?namespace")
                        .map(|v| v.to_string().trim_matches('"').to_string())
                        .unwrap_or_default(),
                    named_graph: sol
                        .get("?namedGraph")
                        .map(|v| v.to_string().trim_matches('"').to_string()),
                    storage_layer: match sol
                        .get("?storageLayer")
                        .map(|v| v.to_string().trim_matches('"').to_string())
                        .as_deref()
                    {
                        Some("L0Permanent") => StorageLayer::L0Permanent,
                        Some("L1Session") => StorageLayer::L1Session,
                        Some("L2Blackboard") => StorageLayer::L2Blackboard,
                        Some("L3Projection") => StorageLayer::L3Projection,
                        Some("KnowledgeGraph") => StorageLayer::KnowledgeGraph,
                        _ => StorageLayer::External,
                    },
                    entity_type: sol
                        .get("?entityType")
                        .map(|v| v.to_string().trim_matches('"').to_string()),
                    created_at: Utc::now(),
                    metadata: HashMap::new(),
                });
            }
        }
        Ok(locations)
    }

    /// 清除 IRI 的本地缓存条目
    pub fn invalidate_cache(&self, iri: &str) {
        self.local_cache.remove(iri);
    }
}

fn escape_sparql_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::store::Store;

    fn test_store() -> Arc<Store> {
        Arc::new(Store::new().unwrap())
    }

    fn make_location(iri: &str, ns: &str, storage: StorageLayer) -> EntityLocation {
        EntityLocation {
            iri: iri.to_string(),
            namespace: ns.to_string(),
            named_graph: Some(format!("graph:{}", ns)),
            storage_layer: storage,
            entity_type: None,
            created_at: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_register_and_resolve() {
        let registry = IriRegistry::with_shared_store(test_store());
        let iri = "iri://task/test-123";

        let loc = make_location(iri, "task", StorageLayer::L2Blackboard);
        let result = registry.register(iri, loc.clone());
        assert!(result.is_new);
        assert!(result.conflicts.is_empty());

        let resolved = registry.resolve(iri);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].namespace, "task");
    }

    #[test]
    fn test_duplicate_detection() {
        let registry = IriRegistry::with_shared_store(test_store());

        let loc1 = make_location("iri://task/dup", "task", StorageLayer::L0Permanent);
        let loc2 = make_location("iri://task/dup", "memory", StorageLayer::L2Blackboard);

        registry.register("iri://task/dup", loc1);
        let result = registry.register("iri://task/dup", loc2);

        assert!(!result.is_new);
        assert!(!result.conflicts.is_empty(), "不同 StorageLayer 应触发冲突");
    }

    #[test]
    fn test_resolve_by_namespace() {
        let registry = IriRegistry::with_shared_store(test_store());

        registry.register(
            "iri://task/a",
            make_location("iri://task/a", "task", StorageLayer::L2Blackboard),
        );

        let tasks = registry.resolve_by_namespace("task");
        assert_eq!(tasks.len(), 1, "should find 1 entity in 'task' namespace");

        let empty = registry.resolve_by_namespace("nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_sparql_integration() {
        let registry = IriRegistry::with_shared_store(test_store());

        registry.register(
            "iri://task/a",
            make_location("iri://task/a", "task", StorageLayer::L2Blackboard),
        );
        registry.register(
            "iri://skills/b",
            make_location("iri://skills/b", "skills", StorageLayer::L0Permanent),
        );

        let tasks = registry.resolve_by_namespace("task");
        assert_eq!(tasks.len(), 1, "register + SPARQL should find 1 entity in 'task'");
    }

    #[test]
    fn test_cache_and_invalidate() {
        let registry = IriRegistry::with_shared_store(test_store());
        let iri = "iri://task/cached";

        registry.register(iri, make_location(iri, "task", StorageLayer::L0Permanent));
        assert_eq!(registry.size(), 1);

        registry.invalidate_cache(iri);
        assert_eq!(registry.size(), 0, "invalidate 后 size 应为 0");
    }

    #[test]
    fn test_register_existing_location_duplicate() {
        let registry = IriRegistry::with_shared_store(test_store());
        let iri = "iri://task/same";

        let loc = make_location(iri, "task", StorageLayer::L0Permanent);
        registry.register(iri, loc.clone());

        let result = registry.register(iri, loc);
        assert!(!result.is_new);
    }
}
