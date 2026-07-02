//! Graph Neural Network — Node feature extraction + neighborhood aggregation.
//!
//! Implements a lightweight **graph feature engine** for the skill graph:
//! - `FeatureExtractor`: converts `SkillGraphNode` + graph topology → numeric feature vectors
//! - `NeighborhoodAggregator`: mean-pooling of neighbor features (simplified GraphSAGE)
//! - `SimilarityEngine`: link prediction via cosine similarity on embeddings
//!
//! # Design Philosophy
//!
//! Instead of a full GCN with learned weights (which requires SGD/backprop),
//! this module uses **geometric embedding alignment**:
//! 1. Extract structural features using existing `SkillGraphEmbedder` (Poincaré coords)
//! 2. Augment with graph metrics from `SkillGraphAlgorithms` (PageRank, centrality, degree)
//! 3. Run iterative neighborhood aggregation (mean-pool) for K rounds
//! 4. Result: learned-style embeddings without a training loop
//!
//! For true GNN training, the `export_for_training()` method exports the graph
//! to JSON for offline Python training. The trained weights can be loaded back
//! via `load_trained_weights()`.

pub mod features;
pub mod similarity;

pub use features::{FeatureExtractor, NeighborhoodAggregator, NodeFeatures};
pub use similarity::{LinkPrediction, SimilarityEngine};
