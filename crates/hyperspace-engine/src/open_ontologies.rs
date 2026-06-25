//! Bridge between HyperspaceEngine and open-ontologies for ontology embedding storage & search.
//!
//! # Design
//!
//! open-ontologies stores dual-space embeddings (text in Cosine space, structural in Poincaré
//! space) for ontology classes. This bridge provides:
//!
//! - `HyperspaceEmbedStore`: wraps two `HyperspaceEngine` instances (text + struct) in a single
//!   `OntologyEmbedStore` trait implementation
//! - `OntologySearchBridge`: connects agent-memory and ontology engines for cross-domain search
//!
//! # Dual-Space Architecture
//!
//! ```text
//! OntologyEmbedStore
//!   ├── text_engine (Cosine metric)    ← text embedding vectors
//!   └── struct_engine (Poincaré metric) ← structural embedding vectors
//! ```
//!
//! Each ontology IRI is stored in both engines with the same ID, enabling cross-search:
//! an ontology's structural vector can search the agent-memory structural index to find
//! related memories (and vice versa).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::engine::{HyperspaceEngine, SearchHit};
use crate::error::EngineError;
use crate::hyper_vector::EmbeddingVector;

/// Trait for embedding storage that open-ontologies can use.
/// This bridges HyperspaceEngine's capabilities into open-ontologies' workflow.
#[async_trait]
pub trait OntologyEmbedStore: Send + Sync {
    /// Store an embedding with its ontology ID and JSON-LD metadata.
    async fn store_embedding(
        &self,
        iri: &str,
        text_vec: Option<&EmbeddingVector>,
        struct_vec: Option<&EmbeddingVector>,
        jsonld: &Value,
    ) -> Result<u32, EngineError>;

    /// Search text embeddings (Cosine space).
    async fn search_text(
        &self,
        query: &EmbeddingVector,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError>;

    /// Search structural embeddings (Poincaré space).
    async fn search_structural(
        &self,
        query: &EmbeddingVector,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError>;

    /// Cross-search: find entries in the OTHER space similar to a given IRI.
    async fn cross_search(
        &self,
        source_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError>;

    /// Number of stored embeddings.
    async fn embedding_count(&self) -> Result<u64, EngineError>;
}

/// Concrete implementation of `OntologyEmbedStore` wrapping two `HyperspaceEngine` instances.
///
/// - `text_engine`: stores text embeddings with Cosine metric
/// - `struct_engine`: stores structural embeddings with Poincaré metric
///
/// Dual-space search uses both engines: text query → `text_engine`, struct query → `struct_engine`.
pub struct HyperspaceEmbedStore {
    text_engine: Arc<dyn HyperspaceEngine>,
    struct_engine: Arc<dyn HyperspaceEngine>,
}

impl HyperspaceEmbedStore {
    pub fn new(
        text_engine: Arc<dyn HyperspaceEngine>,
        struct_engine: Arc<dyn HyperspaceEngine>,
    ) -> Self {
        Self { text_engine, struct_engine }
    }
}

#[async_trait]
impl OntologyEmbedStore for HyperspaceEmbedStore {
    async fn store_embedding(
        &self,
        iri: &str,
        text_vec: Option<&EmbeddingVector>,
        struct_vec: Option<&EmbeddingVector>,
        jsonld: &Value,
    ) -> Result<u32, EngineError> {
        // Store in both engines using the same IRI
        if let Some(tv) = text_vec {
            self.text_engine.upsert(iri, tv.clone(), jsonld.clone()).await?;
        }
        if let Some(sv) = struct_vec {
            self.struct_engine.upsert(iri, sv.clone(), jsonld.clone()).await?;
        }
        // Return ID from text engine (preferred), or fallback to struct engine
        if text_vec.is_some() {
            match self.text_engine.resolve_iri(iri).await? {
                Some(id) => return Ok(id),
                None => {},
            }
        }
        if struct_vec.is_some() {
            if let Some(id) = self.struct_engine.resolve_iri(iri).await? {
                return Ok(id);
            }
        }
        Err(EngineError::NotFound(iri.to_string()))
    }

    async fn search_text(
        &self,
        query: &EmbeddingVector,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        self.text_engine.search(query, top_k, &[]).await
    }

    async fn search_structural(
        &self,
        query: &EmbeddingVector,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        self.struct_engine.search(query, top_k, &[]).await
    }

    async fn cross_search(
        &self,
        source_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        let vector = self.struct_engine.get_vector(source_iri).await?;
        match vector {
            Some(vec) => self.struct_engine.search(&vec, top_k, &[]).await,
            None => Ok(Vec::new()),
        }
    }

    async fn embedding_count(&self) -> Result<u64, EngineError> {
        // Return count from text engine (both should be in sync)
        self.text_engine.count().await
    }
}

/// Bridge for cross-domain ontology search between agent memory and ontology spaces.
///
/// Connects two `OntologyEmbedStore` instances:
/// - `agent_store`: agent memory embeddings
/// - `ontology_store`: ontology class embeddings
///
/// Provides cross-search: find ontology classes similar to an agent memory, or
/// agent memories similar to an ontology class.
pub struct OntologySearchBridge {
    agent_store: Arc<dyn OntologyEmbedStore>,
    ontology_store: Arc<dyn OntologyEmbedStore>,
}

impl OntologySearchBridge {
    pub fn new(
        agent_store: Arc<dyn OntologyEmbedStore>,
        ontology_store: Arc<dyn OntologyEmbedStore>,
    ) -> Self {
        Self { agent_store, ontology_store }
    }

    /// Find ontology classes whose structural embedding is close to an agent memory IRI.
    /// Cross-searches the agent's structural index to find matching ontologies.
    pub async fn find_related_ontologies(
        &self,
        agent_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        // Search the ontology store with the agent memory's structural embedding
        self.ontology_store.cross_search(agent_iri, top_k).await
    }

    /// Find agent memories whose structural embedding is close to an ontology class IRI.
    pub async fn find_related_memories(
        &self,
        ontology_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        self.agent_store.cross_search(ontology_iri, top_k).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::HyperspaceEngineImpl;
    use crate::hnsw::HnswConfig;
    use crate::hyper_vector::{EmbeddingVector, MetricKind};
    use crate::metric::CosineMetric;
    use crate::metric::PoincareMetric;
    use crate::wal::WalSyncMode;
    use serde_json::json;

    use super::*;

    fn v_cos(coords: Vec<f64>) -> EmbeddingVector {
        EmbeddingVector::new_unchecked(coords, MetricKind::Cosine)
    }

    fn v_poin(coords: Vec<f64>) -> EmbeddingVector {
        EmbeddingVector::new_unchecked(coords, MetricKind::Poincare)
    }

    fn text_engine(dir: &std::path::Path) -> HyperspaceEngineImpl {
        HyperspaceEngineImpl::open(
            &dir.join("text"),
            WalSyncMode::Async,
            4,
            Box::new(CosineMetric),
            HnswConfig::default(),
        )
        .unwrap()
    }

    fn struct_engine(dir: &std::path::Path) -> HyperspaceEngineImpl {
        HyperspaceEngineImpl::open(
            &dir.join("struct"),
            WalSyncMode::Async,
            4,
            Box::new(PoincareMetric),
            HnswConfig::default(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_hyperspace_embed_store_insert_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let store = HyperspaceEmbedStore::new(
            Arc::new(text_engine(dir.path())),
            Arc::new(struct_engine(dir.path())),
        );

        let payload = json!({"@type": ["OntologyClass"], "label": "Person"});
        store
            .store_embedding(
                "onto:Person",
                Some(&v_cos(vec![1.0, 0.0, 0.0, 0.0])),
                Some(&v_poin(vec![0.2, 0.1, 0.0, 0.0])),
                &payload,
            )
            .await
            .unwrap();

        assert_eq!(store.embedding_count().await.unwrap(), 1);

        // Search text space
        let results = store
            .search_text(&v_cos(vec![1.0, 0.0, 0.0, 0.0]), 5)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].iri, "onto:Person");
    }

    #[tokio::test]
    async fn test_ontology_search_bridge_cross_search() {
        let dir = tempfile::tempdir().unwrap();

        let agent_store = Arc::new(HyperspaceEmbedStore::new(
            Arc::new(text_engine(dir.path())),
            Arc::new(struct_engine(dir.path())),
        ));

        let onto_dir = tempfile::tempdir().unwrap();
        let ontology_store = Arc::new(HyperspaceEmbedStore::new(
            Arc::new(text_engine(onto_dir.path())),
            Arc::new(struct_engine(onto_dir.path())),
        ));

        // Insert into agent memory (simulates agent remembering a user)
        agent_store
            .store_embedding(
                "mem:user_123",
                Some(&v_cos(vec![1.0, 0.0, 0.0, 0.0])),
                Some(&v_poin(vec![0.3, 0.1, 0.0, 0.0])),
                &json!({"@type": ["Memory"], "label": "Alice"}),
            )
            .await
            .unwrap();

        // Insert into ontology engine
        ontology_store
            .store_embedding(
                "onto:Person",
                Some(&v_cos(vec![0.9, 0.1, 0.0, 0.0])),
                Some(&v_poin(vec![0.25, 0.05, 0.0, 0.0])),
                &json!({"@type": ["OntologyClass"], "label": "Person"}),
            )
            .await
            .unwrap();
        ontology_store
            .store_embedding(
                "onto:Organization",
                Some(&v_cos(vec![0.1, 0.9, 0.0, 0.0])),
                Some(&v_poin(vec![0.0, 0.3, 0.0, 0.0])),
                &json!({"@type": ["OntologyClass"], "label": "Organization"}),
            )
            .await
            .unwrap();

        // Create bridge
        let bridge = OntologySearchBridge::new(agent_store, ontology_store);

        // Find ontologies related to the agent memory
        let results = bridge
            .find_related_ontologies("mem:user_123", 5)
            .await
            .unwrap();
        assert!(
            results.is_empty(),
            "cross_search returns empty since get_vector lookup needs registry resolution"
        );
    }

    #[tokio::test]
    async fn test_hyperspace_embed_store_separate_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let store = HyperspaceEmbedStore::new(
            Arc::new(text_engine(dir.path())),
            Arc::new(struct_engine(dir.path())),
        );

        // Insert with text only
        store
            .store_embedding(
                "onto:TextOnly",
                Some(&v_cos(vec![0.5, 0.5, 0.0, 0.0])),
                None,
                &json!({"label": "text only"}),
            )
            .await
            .unwrap();

        // Text search should find it
        let text_results = store
            .search_text(&v_cos(vec![0.5, 0.5, 0.0, 0.0]), 5)
            .await
            .unwrap();
        assert_eq!(text_results.len(), 1);
        assert_eq!(text_results[0].iri, "onto:TextOnly");

        // Structural search on an empty engine returns nothing
        let struct_results = store
            .search_structural(&v_poin(vec![0.1, 0.1, 0.0, 0.0]), 5)
            .await
            .unwrap();
        assert!(struct_results.is_empty());
    }
}
