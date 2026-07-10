pub mod skill_registry;
pub mod skill_pipeline;
pub mod prompt_registry;
pub mod mcp_client;
pub mod mcp;
pub mod tool_executor;
pub mod tool_groups;
pub mod builtin;
pub mod hooks;
pub mod sharing;
pub mod sharing_audit;

pub mod workspace_monitor;
pub mod result_router;
pub mod tool_guard;
pub mod import_scanner;

pub use skill_registry::SkillRegistry;
pub use prompt_registry::PromptRegistry;
pub use tool_executor::ToolExecutor;
pub use tool_groups::{ToolGroup, ToolGroupManager, ToolGroupSettings, RoleToolConfig};
pub use hooks::{HookManager, HookPoint, HookResult, HookContext, Hook};
pub use sharing::{
    SharingProtocol, SharedReference, ShareRequest, ShareResponse,
    ShareType, Permission, ContextInjector,
};
pub use mcp::{
    MCPMessage, MCPTool, MCPResource, MCPPrompt, MCPError,
    MCPToolRegistry, MCPServer, MCPClient, ToolHandler,
    create_default_mcp_server,
};
