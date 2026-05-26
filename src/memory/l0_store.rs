//! L0 Store - 长期记忆持久化存储
//!
//! 本模块处理记忆和知识的持久化存储，支持 MESI 缓存一致性状态和标签二级索引。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{debug, info};

use crate::CoreError;

/// MESI 缓存一致性状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MesiState {
    Modified,
    Exclusive,
    Shared,
    Invalid,
}

impl Default for MesiState {
    fn default() -> Self {
        MesiState::Shared
    }
}

/// L0 记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L0Entry {
    pub iri: String,
    pub content: String,
    pub importance: f32,
    pub access_count: u32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub mesi_state: MesiState,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub named_graph: Option<String>,
    #[serde(default)]
    pub qdrant_point_id: Option<String>,
    #[serde(default)]
    pub jsonld_context: Option<String>,
    #[serde(default)]
    pub jsonld_types: Vec<String>,
}

/// L0 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L0SearchResult {
    pub iri: String,
    pub content: String,
    pub relevance_score: f32,
    pub importance: f32,
    pub tags: Vec<String>,
}

/// L0 Store 配置
#[derive(Debug, Clone)]
pub struct L0Config {
    pub path: String,
    pub max_entries: usize,
    pub compression: bool,
}

impl Default for L0Config {
    fn default() -> Self {
        Self {
            path: "./data/l0_store".to_string(),
            max_entries: 1_000_000,
            compression: true,
        }
    }
}

/// 计算内容的哈希值
fn compute_content_hash(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// L0 Store
pub struct L0Store {
    db: Db,
    tag_index: sled::Tree,
    named_graph_index: sled::Tree,
    #[allow(dead_code)]
    config: L0Config,
    entry_count: u64,
}

impl L0Store {
    pub fn new(path: &str) -> Result<Self, CoreError> {
        info!("初始化 L0 Store: {}", path);

        std::fs::create_dir_all(path)
            .map_err(|e| CoreError::StorageError {
                message: format!("创建存储目录失败: {}", e),
            })?;

        let db = sled::open(path)
            .map_err(|e| CoreError::StorageError {
                message: format!("打开数据库失败: {}", e),
            })?;

        let tag_index = db.open_tree("tag_index")
            .map_err(|e| CoreError::StorageError {
                message: format!("打开标签索引失败: {}", e),
            })?;

        let named_graph_index = db.open_tree("named_graph_index")
            .map_err(|e| CoreError::StorageError {
                message: format!("打开命名图索引失败: {}", e),
            })?;

        let entry_count = db.len();

        Ok(Self {
            db,
            tag_index,
            named_graph_index,
            config: L0Config {
                path: path.to_string(),
                ..Default::default()
            },
            entry_count: entry_count as u64,
        })
    }

    /// 更新标签索引：先删除旧标签索引，再插入新标签索引
    fn update_tag_index(&self, iri: &str, old_tags: &[String], new_tags: &[String]) -> Result<(), CoreError> {
        for tag in old_tags {
            let index_key = format!("tag:{}", tag);
            self.remove_iri_from_tag_index(&index_key, iri)?;
        }
        for tag in new_tags {
            let index_key = format!("tag:{}", tag);
            self.add_iri_to_tag_index(&index_key, iri)?;
        }
        Ok(())
    }

    /// 向标签索引添加 IRI
    fn add_iri_to_tag_index(&self, index_key: &str, iri: &str) -> Result<(), CoreError> {
        let key = index_key.as_bytes();
        let mut iris: Vec<String> = match self.tag_index.get(key) {
            Ok(Some(value)) => serde_json::from_slice(&value).unwrap_or_default(),
            _ => Vec::new(),
        };
        if !iris.contains(&iri.to_string()) {
            iris.push(iri.to_string());
        }
        let value = serde_json::to_vec(&iris)
            .map_err(|e| CoreError::StorageError {
                message: format!("序列化标签索引失败: {}", e),
            })?;
        self.tag_index.insert(key, value)
            .map_err(|e| CoreError::StorageError {
                message: format!("写入标签索引失败: {}", e),
            })?;
        Ok(())
    }

    /// 从标签索引移除 IRI
    fn remove_iri_from_tag_index(&self, index_key: &str, iri: &str) -> Result<(), CoreError> {
        let key = index_key.as_bytes();
        let mut iris: Vec<String> = match self.tag_index.get(key) {
            Ok(Some(value)) => serde_json::from_slice(&value).unwrap_or_default(),
            _ => return Ok(()),
        };
        iris.retain(|i| i != iri);
        if iris.is_empty() {
            self.tag_index.remove(key)
                .map_err(|e| CoreError::StorageError {
                    message: format!("删除标签索引失败: {}", e),
                })?;
        } else {
            let value = serde_json::to_vec(&iris)
                .map_err(|e| CoreError::StorageError {
                    message: format!("序列化标签索引失败: {}", e),
                })?;
            self.tag_index.insert(key, value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("写入标签索引失败: {}", e),
                })?;
        }
        Ok(())
    }

    pub fn store(&self, iri: &str, content: &str) -> Result<(), CoreError> {
        let content_hash = compute_content_hash(content);

        let existing_entry = self.retrieve_without_update(iri)?;
        
        let entry = if let Some(existing) = existing_entry {
            let new_entry = L0Entry {
                iri: iri.to_string(),
                content: content.to_string(),
                importance: 0.5,
                access_count: 0,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                tags: Vec::new(),
                metadata: serde_json::Map::new(),
                mesi_state: MesiState::Shared,
                content_hash,
                named_graph: None,
                qdrant_point_id: None,
                jsonld_context: None,
                jsonld_types: Vec::new(),
            };
            Self::merge_entries(&existing, &new_entry)
        } else {
            L0Entry {
                iri: iri.to_string(),
                content: content.to_string(),
                importance: 0.5,
                access_count: 0,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                tags: Vec::new(),
                metadata: serde_json::Map::new(),
                mesi_state: MesiState::Shared,
                content_hash,
                named_graph: None,
                qdrant_point_id: None,
                jsonld_context: None,
                jsonld_types: Vec::new(),
            }
        };

        let key = entry.iri.as_bytes();
        let value = serde_json::to_vec(&entry)
            .map_err(|e| CoreError::StorageError {
                message: format!("序列化条目失败: {}", e),
            })?;

        self.db.insert(key, value)
            .map_err(|e| CoreError::StorageError {
                message: format!("存储条目失败: {}", e),
            })?;

        debug!(iri = %entry.iri, "条目已存储到 L0");
        Ok(())
    }

    fn retrieve_without_update(&self, iri: &str) -> Result<Option<L0Entry>, CoreError> {
        let key = iri.as_bytes();
        let value = self.db.get(key)
            .map_err(|e| CoreError::StorageError {
                message: format!("检索条目失败: {}", e),
            })?;

        match value {
            Some(v) => {
                let entry: L0Entry = serde_json::from_slice(&v)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化条目失败: {}", e),
                    })?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    fn merge_entries(existing: &L0Entry, new: &L0Entry) -> L0Entry {
        let mut merged_metadata = existing.metadata.clone();
        for (key, value) in &new.metadata {
            merged_metadata.insert(key.clone(), value.clone());
        }

        let mut merged_tags = existing.tags.clone();
        for tag in &new.tags {
            if !merged_tags.contains(tag) {
                merged_tags.push(tag.clone());
            }
        }

        let mut merged_types = existing.jsonld_types.clone();
        for type_iri in &new.jsonld_types {
            if !merged_types.contains(type_iri) {
                merged_types.push(type_iri.clone());
            }
        }

        L0Entry {
            iri: existing.iri.clone(),
            content: new.content.clone(),
            importance: (existing.importance + new.importance) / 2.0,
            access_count: existing.access_count,
            created_at: existing.created_at,
            last_accessed: Utc::now(),
            tags: merged_tags,
            metadata: merged_metadata,
            mesi_state: new.mesi_state.clone(),
            content_hash: new.content_hash.clone(),
            named_graph: existing.named_graph.clone().or(new.named_graph.clone()),
            qdrant_point_id: existing.qdrant_point_id.clone().or(new.qdrant_point_id.clone()),
            jsonld_context: new.jsonld_context.clone().or(existing.jsonld_context.clone()),
            jsonld_types: merged_types,
        }
    }

    pub fn store_entry(&self, entry: &L0Entry) -> Result<(), CoreError> {
        let old_tags = self.get_entry_tags(&entry.iri)?;
        let old_named_graph = self.get_entry_named_graph(&entry.iri)?;

        let content_hash = if entry.content_hash.is_empty() {
            compute_content_hash(&entry.content)
        } else {
            entry.content_hash.clone()
        };

        let existing_entry = self.retrieve_without_update(&entry.iri)?;
        let entry = if let Some(existing) = existing_entry {
            let entry_with_hash = L0Entry {
                content_hash,
                ..entry.clone()
            };
            Self::merge_entries(&existing, &entry_with_hash)
        } else {
            L0Entry {
                content_hash,
                ..entry.clone()
            }
        };

        self.update_tag_index(&entry.iri, &old_tags, &entry.tags)?;
        
        if old_named_graph != entry.named_graph {
            if let Some(ref old_graph) = old_named_graph {
                self.remove_iri_from_named_graph_index(old_graph, &entry.iri)?;
            }
            if let Some(ref new_graph) = entry.named_graph {
                self.add_iri_to_named_graph_index(new_graph, &entry.iri)?;
            }
        }

        let key = entry.iri.as_bytes();
        let value = serde_json::to_vec(&entry)
            .map_err(|e| CoreError::StorageError {
                message: format!("序列化条目失败: {}", e),
            })?;

        self.db.insert(key, value)
            .map_err(|e| CoreError::StorageError {
                message: format!("存储条目失败: {}", e),
            })?;

        debug!(iri = %entry.iri, "条目已存储到 L0");
        Ok(())
    }

    fn get_entry_named_graph(&self, iri: &str) -> Result<Option<String>, CoreError> {
        let key = iri.as_bytes();
        match self.db.get(key) {
            Ok(Some(value)) => {
                let entry: L0Entry = serde_json::from_slice(&value)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化条目失败: {}", e),
                    })?;
                Ok(entry.named_graph)
            }
            _ => Ok(None),
        }
    }

    fn add_iri_to_named_graph_index(&self, graph: &str, iri: &str) -> Result<(), CoreError> {
        let key = format!("graph:{}", graph);
        let mut iris: Vec<String> = match self.named_graph_index.get(key.as_bytes()) {
            Ok(Some(value)) => serde_json::from_slice(&value).unwrap_or_default(),
            _ => Vec::new(),
        };
        if !iris.contains(&iri.to_string()) {
            iris.push(iri.to_string());
        }
        let value = serde_json::to_vec(&iris)
            .map_err(|e| CoreError::StorageError {
                message: format!("序列化命名图索引失败: {}", e),
            })?;
        self.named_graph_index.insert(key.as_bytes(), value)
            .map_err(|e| CoreError::StorageError {
                message: format!("写入命名图索引失败: {}", e),
            })?;
        Ok(())
    }

    fn remove_iri_from_named_graph_index(&self, graph: &str, iri: &str) -> Result<(), CoreError> {
        let key = format!("graph:{}", graph);
        let mut iris: Vec<String> = match self.named_graph_index.get(key.as_bytes()) {
            Ok(Some(value)) => serde_json::from_slice(&value).unwrap_or_default(),
            _ => return Ok(()),
        };
        iris.retain(|i| i != iri);
        if iris.is_empty() {
            self.named_graph_index.remove(key.as_bytes())
                .map_err(|e| CoreError::StorageError {
                    message: format!("删除命名图索引失败: {}", e),
                })?;
        } else {
            let value = serde_json::to_vec(&iris)
                .map_err(|e| CoreError::StorageError {
                    message: format!("序列化命名图索引失败: {}", e),
                })?;
            self.named_graph_index.insert(key.as_bytes(), value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("写入命名图索引失败: {}", e),
                })?;
        }
        Ok(())
    }

    /// 获取条目的现有标签（用于索引更新）
    fn get_entry_tags(&self, iri: &str) -> Result<Vec<String>, CoreError> {
        let key = iri.as_bytes();
        match self.db.get(key) {
            Ok(Some(value)) => {
                let entry: L0Entry = serde_json::from_slice(&value)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化条目失败: {}", e),
                    })?;
                Ok(entry.tags)
            }
            _ => Ok(Vec::new()),
        }
    }

    pub fn retrieve(&self, iri: &str) -> Result<Option<L0Entry>, CoreError> {
        let key = iri.as_bytes();

        let value = self.db.get(key)
            .map_err(|e| CoreError::StorageError {
                message: format!("检索条目失败: {}", e),
            })?;

        match value {
            Some(v) => {
                let mut entry: L0Entry = serde_json::from_slice(&v)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化条目失败: {}", e),
                    })?;

                entry.access_count += 1;
                entry.last_accessed = Utc::now();
                self.store_entry(&entry)?;

                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    pub fn delete(&self, iri: &str) -> Result<bool, CoreError> {
        let old_tags = self.get_entry_tags(iri)?;

        for tag in &old_tags {
            let index_key = format!("tag:{}", tag);
            self.remove_iri_from_tag_index(&index_key, iri)?;
        }

        let key = iri.as_bytes();
        let removed = self.db.remove(key)
            .map_err(|e| CoreError::StorageError {
                message: format!("删除条目失败: {}", e),
            })?;
        Ok(removed.is_some())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<L0SearchResult>, CoreError> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        for item in self.db.iter() {
            let (_, value) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代失败: {}", e),
                })?;

            let entry: L0Entry = serde_json::from_slice(&value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("反序列化条目失败: {}", e),
                })?;

            let content_lower = entry.content.to_lowercase();
            let tag_match = entry.tags.iter().any(|t| t.to_lowercase().contains(&query_lower));
            let content_match = content_lower.contains(&query_lower);

            if tag_match || content_match {
                let relevance = if content_match { 0.8 } else { 0.5 };
                results.push(L0SearchResult {
                    iri: entry.iri,
                    content: entry.content,
                    relevance_score: relevance,
                    importance: entry.importance,
                    tags: entry.tags,
                });
            }

            if results.len() >= limit {
                break;
            }
        }

        results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());
        Ok(results)
    }

    /// 使用标签索引搜索，索引未命中时回退到全表扫描
    pub fn search_with_index(&self, tags: &[String]) -> Result<Vec<L0Entry>, CoreError> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }

        let mut index_hit = true;
        let mut candidate_iris: Vec<String> = Vec::new();

        for tag in tags {
            let index_key = format!("tag:{}", tag);
            match self.tag_index.get(index_key.as_bytes()) {
                Ok(Some(value)) => {
                    let iris: Vec<String> = serde_json::from_slice(&value).unwrap_or_default();
                    if candidate_iris.is_empty() {
                        candidate_iris = iris;
                    } else {
                        let iris_set: std::collections::HashSet<_> = iris.into_iter().collect();
                        candidate_iris.retain(|iri| iris_set.contains(iri));
                    }
                }
                _ => {
                    index_hit = false;
                    break;
                }
            }
        }

        if index_hit {
            let mut results = Vec::new();
            for iri in &candidate_iris {
                if let Some(entry) = self.retrieve(iri)? {
                    results.push(entry);
                }
            }
            Ok(results)
        } else {
            self.search_by_tags_fallback(tags)
        }
    }

    /// 全表扫描标签搜索（回退方案）
    fn search_by_tags_fallback(&self, tags: &[String]) -> Result<Vec<L0Entry>, CoreError> {
        let mut results = Vec::new();

        for item in self.db.iter() {
            let (_, value) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代失败: {}", e),
                })?;

            let entry: L0Entry = serde_json::from_slice(&value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("反序列化条目失败: {}", e),
                })?;

            if tags.iter().all(|t| entry.tags.contains(t)) {
                results.push(entry);
            }
        }

        Ok(results)
    }

    pub fn search_by_tags(&self, tags: &[String]) -> Result<Vec<L0Entry>, CoreError> {
        self.search_with_index(tags)
    }

    pub fn get_by_importance(&self, min_importance: f32) -> Result<Vec<L0Entry>, CoreError> {
        let mut results = Vec::new();

        for item in self.db.iter() {
            let (_, value) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代失败: {}", e),
                })?;

            let entry: L0Entry = serde_json::from_slice(&value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("反序列化条目失败: {}", e),
                })?;

            if entry.importance >= min_importance {
                results.push(entry);
            }
        }

        results.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap());
        Ok(results)
    }

    /// 更新条目的 MESI 缓存一致性状态
    pub fn update_mesi_state(&self, iri: &str, state: MesiState) -> Result<(), CoreError> {
        let key = iri.as_bytes();

        let value = self.db.get(key)
            .map_err(|e| CoreError::StorageError {
                message: format!("检索条目失败: {}", e),
            })?;

        match value {
            Some(v) => {
                let mut entry: L0Entry = serde_json::from_slice(&v)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化条目失败: {}", e),
                    })?;
                entry.mesi_state = state;
                self.store_entry(&entry)?;
                Ok(())
            }
            None => Err(CoreError::StorageError {
                message: format!("条目不存在: {}", iri),
            }),
        }
    }

    pub fn count(&self) -> u64 {
        self.db.len() as u64
    }

    pub fn flush(&self) -> Result<(), CoreError> {
        self.db.flush()
            .map_err(|e| CoreError::StorageError {
                message: format!("刷新数据库失败: {}", e),
            })?;
        self.tag_index.flush()
            .map_err(|e| CoreError::StorageError {
                message: format!("刷新标签索引失败: {}", e),
            })?;
        self.named_graph_index.flush()
            .map_err(|e| CoreError::StorageError {
                message: format!("刷新命名图索引失败: {}", e),
            })?;
        Ok(())
    }

    /// 按命名图查询所有条目
    pub fn query_by_named_graph(&self, graph: &str) -> Result<Vec<L0Entry>, CoreError> {
        let key = format!("graph:{}", graph);
        match self.named_graph_index.get(key.as_bytes()) {
            Ok(Some(iris_bytes)) => {
                let iris: Vec<String> = serde_json::from_slice(&iris_bytes)
                    .map_err(|e| CoreError::StorageError {
                        message: format!("反序列化命名图索引失败: {}", e),
                    })?;
                let mut entries = Vec::new();
                for iri in iris {
                    if let Some(entry) = self.retrieve(&iri)? {
                        entries.push(entry);
                    }
                }
                Ok(entries)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// 删除命名图中的所有条目
    pub fn delete_named_graph(&self, graph: &str) -> Result<usize, CoreError> {
        let entries = self.query_by_named_graph(graph)?;
        let count = entries.len();

        for entry in &entries {
            self.delete(&entry.iri)?;
        }

        let key = format!("graph:{}", graph);
        self.named_graph_index.remove(key.as_bytes())
            .map_err(|e| CoreError::StorageError {
                message: format!("删除命名图索引失败: {}", e),
            })?;

        Ok(count)
    }

    /// 列出所有命名图
    pub fn list_named_graphs(&self) -> Result<Vec<String>, CoreError> {
        let mut graphs = Vec::new();
        for item in self.named_graph_index.iter() {
            let (key, _) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代命名图索引失败: {}", e),
                })?;
            let key_str = String::from_utf8_lossy(&key);
            if let Some(graph) = key_str.strip_prefix("graph:") {
                graphs.push(graph.to_string());
            }
        }
        Ok(graphs)
    }

    pub fn query_by_type(&self, type_iri: &str) -> Result<Vec<L0Entry>, CoreError> {
        let mut results = Vec::new();

        for item in self.db.iter() {
            let (_, value) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代失败: {}", e),
                })?;

            let entry: L0Entry = serde_json::from_slice(&value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("反序列化条目失败: {}", e),
                })?;

            if entry.jsonld_types.contains(&type_iri.to_string()) {
                results.push(entry);
            }
        }

        Ok(results)
    }

    pub fn query_by_types(&self, type_iris: &[String]) -> Result<Vec<L0Entry>, CoreError> {
        if type_iris.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for item in self.db.iter() {
            let (_, value) = item
                .map_err(|e| CoreError::StorageError {
                    message: format!("迭代失败: {}", e),
                })?;

            let entry: L0Entry = serde_json::from_slice(&value)
                .map_err(|e| CoreError::StorageError {
                    message: format!("反序列化条目失败: {}", e),
                })?;

            if type_iris.iter().any(|t| entry.jsonld_types.contains(t)) {
                results.push(entry);
            }
        }

        Ok(results)
    }

    pub fn store_jsonld_node(&self, node: &serde_json::Value) -> Result<String, CoreError> {
        let node_obj = node.as_object()
            .ok_or_else(|| CoreError::StorageError {
                message: "JSON-LD 节点必须是对象".to_string(),
            })?;

        let iri = node_obj.get("@id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::StorageError {
                message: "JSON-LD 节点缺少 @id 字段".to_string(),
            })?;

        let jsonld_context = node_obj.get("@context")
            .and_then(|v| serde_json::to_string(v).ok());

        let jsonld_types = node_obj.get("@type")
            .and_then(|v| {
                match v {
                    serde_json::Value::String(s) => Some(vec![s.clone()]),
                    serde_json::Value::Array(arr) => {
                        Some(arr.iter().filter_map(|item| item.as_str().map(|s| s.to_string())).collect())
                    }
                    _ => None,
                }
            })
            .unwrap_or_default();

        let content = serde_json::to_string(node)
            .map_err(|e| CoreError::StorageError {
                message: format!("序列化 JSON-LD 节点失败: {}", e),
            })?;

        let content_hash = compute_content_hash(&content);

        let existing_entry = self.retrieve_without_update(iri)?;
        
        let entry = if let Some(existing) = existing_entry {
            let mut merged_metadata = existing.metadata.clone();
            for (key, value) in node_obj.iter() {
                if key != "@id" && key != "@type" && key != "@context" {
                    merged_metadata.insert(key.clone(), value.clone());
                }
            }

            let mut merged_types = existing.jsonld_types.clone();
            for type_iri in &jsonld_types {
                if !merged_types.contains(type_iri) {
                    merged_types.push(type_iri.clone());
                }
            }

            L0Entry {
                iri: iri.to_string(),
                content,
                importance: existing.importance,
                access_count: existing.access_count,
                created_at: existing.created_at,
                last_accessed: Utc::now(),
                tags: existing.tags.clone(),
                metadata: merged_metadata,
                mesi_state: existing.mesi_state.clone(),
                content_hash,
                named_graph: existing.named_graph.clone(),
                qdrant_point_id: existing.qdrant_point_id.clone(),
                jsonld_context: jsonld_context.or(existing.jsonld_context.clone()),
                jsonld_types: merged_types,
            }
        } else {
            let mut metadata = serde_json::Map::new();
            for (key, value) in node_obj.iter() {
                if key != "@id" && key != "@type" && key != "@context" {
                    metadata.insert(key.clone(), value.clone());
                }
            }

            L0Entry {
                iri: iri.to_string(),
                content,
                importance: 0.5,
                access_count: 0,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                tags: Vec::new(),
                metadata,
                mesi_state: MesiState::Shared,
                content_hash,
                named_graph: None,
                qdrant_point_id: None,
                jsonld_context,
                jsonld_types,
            }
        };

        self.store_entry(&entry)?;

        Ok(iri.to_string())
    }

    pub fn retrieve_jsonld_node(&self, iri: &str) -> Result<Option<serde_json::Value>, CoreError> {
        match self.retrieve(iri)? {
            Some(entry) => {
                let mut node = serde_json::Map::new();
                
                node.insert("@id".to_string(), serde_json::Value::String(entry.iri.clone()));
                
                if let Some(context) = entry.jsonld_context {
                    if let Ok(context_value) = serde_json::from_str(&context) {
                        node.insert("@context".to_string(), context_value);
                    }
                }

                if !entry.jsonld_types.is_empty() {
                    if entry.jsonld_types.len() == 1 {
                        node.insert("@type".to_string(), serde_json::Value::String(entry.jsonld_types[0].clone()));
                    } else {
                        node.insert("@type".to_string(), serde_json::Value::Array(
                            entry.jsonld_types.into_iter().map(serde_json::Value::String).collect()
                        ));
                    }
                }

                for (key, value) in entry.metadata {
                    node.insert(key, value);
                }

                Ok(Some(serde_json::Value::Object(node)))
            }
            None => Ok(None),
        }
    }
}

/// 记忆压缩器，用于 L2 -> L0 归档
pub struct MemoryCompressor;

impl MemoryCompressor {
    pub fn compress_session(
        session_id: &str,
        task_id: &str,
        agent_role: &str,
        summary: &str,
    ) -> L0Entry {
        let content_hash = compute_content_hash(summary);
        L0Entry {
            iri: format!("iri://memory/{}", uuid::Uuid::new_v4().hyphenated()),
            content: summary.to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: vec![
                format!("session:{}", session_id),
                format!("task:{}", task_id),
                format!("role:{}", agent_role),
            ],
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash,
            named_graph: Some(format!("session:{}", session_id)),
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Memory".to_string()],
        }
    }

    pub fn compress_nodes(nodes: &[String]) -> String {
        format!(
            r#"{{"@type":"Summary","node_count":{},"compressed_at":"{}"}}"#,
            nodes.len(),
            Utc::now().to_rfc3339()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_l0_store() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        store.store("iri://test/1", r#"{"test": true}"#).unwrap();

        let retrieved = store.retrieve("iri://test/1").unwrap();
        assert!(retrieved.is_some());
        let entry = retrieved.unwrap();
        assert_eq!(entry.mesi_state, MesiState::Shared);
        assert!(!entry.content_hash.is_empty());
    }

    #[test]
    fn test_mesi_state_default() {
        assert_eq!(MesiState::default(), MesiState::Shared);
    }

    #[test]
    fn test_update_mesi_state() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        store.store("iri://test/mesi", "content").unwrap();
        store.update_mesi_state("iri://test/mesi", MesiState::Modified).unwrap();

        let entry = store.retrieve("iri://test/mesi").unwrap().unwrap();
        assert_eq!(entry.mesi_state, MesiState::Modified);
    }

    #[test]
    fn test_tag_index() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let entry = L0Entry {
            iri: "iri://test/tagged".to_string(),
            content: "tagged content".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: vec!["rust".to_string(), "test".to_string()],
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: Vec::new(),
        };
        store.store_entry(&entry).unwrap();

        let results = store.search_by_tags(&["rust".to_string()]).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].iri, "iri://test/tagged");
    }

    #[test]
    fn test_delete_cleans_tag_index() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let entry = L0Entry {
            iri: "iri://test/del".to_string(),
            content: "to be deleted".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: vec!["deleteme".to_string()],
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: Vec::new(),
        };
        store.store_entry(&entry).unwrap();
        store.delete("iri://test/del").unwrap();

        let results = store.search_by_tags(&["deleteme".to_string()]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_content_hash() {
        let hash1 = compute_content_hash("hello");
        let hash2 = compute_content_hash("hello");
        let hash3 = compute_content_hash("world");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_search_with_index_fallback() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        store.store("iri://test/fallback", "fallback content").unwrap();

        let results = store.search_with_index(&["nonexistent".to_string()]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_entity_alignment() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let mut entry1 = L0Entry {
            iri: "iri://test/entity".to_string(),
            content: r#"{"name": "Alice"}"#.to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: vec!["person".to_string()],
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: Some(r#"{"@vocab": "http://example.org/"}"#.to_string()),
            jsonld_types: vec!["Person".to_string()],
        };
        entry1.metadata.insert("name".to_string(), serde_json::json!("Alice"));

        store.store_entry(&entry1).unwrap();

        let mut entry2 = L0Entry {
            iri: "iri://test/entity".to_string(),
            content: r#"{"name": "Alice", "age": 30}"#.to_string(),
            importance: 0.7,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: vec!["employee".to_string()],
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Employee".to_string()],
        };
        entry2.metadata.insert("age".to_string(), serde_json::json!(30));

        store.store_entry(&entry2).unwrap();

        let merged = store.retrieve("iri://test/entity").unwrap().unwrap();
        
        assert_eq!(merged.iri, "iri://test/entity");
        assert!(merged.tags.contains(&"person".to_string()));
        assert!(merged.tags.contains(&"employee".to_string()));
        assert!(merged.jsonld_types.contains(&"Person".to_string()));
        assert!(merged.jsonld_types.contains(&"Employee".to_string()));
        assert!(merged.metadata.contains_key("name"));
        assert!(merged.metadata.contains_key("age"));
        assert_eq!(merged.importance, 0.6);
    }

    #[test]
    fn test_query_by_type() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let mut entry1 = L0Entry {
            iri: "iri://test/person1".to_string(),
            content: "Person 1".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: Vec::new(),
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Person".to_string()],
        };
        store.store_entry(&entry1).unwrap();

        let mut entry2 = L0Entry {
            iri: "iri://test/person2".to_string(),
            content: "Person 2".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: Vec::new(),
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Person".to_string(), "Employee".to_string()],
        };
        store.store_entry(&entry2).unwrap();

        let mut entry3 = L0Entry {
            iri: "iri://test/organization".to_string(),
            content: "Organization".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: Vec::new(),
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Organization".to_string()],
        };
        store.store_entry(&entry3).unwrap();

        let person_results = store.query_by_type("Person").unwrap();
        assert_eq!(person_results.len(), 2);

        let employee_results = store.query_by_type("Employee").unwrap();
        assert_eq!(employee_results.len(), 1);
        assert_eq!(employee_results[0].iri, "iri://test/person2");

        let org_results = store.query_by_type("Organization").unwrap();
        assert_eq!(org_results.len(), 1);
    }

    #[test]
    fn test_query_by_types() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let mut entry1 = L0Entry {
            iri: "iri://test/entity1".to_string(),
            content: "Entity 1".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: Vec::new(),
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Person".to_string()],
        };
        store.store_entry(&entry1).unwrap();

        let mut entry2 = L0Entry {
            iri: "iri://test/entity2".to_string(),
            content: "Entity 2".to_string(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            tags: Vec::new(),
            metadata: serde_json::Map::new(),
            mesi_state: MesiState::Shared,
            content_hash: String::new(),
            named_graph: None,
            qdrant_point_id: None,
            jsonld_context: None,
            jsonld_types: vec!["Organization".to_string()],
        };
        store.store_entry(&entry2).unwrap();

        let results = store.query_by_types(&["Person".to_string(), "Organization".to_string()]).unwrap();
        assert_eq!(results.len(), 2);

        let person_only = store.query_by_types(&["Person".to_string()]).unwrap();
        assert_eq!(person_only.len(), 1);
    }

    #[test]
    fn test_jsonld_node_storage() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let node = serde_json::json!({
            "@id": "iri://test/person/alice",
            "@type": "Person",
            "@context": {
                "@vocab": "http://example.org/"
            },
            "name": "Alice",
            "age": 30
        });

        let iri = store.store_jsonld_node(&node).unwrap();
        assert_eq!(iri, "iri://test/person/alice");

        let retrieved = store.retrieve_jsonld_node("iri://test/person/alice").unwrap();
        assert!(retrieved.is_some());

        let retrieved_node = retrieved.unwrap();
        assert_eq!(retrieved_node["@id"], "iri://test/person/alice");
        assert_eq!(retrieved_node["name"], "Alice");
        assert_eq!(retrieved_node["age"], 30);
    }

    #[test]
    fn test_jsonld_node_merge() {
        let dir = tempdir().unwrap();
        let store = L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap();

        let node1 = serde_json::json!({
            "@id": "iri://test/person/bob",
            "@type": "Person",
            "name": "Bob",
            "age": 25
        });

        store.store_jsonld_node(&node1).unwrap();

        let node2 = serde_json::json!({
            "@id": "iri://test/person/bob",
            "@type": "Employee",
            "department": "Engineering"
        });

        store.store_jsonld_node(&node2).unwrap();

        let retrieved = store.retrieve_jsonld_node("iri://test/person/bob").unwrap().unwrap();
        
        assert_eq!(retrieved["@id"], "iri://test/person/bob");
        assert_eq!(retrieved["name"], "Bob");
        assert_eq!(retrieved["age"], 25);
        assert_eq!(retrieved["department"], "Engineering");

        let types = retrieved["@type"].as_array().unwrap();
        assert!(types.contains(&serde_json::json!("Person")));
        assert!(types.contains(&serde_json::json!("Employee")));
    }
}
