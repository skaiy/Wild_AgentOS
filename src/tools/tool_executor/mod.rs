use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

use crate::tools::builtin::hooks::HookRunner;
use crate::tools::builtin::permissions::{PermissionMode, PermissionOutcome, PermissionPolicy};
use crate::tools::builtin::rag;
use crate::tools::builtin::knowledge;
use crate::knowledge_graph::code_ast::CodeAstExtractor;
use crate::knowledge_graph::extractor::KnowledgeExtractor;
use crate::knowledge_graph::ontology::OntologyManager;
use crate::knowledge_graph::rdf_mapper::RdfMapper;
use crate::knowledge_graph::store::KnowledgeGraphStore;
use crate::knowledge_graph::types::{BridgeRelationType, NodeDef, EdgeDef, RdfQuad, RdfValue};
use crate::tools::tool_groups::ToolGroupManager;
use crate::tools::workspace_monitor::{WorkspaceMonitor, FileState};

mod builtins;

#[cfg(test)]
mod tests;

pub(crate) use self::builtins::init_skill_creator_gateway;

/// Tool input structs
#[derive(Debug, Deserialize)]
pub struct GlobSearchInput {
    pub pattern: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub output_mode: Option<String>,
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub context: Option<usize>,
    pub line_numbers: Option<bool>,
    pub head_limit: Option<usize>,
    pub offset: Option<usize>,
    #[serde(rename = "-i")]
    pub case_insensitive: Option<bool>,
    pub multiline: Option<bool>,
    pub file_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebFetchInput {
    pub url: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebSearchInput {
    pub query: String,
    pub allowed_domains: Option<Vec<String>>,
    pub blocked_domains: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ToolSearchInput {
    pub query: String,
    pub max_results: Option<usize>,
}

type ToolFn = Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> + Send + Sync>;

/// 将同步工具函数包装为异步 ToolFn
fn sync_tool<F>(f: F) -> ToolFn
where
    F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Arc::new(move |input| {
        let f = Arc::clone(&f);
        Box::pin(async move { f(input) })
    })
}

/// 将同步工具函数（取 &Value）包装为异步 ToolFn
fn sync_tool_ref<F>(f: F) -> ToolFn
where
    F: Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Arc::new(move |input| {
        let f = Arc::clone(&f);
        Box::pin(async move { f(&input) })
    })
}

/// 微工具上下文
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MicroToolContext {
    pub call_id: String,
    pub storage_key: String,
    pub tool_name: String,
    pub entity_types: Vec<String>,
    pub preview_size: usize,
}

/// Unified tool executor with built-in tools
#[derive(Clone)]
pub struct ToolExecutor {
    tools: HashMap<String, ToolFn>,
    tool_descriptions: Vec<ToolDescription>,
    kg_store: Arc<RwLock<KnowledgeGraphStore>>,
    projection_engine: Arc<std::sync::RwLock<Option<Arc<crate::memory::l3_projection::ProjectionEngine>>>>,
    micro_tool_contexts: Arc<std::sync::RwLock<HashMap<String, MicroToolContext>>>,
    micro_tool_data: Arc<std::sync::RwLock<HashMap<String, serde_json::Value>>>,
    syscall_gate: Option<crate::core::syscall_gate::SyscallGate>,
    permission_policy: Option<PermissionPolicy>,
    hook_runner: Option<HookRunner>,
    tool_group_manager: Option<ToolGroupManager>,
    workspace_monitor: Arc<std::sync::RwLock<Option<Arc<WorkspaceMonitor>>>>,
}

// 微工具描述数量上限，超过时移除最早注册的条目
// 避免 tool_descriptions 无限膨胀导致每次 LLM 请求携带数千 token 的工具列表
const MAX_MICRO_TOOL_DESCRIPTIONS: usize = 5;
const MICRO_TOOL_PREFIXES: &[&str] = &[
    "read_full_result_",
    "query_",
    "get_entity_details",
    "expand_relation",
];

/// 工具适用角色: ""=全部, "PA"/"DA"/"CA"/"AA"=仅该角色
#[derive(Clone)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub allowed_roles: Vec<String>,  // 空 = 所有角色可用
}

impl ToolExecutor {
    pub fn new() -> Self {
        let kg_store = Arc::new(RwLock::new(
            KnowledgeGraphStore::new().expect("创建知识图谱存储失败")
        ));
        let mut exe = Self {
            tools: HashMap::new(),
            tool_descriptions: Vec::new(),
            kg_store,
            projection_engine: Arc::new(std::sync::RwLock::new(None)),
            micro_tool_contexts: Arc::new(std::sync::RwLock::new(HashMap::new())),
            micro_tool_data: Arc::new(std::sync::RwLock::new(HashMap::new())),
            syscall_gate: None,
            permission_policy: None,
            hook_runner: None,
            tool_group_manager: None,
            workspace_monitor: Arc::new(std::sync::RwLock::new(None)),
        };
        exe.register_builtins();
        exe
    }
    
    pub fn set_projection_engine(&mut self, engine: Arc<crate::memory::l3_projection::ProjectionEngine>) {
        if let Ok(mut pe) = self.projection_engine.write() {
            *pe = Some(engine);
        }
    }
    
    pub fn set_tool_group_manager(&mut self, manager: ToolGroupManager) {
        self.tool_group_manager = Some(manager);
    }

    /// 使用统一 Oxigraph Store 替换内部的 KnowledgeGraphStore
    pub fn set_unified_kg_store(&mut self, store: Arc<oxigraph::store::Store>) {
        self.kg_store = Arc::new(RwLock::new(
            KnowledgeGraphStore::with_shared_store(store).expect("创建共享 KG Store 失败")
        ));
    }

    pub fn set_syscall_gate(&mut self, gate: crate::core::syscall_gate::SyscallGate) {
        self.syscall_gate = Some(gate);
    }

    pub fn set_permission_policy(&mut self, policy: PermissionPolicy) {
        self.permission_policy = Some(policy);
    }

    pub fn set_hook_runner(&mut self, runner: HookRunner) {
        self.hook_runner = Some(runner);
    }

    pub fn set_workspace_monitor(&mut self, monitor: Arc<WorkspaceMonitor>) {
        if let Ok(mut wm) = self.workspace_monitor.write() {
            *wm = Some(monitor);
        }
    }

    /// Default tool requirements: bash/pwsh/code_exec→DangerFullAccess, file_write/edit→WorkspaceWrite, reads→ReadOnly
    pub fn set_default_permission_policy(&mut self) {
        let policy = PermissionPolicy::new(PermissionMode::Allow)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_tool_requirement("powershell", PermissionMode::DangerFullAccess)
            .with_tool_requirement("code_execute", PermissionMode::DangerFullAccess)
            .with_tool_requirement("file_write", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("file_edit", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("file_read", PermissionMode::ReadOnly)
            .with_tool_requirement("grep_search", PermissionMode::ReadOnly)
            .with_tool_requirement("glob_search", PermissionMode::ReadOnly)
            .with_tool_requirement("web_search", PermissionMode::ReadOnly)
            .with_tool_requirement("web_fetch", PermissionMode::ReadOnly);
        self.permission_policy = Some(policy);
    }

    fn register_builtins(&mut self) {
        // 所有工具对所有角色开放, LLM 根据 agent.md 中的角色描述自主选择
        let all: &[&str] = &[];
        self.register("glob_search", "Find files by glob pattern.", json!({
            "properties": {"pattern": {"type":"string"},"path": {"type":"string"}},
            "required": ["pattern"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_glob_search(input).await })), all);
        self.register("grep_search", "Search file contents with regex.", json!({
            "properties": {
                "pattern": {"type":"string","description":"Regex pattern to search for"},
                "path": {"type":"string","description":"Directory to search in"},
                "glob": {"type":"string","description":"File glob pattern (e.g. *.rs)"},
                "output_mode": {"type":"string","description":"Output mode: files_with_matches | content | count"},
                "before": {"type":"integer","description":"Lines before match (-B)"},
                "after": {"type":"integer","description":"Lines after match (-A)"},
                "context": {"type":"integer","description":"Context lines around match (-C)"},
                "line_numbers": {"type":"boolean","description":"Show line numbers (default true)"},
                "head_limit": {"type":"integer","description":"Limit number of results (default 250)"},
                "offset": {"type":"integer","description":"Skip first N results"},
                "-i": {"type":"boolean","description":"Case insensitive search"},
                "multiline": {"type":"boolean","description":"Enable multiline mode"},
                "file_type": {"type":"string","description":"File type filter (rust, python, etc.)"}
            },
            "required": ["pattern"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_grep_search(input).await })), all);
        self.register("web_fetch", "Fetch a URL into readable text.", json!({
            "properties": {"url": {"type":"string"},"prompt": {"type":"string"}},
            "required": ["url"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_web_fetch(input).await })), all);
        self.register("web_search", "Search the web for information.", json!({
            "properties": {"query": {"type":"string","minLength":2}},
            "required": ["query"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_web_search(input).await })), all);
        self.register("tool_search", "Search available tools by name.", json!({
            "properties": {"query": {"type":"string"},"max_results": {"type":"integer"}},
            "required": ["query"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_tool_search(input).await })), all);
        let ws_read = self.workspace_monitor.clone();
        self.register("file_read", "Read a text file. By default reads the entire file. Use offset/limit only for incremental chunked reading (will be tracked cumulatively).", json!({
            "properties": {
                "path": {"type":"string", "description": "File path to read"},
                "offset": {"type":"integer", "description": "Line offset to start from (0-indexed). Omit to read from beginning."},
                "limit": {"type":"integer", "description": "Number of lines to read. Omit to read all remaining lines from offset."},
                "mode": {"type":"string", "description": "Read mode: auto (default) | full | force_refresh | diff. diff returns only new content since last tracked read."}
            },
            "required": ["path"]
        }), Arc::new(move |input: Value| {
            let ws = ws_read.clone();
            Box::pin(async move {
                let result = builtins::execute_file_read(input).await?;
                if let Ok(guard) = ws.read() {
                    if let Some(ref wm) = *guard {
                        if let Some(path) = result.get("path").and_then(|v| v.as_str()) {
                            wm.read_file(path, crate::tools::workspace_monitor::ReadMode::Full).ok();
                        }
                    }
                }
                Ok(result)
            })
        }), all);
        let ws_write = self.workspace_monitor.clone();
        self.register("file_write", "Write content to a file.", json!({
            "properties": {"path": {"type":"string"},"content": {"type":"string"}},
            "required": ["path","content"]
        }), Arc::new(move |input: Value| {
            let ws = ws_write.clone();
            Box::pin(async move {
                let result = builtins::execute_file_write(input).await?;
                if result.get("success") == Some(&Value::Bool(true)) {
                    if let Ok(guard) = ws.read() {
                        if let Some(ref wm) = *guard {
                            if let Some(path) = result.get("path").and_then(|v| v.as_str()) {
                                wm.mark_file_written(path);
                            }
                        }
                    }
                }
                Ok(result)
            })
        }), all);
        let ws_list = self.workspace_monitor.clone();
        self.register("file_list", "List files in a directory.", json!({
            "properties": {"path": {"type":"string"}},
            "required": []
        }), Arc::new(move |input: Value| {
            let ws = ws_list.clone();
            Box::pin(async move {
                let mut result = builtins::execute_file_list(input).await?;
                if let Ok(guard) = ws.read() {
                    if let Some(ref wm) = *guard {
                        if let Some(entries) = result.get_mut("entries").and_then(|e| e.as_array_mut()) {
                            for entry in entries.iter_mut() {
                                let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let inv = wm.inventory.read();
                                if let Some(file_entry) = inv.get_entry(name) {
                                    entry.as_object_mut().map(|obj| {
                                        obj.insert("state".to_string(), Value::String(file_entry.state.as_str().to_string()));
                                        obj.insert("language".to_string(), Value::String(file_entry.language.clone()));
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(result)
            })
        }), all);
        let bash_desc = if cfg!(target_os = "windows") {
            "Execute a shell command via PowerShell. Use for running python, pytest, etc. Supports most common shell commands.\n\nOUTPUT MANAGEMENT (mandatory):\n- If the command may produce >100 lines of output, pipe through | head -N or | grep <keyword> to limit results\n- Use | tail -N for recent entries, | wc -l to count first, | grep -c to match-count\n- For file searches, constrain the path (e.g. grep ... path/) instead of searching the entire workspace\n- The output will be truncated at 16KB if too large; always filter proactively to avoid losing data"
        } else {
            "Execute a shell command. Use for running python, pytest, etc.\n\nOUTPUT MANAGEMENT (mandatory):\n- If the command may produce >100 lines of output, pipe through | head -N or | grep <keyword> to limit results\n- Use | tail -N for recent entries, | wc -l to count first, | grep -c to match-count\n- For file searches, constrain the path (e.g. grep ... path/) instead of searching the entire workspace\n- The output will be truncated at 16KB if too large; always filter proactively to avoid losing data"
        };
        self.register("bash", bash_desc, json!({
            "properties": {"command": {"type":"string","description":"Shell command to run"},"description": {"type":"string","description":"What this command does"},"timeout": {"type":"integer","description":"Timeout in milliseconds"}},
            "required": ["command"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_bash(input).await })), all);
        let ws_edit = self.workspace_monitor.clone();
        self.register("file_edit", "Edit a file by replacing old_string with new_string.", json!({
            "properties": {
                "path": {"type":"string","description":"File path to edit"},
                "old_string": {"type":"string","description":"Text to find and replace"},
                "new_string": {"type":"string","description":"Replacement text"},
                "replace_all": {"type":"boolean","description":"Replace all occurrences (default: false)"}
            },
            "required": ["path","old_string","new_string"]
        }), Arc::new(move |input: Value| {
            let ws = ws_edit.clone();
            Box::pin(async move {
                let result = builtins::execute_file_edit(input).await?;
                if result.get("success") == Some(&Value::Bool(true)) {
                    if let Ok(guard) = ws.read() {
                        if let Some(ref wm) = *guard {
                            if let Some(path) = result.get("path").and_then(|v| v.as_str()) {
                                wm.mark_file_written(path);
                            }
                        }
                    }
                }
                Ok(result)
            })
        }), all);
        self.register("powershell", "Execute a PowerShell command.", json!({
            "properties": {
                "command": {"type":"string","description":"PowerShell command to run"},
                "description": {"type":"string","description":"What this command does"},
                "timeout": {"type":"integer","description":"Timeout in milliseconds"}
            },
            "required": ["command"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_powershell(input).await })), all);
        self.register("rag_search", "Semantic search for relevant documents using RAG (Retrieval-Augmented Generation).", json!({
            "properties": {"query": {"type":"string","description":"Search query"},"limit": {"type":"integer","description":"Max results"}},
            "required": ["query"]
        }), sync_tool_ref(rag::execute_rag_search), all);
        self.register("rag_index", "Index a document for RAG retrieval.", json!({
            "properties": {"content": {"type":"string","description":"Document content to index"},"iri": {"type":"string","description":"Optional IRI identifier"},"tags": {"type":"array","items":{"type":"string"},"description":"Optional tags"}},
            "required": ["content"]
        }), sync_tool_ref(rag::execute_rag_index), all);
        self.register("rag_chunk", "Split a document into chunks for indexing.", json!({
            "properties": {"content": {"type":"string","description":"Document content to chunk"},"chunk_size": {"type":"integer","description":"Chunk size in characters (default 500)"},"overlap": {"type":"integer","description":"Overlap between chunks (default 50)"}},
            "required": ["content"]
        }), sync_tool_ref(rag::execute_rag_chunk), all);

        // ========== 知识导入工具 ==========
        self.register("knowledge_import_file", "Import knowledge from a file (Markdown, TXT, HTML, JSON, etc.). Auto-chunks and indexes the content.", json!({
            "properties": {
                "path": {"type":"string","description":"File path to import"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Tags for categorization"},
                "chunk_size": {"type":"integer","description":"Chunk size in characters (default 1000)"},
                "overlap": {"type":"integer","description":"Overlap between chunks (default 100)"},
                "auto_detect_title": {"type":"boolean","description":"Auto-detect title from content (default true)"}
            },
            "required": ["path"]
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_import_file(input).await })), all);

        self.register("knowledge_import_url", "Import knowledge from a URL. Fetches and extracts text content from web pages.", json!({
            "properties": {
                "url": {"type":"string","description":"URL to fetch and import"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Tags for categorization"},
                "chunk_size": {"type":"integer","description":"Chunk size in characters (default 1000)"},
                "overlap": {"type":"integer","description":"Overlap between chunks (default 100)"},
                "selector": {"type":"string","description":"CSS selector or regex to extract specific content"}
            },
            "required": ["url"]
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_import_url(input).await })), all);

        self.register("knowledge_import_directory", "Batch import knowledge from a directory. Recursively processes matching files.", json!({
            "properties": {
                "path": {"type":"string","description":"Directory path to import"},
                "pattern": {"type":"string","description":"File pattern (default: *.md,*.txt,*.html,*.json)"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Tags for categorization"},
                "recursive": {"type":"boolean","description":"Recursively process subdirectories (default true)"},
                "chunk_size": {"type":"integer","description":"Chunk size in characters (default 1000)"},
                "overlap": {"type":"integer","description":"Overlap between chunks (default 100)"}
            },
            "required": ["path"]
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_import_directory(input).await })), all);

        self.register("knowledge_list", "List imported knowledge entries with optional filtering.", json!({
            "properties": {
                "tags": {"type":"array","items":{"type":"string"},"description":"Filter by tags"},
                "source_type": {"type":"string","description":"Filter by source type (file, url)"},
                "limit": {"type":"integer","description":"Max results (default 100)"},
                "offset": {"type":"integer","description":"Pagination offset"}
            }
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_list(input).await })), all);

        self.register("knowledge_delete", "Delete imported knowledge entries by IRI or tags.", json!({
            "properties": {
                "iri": {"type":"string","description":"IRI of knowledge entry to delete"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Delete all entries with these tags"},
                "all": {"type":"boolean","description":"Delete all knowledge entries"}
            }
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_delete(input).await })), all);

        self.register("knowledge_search", "Search imported knowledge with keyword matching and optional tag filtering.", json!({
            "properties": {
                "query": {"type":"string","description":"Search query"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Filter by tags"},
                "limit": {"type":"integer","description":"Max results (default 10)"},
                "min_score": {"type":"number","description":"Minimum relevance score (default 0.1)"}
            },
            "required": ["query"]
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_search(input).await })), all);

        self.register("knowledge_update", "Update content or tags of an imported knowledge entry.", json!({
            "properties": {
                "iri": {"type":"string","description":"IRI of knowledge entry to update"},
                "content": {"type":"string","description":"New content"},
                "tags": {"type":"array","items":{"type":"string"},"description":"New or additional tags"},
                "append_tags": {"type":"boolean","description":"Append tags instead of replacing (default false)"}
            },
            "required": ["iri"]
        }), Arc::new(|input: Value| Box::pin(async move { knowledge::execute_knowledge_update(input).await })), all);

        // ========== Skill 创建工具 ==========
        self.register("create_skill", "Create a new Skill definition from natural language description using LLM. The skill will be auto-registered and available for use.", json!({
            "properties": {
                "description": {"type":"string","description":"Natural language description of the skill to create"},
                "skill_name_hint": {"type":"string","description":"Suggested skill name (optional, lowercase with underscores)"},
                "category_hint": {"type":"string","description":"Suggested category (optional): file|network|ai|execution|validation|data|meta|system"},
                "security_level_override": {"type":"string","description":"Security level override (optional): low|normal|high|critical"}
            },
            "required": ["description"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_create_skill(input).await })), &["DA"]);

        self.register("convert_skill", "Convert a Markdown-formatted skill description into a JSON-LD Skill definition. Parses the markdown structure and generates proper skill schema.", json!({
            "properties": {
                "markdown_content": {"type":"string","description":"Markdown content describing the skill"},
                "source_path": {"type":"string","description":"Source file path (optional)"}
            },
            "required": ["markdown_content"]
        }), Arc::new(|input: Value| Box::pin(async move { builtins::execute_convert_skill(input).await })), &["DA","CA"]);

        // ========== 知识图谱工具 ==========
        let kg_store_for_extract = self.kg_store.clone();
        self.register("knowledge_extract", "从非结构化文本中抽取实体和关系，写入知识图谱。使用 LLM 进行智能抽取。", json!({
            "properties": {
                "text": {"type":"string","description":"待抽取的文本内容"},
                "domain": {"type":"string","description":"领域过滤 (可选，如 business/core)"}
            },
            "required": ["text"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_extract.clone();
            Box::pin(async move { builtins::execute_knowledge_extract(input, kg_store).await })
        }), all);

        let kg_store_for_query = self.kg_store.clone();
        self.register("knowledge_query", "执行 SPARQL SELECT 查询知识图谱。", json!({
            "properties": {
                "sparql": {"type":"string","description":"SPARQL SELECT 查询语句"},
                "named_graph": {"type":"string","description":"命名图 IRI (可选)"}
            },
            "required": ["sparql"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_query.clone();
            Box::pin(async move { builtins::execute_knowledge_query(input, kg_store).await })
        }), all);

        let kg_store_for_search = self.kg_store.clone();
        self.register("kg_search", "在知识图谱中模糊搜索实体。", json!({
            "properties": {
                "keyword": {"type":"string","description":"搜索关键词"},
                "entity_type": {"type":"string","description":"实体类型 IRI 过滤 (可选)"}
            },
            "required": ["keyword"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_search.clone();
            Box::pin(async move { builtins::execute_knowledge_search(input, kg_store).await })
        }), all);

        let kg_store_for_neighbors = self.kg_store.clone();
        self.register("knowledge_neighbors", "获取指定实体的邻居节点和关系。", json!({
            "properties": {
                "entity_id": {"type":"string","description":"实体 ID 或 IRI"},
                "depth": {"type":"integer","description":"遍历深度 (1-3, 默认 1)"}
            },
            "required": ["entity_id"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_neighbors.clone();
            Box::pin(async move { builtins::execute_knowledge_neighbors(input, kg_store).await })
        }), all);

        let kg_store_for_import = self.kg_store.clone();
        self.register("knowledge_import_json", "将结构化 JSON 数据映射为知识图谱节点。", json!({
            "properties": {
                "json_data": {"type":"string","description":"JSON 格式的数据 (对象或数组)"},
                "mapping_config": {"type":"string","description":"映射配置 JSON: {id_field, type_field, label_field, relations:[{field, relation, target_prefix}]}"}
            },
            "required": ["json_data","mapping_config"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_import.clone();
            Box::pin(async move { builtins::execute_knowledge_import_json(input, kg_store).await })
        }), all);

        let kg_store_for_ontology = self.kg_store.clone();
        self.register("ontology_register", "注册自定义本体类或属性到知识图谱。", json!({
            "properties": {
                "terms": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "iri": {"type":"string","description":"本体术语 IRI"},
                            "label": {"type":"string","description":"术语标签"},
                            "description": {"type":"string","description":"术语描述"},
                            "term_type": {"type":"string","description":"类型: Class | Property | Relation"}
                        },
                        "required": ["iri","label","description","term_type"]
                    },
                    "description": "本体术语列表"
                }
            },
            "required": ["terms"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_ontology.clone();
            Box::pin(async move { builtins::execute_ontology_register(input, kg_store).await })
        }), all);

        let kg_store_for_bridge = self.kg_store.clone();
        self.register("knowledge_bridge", "创建知识图谱实体与技能之间的桥接关系。", json!({
            "properties": {
                "entity_id": {"type":"string","description":"实体 ID"},
                "skill_iri": {"type":"string","description":"技能 IRI"},
                "relation_type": {"type":"string","description":"关系类型: HasSkill | ApplicableIn | RelatedTo (默认 HasSkill)"}
            },
            "required": ["entity_id","skill_iri"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_bridge.clone();
            Box::pin(async move { builtins::execute_knowledge_bridge_with_store(input, kg_store).await })
        }), all);

        let kg_store_for_code = self.kg_store.clone();
        self.register("knowledge_extract_code", "使用 tree-sitter 从代码文件中提取 AST 结构（函数、类、导入、调用关系等），写入知识图谱。支持增量更新：文件未变化时自动跳过。支持 Rust/Python/JS/TS/Go/Java/C/C++。", json!({
            "properties": {
                "file_path": {"type":"string","description":"代码文件路径"},
                "named_graph": {"type":"string","description":"命名图 IRI (可选，默认 graph:code)"},
                "force": {"type":"boolean","description":"强制全量提取，忽略缓存 (可选，默认 false)"}
            },
            "required": ["file_path"]
        }), Arc::new(move |input: Value| {
            let kg_store = kg_store_for_code.clone();
            Box::pin(async move { builtins::execute_knowledge_extract_code(input, kg_store).await })
        }), all);

        // ========== L3 投影查询工具 ==========
        let proj_for_tool = self.projection_engine.clone();
        self.register("read_agent_output", "通过 L3 投影读取指定 agent 的完整输出。用于查看前序 agent（PA/DA/CA/AA）的详细报告。node_iri 可从任务上下文中获取（格式如 iri://task/xxx/turn_3）。", json!({
            "properties": {
                "node_iri": {"type":"string","description":"要读取的 L2 节点 IRI（如 iri://task/xxx/turn_3）"}
            },
            "required": ["node_iri"]
        }), Arc::new(move |input: Value| {
            let proj = proj_for_tool.clone();
            Box::pin(async move {
                let node_iri = input
                    .get("node_iri")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "缺少 node_iri 参数".to_string())?;
                let guard = proj.read().map_err(|e| format!("投影引擎读锁失败: {}", e))?;
                let engine = guard.as_ref()
                    .ok_or_else(|| "投影引擎未初始化".to_string())?;
                let result = engine.read_node(node_iri)
                    .map_err(|e| format!("读取 L2 节点失败: {}", e))?;
                match result {
                    Some(node) => Ok(node),
                    None => Err(format!("节点未找到: {}", node_iri)),
                }
            })
        }), all);
    }

    /// Register a tool with role whitelist. 空 = 所有角色可用.
    pub fn register(&mut self, name: &str, description: &str, parameters: Value, handler: ToolFn, allowed_roles: &[&str]) {
        let roles: Vec<String> = allowed_roles.iter().map(|s| s.to_string()).collect();
        self.tools.insert(name.to_string(), handler);
        
        if let Some(existing) = self.tool_descriptions.iter_mut().find(|td| td.name == name) {
            existing.description = description.to_string();
            existing.parameters = parameters.clone();
            existing.allowed_roles = roles;
        } else {
            self.tool_descriptions.push(ToolDescription {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
                allowed_roles: roles,
            });
            // 微工具描述上限：超过时移除最早注册的条目
            if Self::is_micro_tool_name(name) {
                while self.tool_descriptions.iter()
                    .filter(|td| Self::is_micro_tool_name(&td.name))
                    .count() > MAX_MICRO_TOOL_DESCRIPTIONS
                {
                    // position() 返回第一个匹配项（最早注册的）
                    if let Some(pos) = self.tool_descriptions.iter()
                        .position(|td| Self::is_micro_tool_name(&td.name))
                    {
                        self.tool_descriptions.remove(pos);
                    } else {
                        break;
                    }
                }
            }
        }
    }

    fn is_micro_tool_name(name: &str) -> bool {
        MICRO_TOOL_PREFIXES.iter().any(|p| name.starts_with(p))
    }

    /// 注册微工具（动态生成的工具，用于查询大型工具结果）
    pub fn register_micro_tool(&mut self, tool_name: &str, context: MicroToolContext) {
        let contexts = Arc::clone(&self.micro_tool_contexts);
        let data = Arc::clone(&self.micro_tool_data);
        let tool_name_owned = tool_name.to_string();
        
        if let Ok(mut ctx_guard) = contexts.write() {
            ctx_guard.insert(tool_name.to_string(), context.clone());
        }
        
        let description = if tool_name.starts_with("read_full_result_") {
            format!("读取工具完整结果。call_id: {}", context.call_id)
        } else if tool_name.starts_with("query_") {
            format!("查询实体类型: {:?}。call_id: {}", context.entity_types, context.call_id)
        } else if tool_name.starts_with("get_entity_details_") {
            format!("获取实体详情。call_id: {}", context.call_id)
        } else {
            format!("微工具: {}", tool_name)
        };

        let params = json!({
            "type": "object",
            "properties": {
                "offset": {"type": "integer", "description": "起始位置"},
                "limit": {"type": "integer", "description": "返回数量限制"}
            }
        });

        self.register(tool_name, &description, params, Arc::new(move |input: Value| {
            let contexts = contexts.clone();
            let tool_name_owned = tool_name_owned.clone();
            let data = data.clone();
            Box::pin(async move {
            let offset = input["offset"].as_u64().unwrap_or(0) as usize;
            let limit = input["limit"].as_u64().unwrap_or(100) as usize;

            let ctx_guard = contexts.read().map_err(|e| format!("获取上下文锁失败: {}", e))?;
            let ctx = ctx_guard.get(&tool_name_owned)
                .ok_or_else(|| format!("微工具上下文未找到: {}", tool_name_owned))?;

            let data_guard = data.read().map_err(|e| format!("获取数据锁失败: {}", e))?;
            let stored_data = data_guard.get(&ctx.storage_key)
                .ok_or_else(|| format!("微工具数据未找到: {}", ctx.storage_key))?;

            if tool_name_owned.starts_with("read_full_result_") {
                if let Some(content) = stored_data.get("content").and_then(|v| v.as_str()) {
                    let lines: Vec<&str> = content.lines().collect();
                    let selected: Vec<String> = lines.iter()
                        .skip(offset)
                        .take(limit)
                        .map(|l| l.to_string())
                        .collect();
                    return Ok(json!({
                        "content": selected.join("\n"),
                        "total_lines": lines.len(),
                        "offset": offset,
                        "returned": selected.len(),
                        "call_id": ctx.call_id,
                    }));
                }
            } else if tool_name_owned.starts_with("query_") {
                if let Some(content) = stored_data.get("content").and_then(|v| v.as_str()) {
                    let query_type = input["entity_type"].as_str().unwrap_or("");
                    let keyword = input["keyword"].as_str().unwrap_or("");
                    
                    let mut results = Vec::new();
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                        if let Some(arr) = parsed.as_array() {
                            for item in arr.iter().skip(offset).take(limit) {
                                let type_match = query_type.is_empty() || 
                                    item.get("type").and_then(|v| v.as_str()).map(|t| t.contains(query_type)).unwrap_or(false);
                                let keyword_match = keyword.is_empty() ||
                                    item.to_string().to_lowercase().contains(&keyword.to_lowercase());
                                if type_match && keyword_match {
                                    results.push(item.clone());
                                }
                            }
                        }
                    }
                    return Ok(json!({
                        "results": results,
                        "count": results.len(),
                        "call_id": ctx.call_id,
                    }));
                }
            } else if tool_name_owned.starts_with("get_entity_details_") {
                let entity_id = input["entity_id"].as_str().unwrap_or("");
                if let Some(content) = stored_data.get("content").and_then(|v| v.as_str()) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                        if let Some(arr) = parsed.as_array() {
                            for item in arr {
                                if item.get("id").and_then(|v| v.as_str()) == Some(entity_id) {
                                    return Ok(json!({
                                        "entity": item,
                                        "call_id": ctx.call_id,
                                    }));
                                }
                            }
                        }
                    }
                }
                return Ok(json!({
                    "error": "实体未找到",
                    "entity_id": entity_id,
                    "call_id": ctx.call_id,
                }));
            }

            Ok(json!({
                "data": stored_data,
                "call_id": ctx.call_id,
            }))
        })
    }), &[]);
    }

    /// 存储微工具数据
    pub fn store_micro_tool_data(&self, storage_key: &str, data: serde_json::Value) {
        if let Ok(mut guard) = self.micro_tool_data.write() {
            guard.insert(storage_key.to_string(), data);
        }
    }

    /// 获取已注册的微工具列表
    pub fn get_micro_tool_names(&self) -> Vec<String> {
        if let Ok(guard) = self.micro_tool_contexts.read() {
            guard.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub async fn execute(&self, name: &str, input: Value) -> Result<Value, String> {
        let input_str = input.to_string();

        if let Some(ref policy) = self.permission_policy {
            match policy.authorize(name, &input_str, None) {
                PermissionOutcome::Deny { reason } => {
                    return Ok(json!({"error": format!("Permission denied: {}", reason)}));
                }
                PermissionOutcome::Allow => {}
            }
        }

        if let Some(ref runner) = self.hook_runner {
            let hook_result = runner.run_pre_tool_use(name, &input_str);
            if hook_result.is_denied() {
                return Ok(json!({"error": format!("Pre-tool hook denied: {}", hook_result.messages().join("; "))}));
            }
            if hook_result.is_failed() {
                return Ok(json!({"error": format!("Pre-tool hook failed: {}", hook_result.messages().join("; "))}));
            }
            if hook_result.is_cancelled() {
                return Ok(json!({"error": "Pre-tool hook was cancelled"}));
            }
        }

        if let Some(ref gate) = self.syscall_gate {
            if let Err(e) = gate.validate_tool_with_5w2h(name, "unknown", None) {
                return Ok(json!({"error": format!("SyscallGate rejected: {}", e)}));
            }
        }

        let handler = match self.tools.get(name) {
            Some(h) => h.clone(),
            None => return Err(format!("Tool not found: {}", name)),
        };
        debug!(tool = %name, "Executing tool");

        // Execute and capture result for post-hooks
        let result = handler(input).await;

        // Post-tool-use hook
        if let Some(ref runner) = self.hook_runner {
            match &result {
                Ok(output) => {
                    let output_str = output.to_string();
                    let post_result = runner.run_post_tool_use(name, &input_str, &output_str, false);
                    if post_result.is_denied() {
                        return Ok(json!({"error": format!("Post-tool hook denied: {}", post_result.messages().join("; ")), "original_output": output}));
                    }
                }
                Err(e) => {
                    let _ = runner.run_post_tool_use_failure(name, &input_str, e);
                }
            }
        }

        result
    }

    /// 获取工具处理函数（避免跨 await 持有锁）
    pub fn get_handler(&self, name: &str) -> Option<ToolFn> {
        self.tools.get(name).cloned()
    }

    /// 获取工具处理函数（带微工具 fallback）。
    /// 当普通查找失败时，尝试从微工具数据存储中动态构建 handler，
    /// 避免 LLM 因 registry/handler 不一致而反复重试并耗尽 turns。
    pub fn try_get_handler(&self, name: &str) -> Option<ToolFn> {
        // 1. 先查已注册的 handler
        if let Some(handler) = self.tools.get(name) {
            return Some(handler.clone());
        }
        // 2. Fallback: 对 read_full_result_* 微工具从存储数据动态构建 handler
        if name.starts_with("read_full_result_") {
            return self.make_micro_tool_fallback_handler(name);
        }
        None
    }

    /// 为微工具动态构建 fallback handler（从 micro_tool_data / micro_tool_contexts 读取）
    fn make_micro_tool_fallback_handler(&self, name: &str) -> Option<ToolFn> {
        let ctx_guard = self.micro_tool_contexts.read().ok()?;
        let ctx = ctx_guard.get(name)?.clone();
        let storage_key = ctx.storage_key.clone();
        let call_id = ctx.call_id.clone();
        drop(ctx_guard);

        let data_guard = self.micro_tool_data.read().ok()?;
        let stored_data = data_guard.get(&storage_key)?.clone();
        drop(data_guard);

        Some(Arc::new(move |input: Value| {
            let storage_key = storage_key.clone();
            let call_id = call_id.clone();
            let stored_data = stored_data.clone();

            Box::pin(async move {
                let offset = input["offset"].as_u64().unwrap_or(0) as usize;
                let limit = input["limit"].as_u64().unwrap_or(100) as usize;

                if let Some(content) = stored_data.get("content").and_then(|v| v.as_str()) {
                    let lines: Vec<&str> = content.lines().collect();
                    let selected: Vec<String> = lines
                        .iter()
                        .skip(offset)
                        .take(limit)
                        .map(|l| l.to_string())
                        .collect();
                    return Ok(serde_json::json!({
                        "content": selected.join("
"),
                        "total_lines": lines.len(),
                        "offset": offset,
                        "returned": selected.len(),
                        "call_id": call_id,
                    }));
                }

                Ok(serde_json::json!({
                    "data": stored_data,
                    "call_id": call_id,
                }))
            })
        }))
    }

    /// 列出所有工具
    pub fn list_tools(&self, _role: &str) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// 返回所有工具定义 (LLM 根据 agent.md 中的角色描述自主选择)
    pub fn tool_definitions_for_role(&self, role: &str) -> Vec<Value> {
        let role_name = match role {
            "PA" | "Plan" => "Plan",
            "DA" | "Do" => "Do",
            "CA" | "Check" => "Check",
            "AA" | "Act" => "Act",
            _ => role,
        };
        
        let (default_tools, on_demand_tools) = if let Some(ref manager) = self.tool_group_manager {
            manager.get_tool_names_for_role(role_name)
        } else {
            let is_pa = role == "Plan" || role == "PA";
            let is_aa = role == "Act" || role == "AA";
            if is_pa {
                let default: HashSet<String> = Self::pa_readonly_tools().iter().map(|s| s.to_string()).collect();
                (default.clone(), default)
            } else if is_aa {
                // 设计: AA = Core(file_read,file_list) + System(tool_search) 默认, Search+Knowledge 按需
                let aa_tools: HashSet<String> = [
                    "file_read", "file_list", "tool_search",
                    "grep_search", "glob_search", "rag_search", "kg_search", "codebase_search",
                    "knowledge_list", "knowledge_search", "knowledge_extract_code",
                ].iter().map(|s| s.to_string()).collect();
                let all = aa_tools.clone();
                (all, aa_tools)
            } else {
                let all: HashSet<String> = self.tool_descriptions.iter().map(|td| td.name.clone()).collect();
                (all.clone(), all)
            }
        };
        
        let mut result: Vec<Value> = self.tool_descriptions.iter()
            .filter(|td| {
                if !td.allowed_roles.is_empty() {
                    return td.allowed_roles.iter().any(|r| r == role);
                }
                default_tools.contains(&td.name) || on_demand_tools.contains(&td.name)
            })
            .map(|td| {
                let mut params = td.parameters.clone();
                if params.get("type").is_none() {
                    params["type"] = json!("object");
                }
                json!({
                    "type": "function",
                    "function": {
                        "name": td.name,
                        "description": td.description,
                        "parameters": params,
                    }
                })
            })
            .collect();

        let tool_names: Vec<&str> = result.iter()
            .filter_map(|v| v["function"]["name"].as_str())
            .collect();
        tracing::debug!("[tool_definitions_for_role] role={}, filtered={}/{}, tools={:?}",
            role, result.len(), self.tool_descriptions.len(), tool_names);

        result
    }

    pub fn pa_readonly_tools() -> &'static [&'static str] {
        &[
            "file_read", "file_list", "glob_search", "grep_search",
            "web_search", "web_fetch", "tool_search",
            "rag_search", "knowledge_list", "knowledge_search", "kg_search",
            "knowledge_extract_code", "bash",
        ]
    }

    pub fn is_pa_readonly_tool(name: &str) -> bool {
        Self::pa_readonly_tools().contains(&name)
    }

    /// ToolSearch needs access to the tool list
    pub fn search_tools(&self, query: &str, max_results: Option<usize>) -> Value {
        let query_lower = query.to_lowercase();
        let max = max_results.unwrap_or(10);
        let mut matches: Vec<Value> = self
            .tool_descriptions
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&query_lower)
                    || t.description.to_lowercase().contains(&query_lower)
            })
            .take(max)
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                })
            })
            .collect();
        json!({
            "matches": matches,
            "count": matches.len(),
            "query": query,
        })
    }
}

