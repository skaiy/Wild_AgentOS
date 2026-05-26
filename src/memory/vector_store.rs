use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_client::qdrant::{
    value::Kind, Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter,
    PointStruct, Range, SearchPointsBuilder, UpsertPointsBuilder, VectorParams,
};
use qdrant_client::Qdrant;
use serde::Deserialize;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::CoreError;

const COLLECTION: &str = "agent_os_memory";
const DEFAULT_VEC_SIZE: usize = 128;

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
    fn dimension(&self) -> usize;
}

pub struct OneApiEmbeddingService {
    client: reqwest::Client,
    api_url: String,
    api_key: String,
    model: String,
    dimension: usize,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl OneApiEmbeddingService {
    pub fn new(api_url: &str, api_key: &str, model: &str, dimension: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_url: api_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimension,
        }
    }
}

#[async_trait]
impl EmbeddingService for OneApiEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let url = format!("{}/v1/embeddings", self.api_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "input": text
        });
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Embedding 请求失败: {}", e))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Embedding API 返回错误: {} - {}", status, body));
        }
        let result: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| format!("Embedding 响应解析失败: {}", e))?;
        result
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| "Embedding 响应中无数据".to_string())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

pub struct FallbackEmbeddingService {
    dimension: usize,
}

impl FallbackEmbeddingService {
    pub fn new() -> Self {
        Self {
            dimension: DEFAULT_VEC_SIZE,
        }
    }

    pub fn with_dimension(dimension: usize) -> Self {
        Self { dimension }
    }

    fn embed_fallback(&self, text: &str) -> Vec<f32> {
        let dim = self.dimension;
        let mut v = vec![0.0f32; dim];
        for word in text.split_whitespace() {
            v[(fnv_hash(word) % dim as u64) as usize] += 1.0;
        }
        let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag > 0.0 {
            for x in &mut v {
                *x /= mag;
            }
        }
        v
    }
}

impl Default for FallbackEmbeddingService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingService for FallbackEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(self.embed_fallback(text))
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

pub struct OllamaEmbeddingService {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimension: usize,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaEmbeddingService {
    pub fn new(base_url: &str, model: &str, dimension: usize) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimension,
        }
    }
}

#[async_trait]
impl EmbeddingService for OllamaEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let url = format!("{}/api/embed", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": text
        });
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama Embedding 请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama Embedding API 返回错误: {} - {}", status, body));
        }

        let result: OllamaEmbedResponse = resp
            .json()
            .await
            .map_err(|e| format!("Ollama Embedding 响应解析失败: {}", e))?;

        result
            .embeddings
            .into_iter()
            .next()
            .ok_or_else(|| "Ollama Embedding 响应中无 embeddings 数据".to_string())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

pub fn create_embedding_service_from_config(
    config: &crate::config::settings::EmbeddingSettings,
) -> Arc<dyn EmbeddingService> {
    if !config.enabled {
        info!("Embedding 已禁用，使用 Fallback 服务");
        return Arc::new(FallbackEmbeddingService::with_dimension(config.fallback.dimension));
    }

    match config.provider.as_str() {
        "ollama" => {
            info!(
                url = %config.ollama.base_url,
                model = %config.ollama.model,
                dim = config.ollama.dimension,
                "使用 Ollama Embedding 服务"
            );
            Arc::new(OllamaEmbeddingService::new(
                &config.ollama.base_url,
                &config.ollama.model,
                config.ollama.dimension,
            ))
        }
        "oneapi" => {
            if config.oneapi.base_url.is_empty() || config.oneapi.api_key.is_empty() {
                warn!("OneAPI Embedding 配置不完整，退化为 Fallback");
                return Arc::new(FallbackEmbeddingService::with_dimension(config.fallback.dimension));
            }
            info!(
                url = %config.oneapi.base_url,
                model = %config.oneapi.model,
                dim = config.oneapi.dimension,
                "使用 OneAPI Embedding 服务"
            );
            Arc::new(OneApiEmbeddingService::new(
                &config.oneapi.base_url,
                &config.oneapi.api_key,
                &config.oneapi.model,
                config.oneapi.dimension,
            ))
        }
        "fallback" | "" => {
            info!("使用 Fallback Embedding 服务");
            Arc::new(FallbackEmbeddingService::with_dimension(config.fallback.dimension))
        }
        other => {
            warn!(provider = other, "未知 Embedding provider，退化为 Fallback");
            Arc::new(FallbackEmbeddingService::with_dimension(config.fallback.dimension))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HybridSearchFilter {
    pub must_tags: Vec<String>,
    pub should_tags: Vec<String>,
    pub must_not_tags: Vec<String>,
    pub min_importance: Option<f32>,
    pub jsonld_types: Vec<String>,
    pub named_graph: Option<String>,
}

impl HybridSearchFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_must_tags(mut self, tags: Vec<String>) -> Self {
        self.must_tags = tags;
        self
    }

    pub fn with_should_tags(mut self, tags: Vec<String>) -> Self {
        self.should_tags = tags;
        self
    }

    pub fn with_must_not_tags(mut self, tags: Vec<String>) -> Self {
        self.must_not_tags = tags;
        self
    }

    pub fn with_min_importance(mut self, min: f32) -> Self {
        self.min_importance = Some(min);
        self
    }

    pub fn with_jsonld_types(mut self, types: Vec<String>) -> Self {
        self.jsonld_types = types;
        self
    }

    pub fn with_named_graph(mut self, graph: String) -> Self {
        self.named_graph = Some(graph);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.must_tags.is_empty()
            && self.should_tags.is_empty()
            && self.must_not_tags.is_empty()
            && self.min_importance.is_none()
            && self.jsonld_types.is_empty()
            && self.named_graph.is_none()
    }
}

pub struct VectorStore {
    client: Qdrant,
    embedding_service: Arc<dyn EmbeddingService>,
    fallback: FallbackEmbeddingService,
}

impl VectorStore {
    pub async fn new(url: &str, embedding_service: Option<Arc<dyn EmbeddingService>>) -> Result<Self, CoreError> {
        let embedding_service = embedding_service
            .unwrap_or_else(|| Arc::new(FallbackEmbeddingService::new()));
        let vec_size = embedding_service.dimension() as u64;
        let fallback = FallbackEmbeddingService::with_dimension(embedding_service.dimension());

        let grpc_url = if url.contains(":6333") {
            url.replace(":6333", ":6334")
        } else {
            url.to_string()
        };
        let client = Qdrant::new(
            qdrant_client::config::QdrantConfig::from_url(&grpc_url).skip_compatibility_check(),
        )
        .map_err(|e| CoreError::Internal { message: format!("Qdrant connect: {}", e) })?;
        info!(url = %url, "Qdrant connected");

        if !client
            .collection_exists(COLLECTION)
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant check: {}", e) })?
        {
            let params = VectorParams {
                size: vec_size,
                distance: Distance::Cosine.into(),
                ..Default::default()
            };
            client
                .create_collection(CreateCollectionBuilder::new(COLLECTION).vectors_config(params))
                .await
                .map_err(|e| CoreError::Internal { message: format!("Qdrant create: {}", e) })?;
            info!("Created Qdrant collection: {} (dim={})", COLLECTION, vec_size);
        }
        Ok(Self {
            client,
            embedding_service,
            fallback,
        })
    }

    async fn get_embedding(&self, text: &str) -> Vec<f32> {
        match self.embedding_service.embed(text).await {
            Ok(vec) => vec,
            Err(e) => {
                warn!(error = %e, "Embedding 服务失败，退化为关键词嵌入");
                self.fallback.embed_fallback(text)
            }
        }
    }

    pub async fn upsert(&self, iri: &str, text: &str, tags: &[String]) -> Result<(), CoreError> {
        self.upsert_with_metadata(iri, text, tags, None, None, None).await
    }

    pub async fn upsert_with_metadata(
        &self,
        iri: &str,
        text: &str,
        tags: &[String],
        importance: Option<f32>,
        jsonld_types: Option<&[String]>,
        named_graph: Option<&str>,
    ) -> Result<(), CoreError> {
        let vector = self.get_embedding(text).await;
        let mut payload: HashMap<String, Value> = HashMap::from([
            ("iri".into(), Value::String(iri.into())),
            ("text".into(), Value::String(text.chars().take(500).collect())),
            (
                "tags".into(),
                Value::Array(tags.iter().map(|t| Value::String(t.clone())).collect()),
            ),
        ]);

        if let Some(imp) = importance {
            payload.insert("importance".into(), Value::Number(serde_json::Number::from_f64(imp as f64).unwrap_or_else(|| serde_json::Number::from(0))));
        }

        if let Some(types) = jsonld_types {
            payload.insert(
                "jsonld_types".into(),
                Value::Array(types.iter().map(|t| Value::String(t.clone())).collect()),
            );
        }

        if let Some(graph) = named_graph {
            payload.insert("named_graph".into(), Value::String(graph.to_string()));
        }

        let point = PointStruct::new(iri.to_string(), vector, payload);
        let req = UpsertPointsBuilder::new(COLLECTION, vec![point]).wait(true);
        self.client
            .upsert_points(req)
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant upsert: {}", e) })?;
        debug!(iri = %iri, "Vector stored");
        Ok(())
    }

    pub async fn search(&self, query: &str, limit: u64) -> Result<Vec<ScoredEntry>, CoreError> {
        let vector = self.get_embedding(query).await;
        let results = self
            .client
            .search_points(SearchPointsBuilder::new(COLLECTION, vector, limit).with_payload(true))
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant search: {}", e) })?;
        Ok(results
            .result
            .into_iter()
            .map(|s| {
                let p = s.payload;
                let iri = extract_str(&p, "iri");
                let text = extract_str(&p, "text");
                let tags = extract_str_array(&p, "tags");
                let importance = extract_float(&p, "importance");
                let jsonld_types = extract_str_array(&p, "jsonld_types");
                ScoredEntry {
                    iri,
                    text,
                    score: s.score,
                    tags,
                    importance,
                    jsonld_types,
                }
            })
            .collect())
    }

    pub async fn search_with_filter(
        &self,
        query: &str,
        filter: &HybridSearchFilter,
        limit: u64,
    ) -> Result<Vec<ScoredEntry>, CoreError> {
        let vector = self.get_embedding(query).await;

        let qdrant_filter = self.build_qdrant_filter(filter);

        let mut builder = SearchPointsBuilder::new(COLLECTION, vector, limit).with_payload(true);

        if let Some(f) = qdrant_filter {
            builder = builder.filter(f);
        }

        let results = self
            .client
            .search_points(builder)
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant hybrid search: {}", e) })?;

        Ok(results
            .result
            .into_iter()
            .map(|s| {
                let p = s.payload;
                let iri = extract_str(&p, "iri");
                let text = extract_str(&p, "text");
                let tags = extract_str_array(&p, "tags");
                let importance = extract_float(&p, "importance");
                let jsonld_types = extract_str_array(&p, "jsonld_types");
                ScoredEntry {
                    iri,
                    text,
                    score: s.score,
                    tags,
                    importance,
                    jsonld_types,
                }
            })
            .collect())
    }

    pub async fn search_by_tags(
        &self,
        tags: &[String],
        limit: u64,
    ) -> Result<Vec<ScoredEntry>, CoreError> {
        let filter = HybridSearchFilter::new().with_must_tags(tags.to_vec());
        let dummy_vector = vec![0.0f32; self.embedding_service.dimension()];
        
        let qdrant_filter = self.build_qdrant_filter(&filter);
        
        let mut builder = SearchPointsBuilder::new(COLLECTION, dummy_vector, limit).with_payload(true);
        
        if let Some(f) = qdrant_filter {
            builder = builder.filter(f);
        }

        let results = self
            .client
            .search_points(builder)
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant tag search: {}", e) })?;

        Ok(results
            .result
            .into_iter()
            .map(|s| {
                let p = s.payload;
                let iri = extract_str(&p, "iri");
                let text = extract_str(&p, "text");
                let tags = extract_str_array(&p, "tags");
                let importance = extract_float(&p, "importance");
                let jsonld_types = extract_str_array(&p, "jsonld_types");
                ScoredEntry {
                    iri,
                    text,
                    score: s.score,
                    tags,
                    importance,
                    jsonld_types,
                }
            })
            .collect())
    }

    pub async fn hybrid_search(
        &self,
        query: &str,
        must_tags: &[String],
        should_tags: &[String],
        min_importance: Option<f32>,
        limit: u64,
    ) -> Result<Vec<ScoredEntry>, CoreError> {
        let mut filter = HybridSearchFilter::new()
            .with_must_tags(must_tags.to_vec())
            .with_should_tags(should_tags.to_vec());
        
        if let Some(min) = min_importance {
            filter = filter.with_min_importance(min);
        }

        self.search_with_filter(query, &filter, limit).await
    }

    fn build_qdrant_filter(&self, filter: &HybridSearchFilter) -> Option<Filter> {
        if filter.is_empty() {
            return None;
        }

        let mut must_conditions: Vec<Condition> = Vec::new();
        let mut should_conditions: Vec<Condition> = Vec::new();
        let mut must_not_conditions: Vec<Condition> = Vec::new();

        for tag in &filter.must_tags {
            must_conditions.push(Condition::matches("tags", tag.clone()));
        }

        for tag in &filter.should_tags {
            should_conditions.push(Condition::matches("tags", tag.clone()));
        }

        for tag in &filter.must_not_tags {
            must_not_conditions.push(Condition::matches("tags", tag.clone()));
        }

        for type_iri in &filter.jsonld_types {
            must_conditions.push(Condition::matches("jsonld_types", type_iri.clone()));
        }

        if let Some(ref graph) = filter.named_graph {
            must_conditions.push(Condition::matches("named_graph", graph.clone()));
        }

        if let Some(min_imp) = filter.min_importance {
            must_conditions.push(Condition::range(
                "importance",
                Range {
                    gte: Some(min_imp as f64),
                    ..Default::default()
                },
            ));
        }

        if must_conditions.is_empty() && should_conditions.is_empty() && must_not_conditions.is_empty() {
            return None;
        }

        let mut result_filter = Filter::default();

        if !must_conditions.is_empty() {
            result_filter.must = must_conditions;
        }

        if !should_conditions.is_empty() {
            result_filter.should = should_conditions;
        }

        if !must_not_conditions.is_empty() {
            result_filter.must_not = must_not_conditions;
        }

        Some(result_filter)
    }

    pub async fn delete(&self, iri: &str) -> Result<(), CoreError> {
        self.client
            .delete_points(DeletePointsBuilder::new(COLLECTION).points(vec![iri.to_string()]))
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant delete: {}", e) })?;
        Ok(())
    }

    pub async fn count(&self) -> Result<u64, CoreError> {
        let result = self
            .client
            .count(qdrant_client::qdrant::CountPointsBuilder::new(COLLECTION).exact(true))
            .await
            .map_err(|e| CoreError::Internal { message: format!("Qdrant count: {}", e) })?;
        Ok(result.result.unwrap_or_default().count)
    }
}

fn extract_str(map: &std::collections::HashMap<String, qdrant_client::qdrant::Value>, key: &str) -> String {
    map.get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn extract_str_array(map: &std::collections::HashMap<String, qdrant_client::qdrant::Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::ListValue(list)) => Some(
                list.values
                    .iter()
                    .filter_map(|item| match &item.kind {
                        Some(Kind::StringValue(s)) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn extract_float(map: &std::collections::HashMap<String, qdrant_client::qdrant::Value>, key: &str) -> Option<f32> {
    map.get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::DoubleValue(d)) => Some(*d as f32),
            Some(Kind::IntegerValue(i)) => Some(*i as f32),
            _ => None,
        })
}

fn fnv_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[derive(Debug, Clone)]
pub struct ScoredEntry {
    pub iri: String,
    pub text: String,
    pub score: f32,
    pub tags: Vec<String>,
    pub importance: Option<f32>,
    pub jsonld_types: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_embed() {
        let svc = FallbackEmbeddingService::new();
        let v = svc.embed_fallback("hello world");
        assert_eq!(v.len(), DEFAULT_VEC_SIZE);
        assert!(v.iter().any(|x| *x > 0.0));
    }

    #[test]
    fn test_fallback_embed_custom_dimension() {
        let svc = FallbackEmbeddingService::with_dimension(256);
        let v = svc.embed_fallback("hello world");
        assert_eq!(v.len(), 256);
        assert!(v.iter().any(|x| *x > 0.0));
    }

    #[tokio::test]
    async fn test_fallback_embedding_service_trait() {
        let svc = FallbackEmbeddingService::new();
        let v = svc.embed("hello world").await.unwrap();
        assert_eq!(v.len(), DEFAULT_VEC_SIZE);
        assert!(v.iter().any(|x| *x > 0.0));
    }

    #[test]
    fn test_fnv() {
        assert_eq!(fnv_hash("a"), fnv_hash("a"));
        assert_ne!(fnv_hash("a"), fnv_hash("b"));
    }

    #[test]
    fn test_hybrid_search_filter_builder() {
        let filter = HybridSearchFilter::new()
            .with_must_tags(vec!["rust".to_string(), "async".to_string()])
            .with_should_tags(vec!["tokio".to_string()])
            .with_min_importance(0.5)
            .with_jsonld_types(vec!["Code".to_string()]);

        assert_eq!(filter.must_tags.len(), 2);
        assert_eq!(filter.should_tags.len(), 1);
        assert_eq!(filter.min_importance, Some(0.5));
        assert_eq!(filter.jsonld_types.len(), 1);
        assert!(!filter.is_empty());
    }

    #[test]
    fn test_empty_filter() {
        let filter = HybridSearchFilter::new();
        assert!(filter.is_empty());
    }
}
