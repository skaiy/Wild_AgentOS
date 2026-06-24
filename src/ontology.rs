//! GH + OO ontology fusion module.
//!
//! Bridges Gliding Horse's embedded ontology store (`knowledge_graph`) with
//! Open Ontologies' full RDF/OWL pipeline (SHACL, reason, align, drift,
//! lifecycle). Gated behind the `ontology` Cargo feature.
//!
//! ## Design
//!
//! ```text
//! GH subsystems                 OO pipeline
//! ────────────                 ───────────
//! knowledge_graph/store  ──►  SharedGraphStore
//!   (existing Oxigraph)       (Arc<Store>, no Mutex)
//!                              │
//!                              ├─ create temp OO GraphStore via to_graph_store()
//!                              ├─ ShaclValidator / Reasoner / OntologyService
//!                              └─ return JSON reports
//! ```

use std::sync::Arc;

// ─── Re-exports ────────────────────────────────────────────

pub use open_ontologies::graph::SharedGraphStore;
pub use open_ontologies::graph::SparqlAuth as OoSparqlAuth;
pub use open_ontologies::graph::GraphStore as OoGraphStore;

/// Build a `SharedGraphStore` that shares nothing with existing GH stores.
pub fn new_shared_store() -> anyhow::Result<SharedGraphStore> {
    SharedGraphStore::new()
}

/// Build a `SharedGraphStore` wrapping the same `Arc<Store>` as an existing
/// `KnowledgeGraphStore` so both layers share triple data.
pub fn from_kg_store(store: &Arc<oxigraph::store::Store>) -> SharedGraphStore {
    SharedGraphStore::from_arc(Arc::clone(store))
}

// ─── OntologyPipeline trait ────────────────────────────────

/// Trait that extends `SharedGraphStore` with OO pipeline capabilities.
pub trait OntologyPipeline: Send + Sync {
    fn validate_turtle(&self, ttl: &str) -> anyhow::Result<usize>;
    fn validate_shacl(&self, shapes_ttl: &str) -> anyhow::Result<String>;
    fn check_shacl(&self, shapes_ttl: &str) -> anyhow::Result<String>;
    fn reason(&self, profile: &str, materialize: bool) -> anyhow::Result<String>;
    fn lint(&self) -> anyhow::Result<String>;
}

impl OntologyPipeline for SharedGraphStore {
    fn validate_turtle(&self, ttl: &str) -> anyhow::Result<usize> {
        OoGraphStore::validate_turtle(ttl)
    }

    fn validate_shacl(&self, shapes_ttl: &str) -> anyhow::Result<String> {
        let oo_graph = Arc::new(self.to_graph_store()?);
        open_ontologies::shacl::ShaclValidator::validate(&oo_graph, shapes_ttl)
    }

    fn check_shacl(&self, shapes_ttl: &str) -> anyhow::Result<String> {
        let oo_graph = Arc::new(self.to_graph_store()?);
        open_ontologies::shacl::ShaclValidator::check_shapes(&oo_graph, shapes_ttl)
    }

    fn reason(&self, profile: &str, materialize: bool) -> anyhow::Result<String> {
        let oo_graph = Arc::new(self.to_graph_store()?);
        open_ontologies::reason::Reasoner::run(&oo_graph, profile, materialize)
    }

    fn lint(&self) -> anyhow::Result<String> {
        let ttl = self.serialize("turtle")?;
        open_ontologies::ontology::OntologyService::lint(&ttl)
    }
}

// ─── Free functions (operate on Turtle strings, no store needed) ──

/// Validate Turtle syntax without loading.
pub fn validate_turtle_content(content: &str) -> anyhow::Result<String> {
    open_ontologies::ontology::OntologyService::validate_string(content)
}

/// Lint Turtle content (checks labels, comments, domains).
pub fn lint_turtle_content(content: &str) -> anyhow::Result<String> {
    open_ontologies::ontology::OntologyService::lint(content)
}

/// Diff two Turtle documents.
pub fn diff_turtle(old: &str, new: &str) -> anyhow::Result<String> {
    open_ontologies::ontology::OntologyService::diff(old, new)
}
