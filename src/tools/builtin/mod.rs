pub mod permissions;
pub mod hooks;
pub mod rag;
pub mod knowledge;
pub mod bash;
pub mod file_ops;
pub mod mcp;
pub mod sandbox;

#[cfg(feature = "ontology")]
pub mod ontology_tools;

pub use permissions::{
    PermissionMode,
    PermissionContext,
    PermissionRequest,
    PermissionPromptDecision,
    PermissionPrompter,
    PermissionOutcome,
    PermissionPolicy,
    PermissionOverride,
};

pub use hooks::{
    HookEvent,
    HookProgressEvent,
    HookProgressReporter,
    HookAbortSignal,
    HookRunResult,
    HookRunner,
    HookPermissionDecision,
};

pub use knowledge::{
    execute_knowledge_import_file,
    execute_knowledge_import_url,
    execute_knowledge_import_directory,
    execute_knowledge_list,
    execute_knowledge_delete,
    execute_knowledge_search,
    execute_knowledge_update,
};

#[cfg(feature = "ontology")]
pub use ontology_tools::{
    execute_ontology_validate_turtle,
    execute_ontology_lint_turtle,
    execute_ontology_diff_turtle,
    execute_ontology_validate_shacl,
};
