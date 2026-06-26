//! Bridge between HyperspaceEngine and ontology systems for dual-space embedding storage & search.
//!
//! Moved from `crates/hyperspace-engine/src/open_ontologies.rs` — this is a cross-cutting
//! concern that bridges two independent subsystems (vector engine + ontology), so it belongs
//! in the application crate rather than inside either subsystem crate.
//!
//! # Design
//!
//! Open ontologies store dual-space embeddings (text in Cosine space, structural in Poincaré
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

use hyperspace_engine::engine::{HyperspaceEngine, SearchHit};
use hyperspace_engine::error::EngineError;
use hyperspace_engine::hyper_vector::EmbeddingVector;

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

    /// Cross-search: given an IRI in this store, retrieve the structural vector
    /// and search the text space with it (cross-space retrieval).
    async fn cross_search(
        &self,
        source_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError>;

    /// Retrieve the text-space (Cosine) embedding vector for a given IRI.
    async fn get_text_vector(&self, iri: &str) -> Result<Option<EmbeddingVector>, EngineError>;

    /// Retrieve the structural-space (Poincaré) embedding vector for a given IRI.
    async fn get_struct_vector(&self, iri: &str) -> Result<Option<EmbeddingVector>, EngineError>;

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

    /// Expose the text engine for advanced use (OntologySearchBridge needs cross-store routing).
    pub fn text_engine(&self) -> &Arc<dyn HyperspaceEngine> {
        &self.text_engine
    }

    /// Expose the struct engine for advanced use.
    pub fn struct_engine(&self) -> &Arc<dyn HyperspaceEngine> {
        &self.struct_engine
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
                None => {}
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

    /// Cross-search: get the STRUCTURAL vector for `source_iri` and use it to
    /// search the TEXT space. This finds text entries whose structural semantics
    /// are similar to the source IRI's structural embedding.
    ///
    /// If the source has no structural vector, falls back to getting the TEXT
    /// vector and searching the STRUCTURAL space.
    async fn cross_search(
        &self,
        source_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        // Primary: get struct vector → search text space
        if let Some(vec) = self.struct_engine.get_vector(source_iri).await? {
            return self.text_engine.search(&vec, top_k, &[]).await;
        }
        // Fallback: get text vector → search struct space
        if let Some(vec) = self.text_engine.get_vector(source_iri).await? {
            return self.struct_engine.search(&vec, top_k, &[]).await;
        }
        Ok(Vec::new())
    }

    async fn get_text_vector(&self, iri: &str) -> Result<Option<EmbeddingVector>, EngineError> {
        self.text_engine.get_vector(iri).await
    }

    async fn get_struct_vector(&self, iri: &str) -> Result<Option<EmbeddingVector>, EngineError> {
        self.struct_engine.get_vector(iri).await
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
///
/// Unlike `OntologyEmbedStore::cross_search` (which searches across spaces WITHIN
/// one store), this bridge searches ACROSS stores: agent memory IRI → ontology space.
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
    ///
    /// Algorithm:
    /// 1. Get the structural (Poincaré) vector for `agent_iri` from the agent store
    /// 2. Search the ontology store's structural space with that vector
    ///
    /// This finds ontology classes that have similar STRUCTURAL roles to the agent memory.
    pub async fn find_related_ontologies(
        &self,
        agent_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        // Agent memory's structural vector lives in the agent store
        let vec = self.agent_store.get_struct_vector(agent_iri).await?;
        match vec {
            Some(v) => self.ontology_store.search_structural(&v, top_k).await,
            None => Ok(Vec::new()),
        }
    }

    /// Find agent memories whose structural embedding is close to an ontology class IRI.
    ///
    /// Algorithm:
    /// 1. Get the structural (Poincaré) vector for `ontology_iri` from the ontology store
    /// 2. Search the agent store's structural space with that vector
    ///
    /// This finds agent memories that have similar STRUCTURAL roles to the ontology class.
    pub async fn find_related_memories(
        &self,
        ontology_iri: &str,
        top_k: usize,
    ) -> Result<Vec<SearchHit>, EngineError> {
        // Ontology class's structural vector lives in the ontology store
        let vec = self.ontology_store.get_struct_vector(ontology_iri).await?;
        match vec {
            Some(v) => self.agent_store.search_structural(&v, top_k).await,
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hyperspace_engine::engine::HyperspaceEngineImpl;
    use hyperspace_engine::hnsw::HnswConfig;
    use hyperspace_engine::hyper_vector::{EmbeddingVector, MetricKind};
    use hyperspace_engine::metric::CosineMetric;
    use hyperspace_engine::metric::PoincareMetric;
    use hyperspace_engine::wal::WalSyncMode;
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

        // Insert into agent memory (simulates agent remembering a user).
        // Both text AND structural vectors are stored in the agent store.
        agent_store
            .store_embedding(
                "mem:user_123",
                Some(&v_cos(vec![1.0, 0.0, 0.0, 0.0])),
                Some(&v_poin(vec![0.3, 0.1, 0.0, 0.0])),
                &json!({"@type": ["Memory"], "label": "Alice"}),
            )
            .await
            .unwrap();

        // Insert into ontology engine (ontology class Person with similar struct vector)
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

        // Find ontologies related to the agent memory.
        // The fix: find_related_ontologies now correctly retrieves the struct vector
        // from the AGENT store and searches the ONTOLOGY store's struct space.
        let results = bridge
            .find_related_ontologies("mem:user_123", 5)
            .await
            .unwrap();

        // "Person" (struct vec [0.25, 0.05, ...]) is closer to "mem:user_123"
        // (struct vec [0.3, 0.1, ...]) than "Organization" (struct vec [0.0, 0.3, ...]).
        // Both should be returned since they're both structurally close enough.
        assert_eq!(results.len(), 2, "Should find both Person and Organization as related ontologies");
        assert_eq!(results[0].iri, "onto:Person", "Person should be the most related");
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

    #[tokio::test]
    async fn test_cross_search_with_both_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let store = HyperspaceEmbedStore::new(
            Arc::new(text_engine(dir.path())),
            Arc::new(struct_engine(dir.path())),
        );

        // Insert with BOTH text and struct vectors
        store
            .store_embedding(
                "onto:Dual",
                Some(&v_cos(vec![1.0, 0.0, 0.0, 0.0])),
                Some(&v_poin(vec![0.2, 0.1, 0.0, 0.0])),
                &json!({"label": "dual space entry"}),
            )
            .await
            .unwrap();

        // cross_search should find entries in the STRUCTURAL space similar to
        // the TEXT vector — but since both spaces have the same entries,
        // this is primarily testing the routing works (non-panicking, returns results)
        let results = store
            .cross_search("onto:Dual", 5)
            .await
            .unwrap();
        assert_eq!(results.len(), 1, "cross_search should return at least the source entry");
        assert_eq!(results[0].iri, "onto:Dual");
    }

    #[tokio::test]
    async fn test_get_text_and_struct_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let store = HyperspaceEmbedStore::new(
            Arc::new(text_engine(dir.path())),
            Arc::new(struct_engine(dir.path())),
        );

        store
            .store_embedding(
                "onto:TestVec",
                Some(&v_cos(vec![0.1, 0.2, 0.3, 0.4])),
                Some(&v_poin(vec![0.1, 0.05, 0.0, 0.0])),
                &json!({"label": "vector test"}),
            )
            .await
            .unwrap();

        let text_vec = store.get_text_vector("onto:TestVec").await.unwrap();
        assert!(text_vec.is_some(), "text vector should exist");
        assert_eq!(text_vec.unwrap().metric, MetricKind::Cosine);

        let struct_vec = store.get_struct_vector("onto:TestVec").await.unwrap();
        assert!(struct_vec.is_some(), "struct vector should exist");
        assert_eq!(struct_vec.unwrap().metric, MetricKind::Poincare);

        // Non-existent IRI returns None
        let missing = store.get_text_vector("onto:Nobody").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_bridge_no_match_returns_empty() {
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

        // Agent has an entry but ontology store is empty
        agent_store
            .store_embedding(
                "mem:lonely",
                Some(&v_cos(vec![1.0, 0.0, 0.0, 0.0])),
                Some(&v_poin(vec![0.1, 0.1, 0.0, 0.0])),
                &json!({"label": "lonely memory"}),
            )
            .await
            .unwrap();

        let bridge = OntologySearchBridge::new(agent_store, ontology_store);
        let results = bridge
            .find_related_ontologies("mem:lonely", 5)
            .await
            .unwrap();
        assert!(results.is_empty(), "Empty ontology store should return no matches");
    }
}
