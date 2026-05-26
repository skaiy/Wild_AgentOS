pub mod skill_registry;
pub mod mcp_client;
pub mod mcp;
pub mod tool_executor;
pub mod builtin;
pub mod hooks;
pub mod sharing;
pub mod knowledge_graph;
pub mod result_router;

pub use skill_registry::SkillRegistry;
pub use tool_executor::ToolExecutor;
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
