//! JSON-LD 模块 - 实现 JSON-LD 上下文、类型和工具函数
//!
//! 提供全局统一的 @context 定义、JSON-LD 节点类型和 IRI 处理工具

pub mod context;
pub mod framing;
pub mod registry;
pub mod type_router;
pub mod types;
pub mod utils;

pub use context::JsonLdContext;
pub use framing::{
    apply_frame, embed_node, estimate_tokens, filter_properties, fit_to_budget,
    EmbedDirective, FrameTemplate, AA_DECISION_FRAME, CA_REVIEW_FRAME, DA_INPUT_FRAME,
    PLAN_CONTEXT_FRAME, SUMMARY_ONLY_FRAME,
};
pub use type_router::{TypeRoute, TypeRouter};
pub use types::{JsonLdKeyword, JsonLdNode};
pub use utils::{generate_iri, is_iri_reference, parse_iri, validate_jsonld_node};
