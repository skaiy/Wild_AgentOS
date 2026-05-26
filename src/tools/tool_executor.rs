use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

use crate::tools::builtin::rag;
use crate::tools::builtin::knowledge;
use crate::tools::knowledge_graph::code_ast::CodeAstExtractor;
use crate::tools::knowledge_graph::extractor::KnowledgeExtractor;
use crate::tools::knowledge_graph::ontology::OntologyManager;
use crate::tools::knowledge_graph::rdf_mapper::RdfMapper;
use crate::tools::knowledge_graph::store::KnowledgeGraphStore;
use crate::tools::knowledge_graph::types::{BridgeRelationType, NodeDef, EdgeDef, RdfQuad, RdfValue};

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

type ToolFn = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

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
    kg_store: Arc<Mutex<KnowledgeGraphStore>>,
    micro_tool_contexts: Arc<std::sync::RwLock<HashMap<String, MicroToolContext>>>,
    micro_tool_data: Arc<std::sync::RwLock<HashMap<String, serde_json::Value>>>,
}

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
        let kg_store = Arc::new(Mutex::new(
            KnowledgeGraphStore::new().expect("创建知识图谱存储失败")
        ));
        let mut exe = Self {
            tools: HashMap::new(),
            tool_descriptions: Vec::new(),
            kg_store,
            micro_tool_contexts: Arc::new(std::sync::RwLock::new(HashMap::new())),
            micro_tool_data: Arc::new(std::sync::RwLock::new(HashMap::new())),
        };
        exe.register_builtins();
        exe
    }

    fn register_builtins(&mut self) {
        // 所有工具对所有角色开放, LLM 根据 agent.md 中的角色描述自主选择
        let all: &[&str] = &[];
        self.register("glob_search", "Find files by glob pattern.", json!({
            "properties": {"pattern": {"type":"string"},"path": {"type":"string"}},
            "required": ["pattern"]
        }), execute_glob_search, all);
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
        }), execute_grep_search, all);
        self.register("WebFetch", "Fetch a URL into readable text.", json!({
            "properties": {"url": {"type":"string"},"prompt": {"type":"string"}},
            "required": ["url"]
        }), execute_web_fetch, all);
        self.register("WebSearch", "Search the web for information.", json!({
            "properties": {"query": {"type":"string","minLength":2}},
            "required": ["query"]
        }), execute_web_search, all);
        self.register("ToolSearch", "Search available tools by name.", json!({
            "properties": {"query": {"type":"string"},"max_results": {"type":"integer"}},
            "required": ["query"]
        }), execute_tool_search, all);
        self.register("file_read", "Read a text file.", json!({
            "properties": {"path": {"type":"string"},"offset": {"type":"integer"},"limit": {"type":"integer"}},
            "required": ["path"]
        }), execute_file_read, all);
        self.register("file_write", "Write content to a file.", json!({
            "properties": {"path": {"type":"string"},"content": {"type":"string"}},
            "required": ["path","content"]
        }), execute_file_write, all);
        self.register("file_list", "List files in a directory.", json!({
            "properties": {"path": {"type":"string"}},
            "required": []
        }), execute_file_list, all);
        self.register("Bash", "Execute a shell command. Use for running python, pytest, etc.", json!({
            "properties": {"command": {"type":"string","description":"Shell command to run"},"description": {"type":"string","description":"What this command does"},"timeout": {"type":"integer","description":"Timeout in milliseconds"}},
            "required": ["command"]
        }), execute_bash, all);
        self.register("file_edit", "Edit a file by replacing old_string with new_string.", json!({
            "properties": {
                "path": {"type":"string","description":"File path to edit"},
                "old_string": {"type":"string","description":"Text to find and replace"},
                "new_string": {"type":"string","description":"Replacement text"},
                "replace_all": {"type":"boolean","description":"Replace all occurrences (default: false)"}
            },
            "required": ["path","old_string","new_string"]
        }), execute_file_edit, all);
        self.register("PowerShell", "Execute a PowerShell command.", json!({
            "properties": {
                "command": {"type":"string","description":"PowerShell command to run"},
                "description": {"type":"string","description":"What this command does"},
                "timeout": {"type":"integer","description":"Timeout in milliseconds"}
            },
            "required": ["command"]
        }), execute_powershell, all);
        self.register("rag_search", "Semantic search for relevant documents using RAG (Retrieval-Augmented Generation).", json!({
            "properties": {"query": {"type":"string","description":"Search query"},"limit": {"type":"integer","description":"Max results"}},
            "required": ["query"]
        }), rag::execute_rag_search, all);
        self.register("rag_index", "Index a document for RAG retrieval.", json!({
            "properties": {"content": {"type":"string","description":"Document content to index"},"iri": {"type":"string","description":"Optional IRI identifier"},"tags": {"type":"array","items":{"type":"string"},"description":"Optional tags"}},
            "required": ["content"]
        }), rag::execute_rag_index, all);
        self.register("rag_chunk", "Split a document into chunks for indexing.", json!({
            "properties": {"content": {"type":"string","description":"Document content to chunk"},"chunk_size": {"type":"integer","description":"Chunk size in characters (default 500)"},"overlap": {"type":"integer","description":"Overlap between chunks (default 50)"}},
            "required": ["content"]
        }), rag::execute_rag_chunk, all);

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
        }), knowledge::execute_knowledge_import_file, all);

        self.register("knowledge_import_url", "Import knowledge from a URL. Fetches and extracts text content from web pages.", json!({
            "properties": {
                "url": {"type":"string","description":"URL to fetch and import"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Tags for categorization"},
                "chunk_size": {"type":"integer","description":"Chunk size in characters (default 1000)"},
                "overlap": {"type":"integer","description":"Overlap between chunks (default 100)"},
                "selector": {"type":"string","description":"CSS selector or regex to extract specific content"}
            },
            "required": ["url"]
        }), knowledge::execute_knowledge_import_url, all);

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
        }), knowledge::execute_knowledge_import_directory, all);

        self.register("knowledge_list", "List imported knowledge entries with optional filtering.", json!({
            "properties": {
                "tags": {"type":"array","items":{"type":"string"},"description":"Filter by tags"},
                "source_type": {"type":"string","description":"Filter by source type (file, url)"},
                "limit": {"type":"integer","description":"Max results (default 100)"},
                "offset": {"type":"integer","description":"Pagination offset"}
            }
        }), knowledge::execute_knowledge_list, all);

        self.register("knowledge_delete", "Delete imported knowledge entries by IRI or tags.", json!({
            "properties": {
                "iri": {"type":"string","description":"IRI of knowledge entry to delete"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Delete all entries with these tags"},
                "all": {"type":"boolean","description":"Delete all knowledge entries"}
            }
        }), knowledge::execute_knowledge_delete, all);

        self.register("knowledge_search", "Search imported knowledge with keyword matching and optional tag filtering.", json!({
            "properties": {
                "query": {"type":"string","description":"Search query"},
                "tags": {"type":"array","items":{"type":"string"},"description":"Filter by tags"},
                "limit": {"type":"integer","description":"Max results (default 10)"},
                "min_score": {"type":"number","description":"Minimum relevance score (default 0.1)"}
            },
            "required": ["query"]
        }), knowledge::execute_knowledge_search, all);

        self.register("knowledge_update", "Update content or tags of an imported knowledge entry.", json!({
            "properties": {
                "iri": {"type":"string","description":"IRI of knowledge entry to update"},
                "content": {"type":"string","description":"New content"},
                "tags": {"type":"array","items":{"type":"string"},"description":"New or additional tags"},
                "append_tags": {"type":"boolean","description":"Append tags instead of replacing (default false)"}
            },
            "required": ["iri"]
        }), knowledge::execute_knowledge_update, all);

        // ========== Skill 创建工具 ==========
        self.register("create_skill", "Create a new Skill definition from natural language description using LLM. The skill will be auto-registered and available for use.", json!({
            "properties": {
                "description": {"type":"string","description":"Natural language description of the skill to create"},
                "skill_name_hint": {"type":"string","description":"Suggested skill name (optional, lowercase with underscores)"},
                "category_hint": {"type":"string","description":"Suggested category (optional): file|network|ai|execution|validation|data|meta|system"},
                "security_level_override": {"type":"string","description":"Security level override (optional): low|normal|high|critical"}
            },
            "required": ["description"]
        }), execute_create_skill, &["DA"]);

        self.register("convert_skill", "Convert a Markdown-formatted skill description into a JSON-LD Skill definition. Parses the markdown structure and generates proper skill schema.", json!({
            "properties": {
                "markdown_content": {"type":"string","description":"Markdown content describing the skill"},
                "source_path": {"type":"string","description":"Source file path (optional)"}
            },
            "required": ["markdown_content"]
        }), execute_convert_skill, &["DA","CA"]);

        // ========== 知识图谱工具 ==========
        let kg_store_for_extract = self.kg_store.clone();
        self.register("knowledge_extract", "从非结构化文本中抽取实体和关系，写入知识图谱。使用 LLM 进行智能抽取。", json!({
            "properties": {
                "text": {"type":"string","description":"待抽取的文本内容"},
                "domain": {"type":"string","description":"领域过滤 (可选，如 business/core)"}
            },
            "required": ["text"]
        }), move |input: &Value| execute_knowledge_extract(input, &kg_store_for_extract), all);

        let kg_store_for_query = self.kg_store.clone();
        self.register("knowledge_query", "执行 SPARQL SELECT 查询知识图谱。", json!({
            "properties": {
                "sparql": {"type":"string","description":"SPARQL SELECT 查询语句"},
                "named_graph": {"type":"string","description":"命名图 IRI (可选)"}
            },
            "required": ["sparql"]
        }), move |input: &Value| execute_knowledge_query(input, &kg_store_for_query), all);

        let kg_store_for_search = self.kg_store.clone();
        self.register("kg_search", "在知识图谱中模糊搜索实体。", json!({
            "properties": {
                "keyword": {"type":"string","description":"搜索关键词"},
                "entity_type": {"type":"string","description":"实体类型 IRI 过滤 (可选)"}
            },
            "required": ["keyword"]
        }), move |input: &Value| execute_knowledge_search(input, &kg_store_for_search), all);

        let kg_store_for_neighbors = self.kg_store.clone();
        self.register("knowledge_neighbors", "获取指定实体的邻居节点和关系。", json!({
            "properties": {
                "entity_id": {"type":"string","description":"实体 ID 或 IRI"},
                "depth": {"type":"integer","description":"遍历深度 (1-3, 默认 1)"}
            },
            "required": ["entity_id"]
        }), move |input: &Value| execute_knowledge_neighbors(input, &kg_store_for_neighbors), all);

        let kg_store_for_import = self.kg_store.clone();
        self.register("knowledge_import_json", "将结构化 JSON 数据映射为知识图谱节点。", json!({
            "properties": {
                "json_data": {"type":"string","description":"JSON 格式的数据 (对象或数组)"},
                "mapping_config": {"type":"string","description":"映射配置 JSON: {id_field, type_field, label_field, relations:[{field, relation, target_prefix}]}"}
            },
            "required": ["json_data","mapping_config"]
        }), move |input: &Value| execute_knowledge_import_json(input, &kg_store_for_import), all);

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
        }), move |input: &Value| execute_ontology_register(input, &kg_store_for_ontology), all);

        let kg_store_for_bridge = self.kg_store.clone();
        self.register("knowledge_bridge", "创建知识图谱实体与技能之间的桥接关系。", json!({
            "properties": {
                "entity_id": {"type":"string","description":"实体 ID"},
                "skill_iri": {"type":"string","description":"技能 IRI"},
                "relation_type": {"type":"string","description":"关系类型: HasSkill | ApplicableIn | RelatedTo (默认 HasSkill)"}
            },
            "required": ["entity_id","skill_iri"]
        }), move |input: &Value| execute_knowledge_bridge_with_store(input, &kg_store_for_bridge), all);

        let kg_store_for_code = self.kg_store.clone();
        self.register("knowledge_extract_code", "使用 tree-sitter 从代码文件中提取 AST 结构（函数、类、导入、调用关系等），写入知识图谱。支持增量更新：文件未变化时自动跳过。支持 Rust/Python/JS/TS/Go/Java/C/C++。", json!({
            "properties": {
                "file_path": {"type":"string","description":"代码文件路径"},
                "named_graph": {"type":"string","description":"命名图 IRI (可选，默认 graph:code)"},
                "force": {"type":"boolean","description":"强制全量提取，忽略缓存 (可选，默认 false)"}
            },
            "required": ["file_path"]
        }), move |input: &Value| execute_knowledge_extract_code(input, &kg_store_for_code), all);
    }

    /// Register a tool with role whitelist. 空 = 所有角色可用.
    pub fn register<F>(&mut self, name: &str, description: &str, parameters: Value, handler: F, allowed_roles: &[&str])
    where
        F: Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let roles: Vec<String> = allowed_roles.iter().map(|s| s.to_string()).collect();
        self.tools.insert(name.to_string(), Arc::new(handler));
        
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
        }
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

        self.register(tool_name, &description, params, move |input: &Value| {
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
        }, &[]);
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

    pub fn execute(&self, name: &str, input: &Value) -> Result<Value, String> {
        let handler = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Tool not found: {}", name))?;
        debug!(tool = %name, "Executing tool");
        handler(input)
    }

    /// 列出所有工具
    pub fn list_tools(&self, _role: &str) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// 返回所有工具定义 (LLM 根据 agent.md 中的角色描述自主选择)
    pub fn tool_definitions_for_role(&self, role: &str) -> Vec<Value> {
        let pa_readonly_tools = [
            "file_read", "file_list", "glob_search", "grep_search",
            "WebSearch", "WebFetch", "ToolSearch",
            "rag_search", "knowledge_list", "knowledge_search", "kg_search",
            "knowledge_extract_code",
        ];

        let is_pa = role == "Plan" || role == "PA";
        let mut result: Vec<Value> = self.tool_descriptions.iter()
            .filter(|td| {
                if !td.allowed_roles.is_empty() {
                    return td.allowed_roles.iter().any(|r| r == role);
                }
                if is_pa {
                    return pa_readonly_tools.contains(&td.name.as_str());
                }
                true
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

        if is_pa {
            let tool_names: Vec<&str> = result.iter()
                .filter_map(|v| v["function"]["name"].as_str())
                .collect();
            tracing::debug!("[PA] tool_definitions_for_role: role={}, filtered={}/{}, tools={:?}",
                role, result.len(), self.tool_descriptions.len(), tool_names);
        }

        result
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

// ========== Tool implementations ==========

fn execute_glob_search(input: &Value) -> Result<Value, String> {
    let params: GlobSearchInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let root = params.path.as_deref().unwrap_or(".");
    let mut files = Vec::new();
    let glob_pattern = if root != "." {
        format!("{}/{}", root.trim_end_matches('/'), &params.pattern)
    } else {
        params.pattern.clone()
    };
    match glob::glob(&glob_pattern) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if let Some(p) = entry.to_str() {
                    files.push(p.to_string());
                }
            }
        }
        Err(e) => return Err(format!("Glob error: {}", e)),
    }
    files.sort();
    files.dedup();
    Ok(json!({ "files": files, "count": files.len(), "pattern": params.pattern }))
}

fn execute_grep_search(input: &Value) -> Result<Value, String> {
    let params: GrepSearchInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;

    let root = params.path.as_deref().unwrap_or(".");
    let mode = params.output_mode.as_deref().unwrap_or("files_with_matches");
    if mode != "files_with_matches" && mode != "content" && mode != "count" {
        return Err(format!("Invalid output_mode: {}. Must be files_with_matches, content, or count", mode));
    }

    let ci = params.case_insensitive.unwrap_or(false);
    let ml = params.multiline.unwrap_or(false);
    let re = regex::RegexBuilder::new(&params.pattern)
        .case_insensitive(ci)
        .multi_line(ml)
        .dot_matches_new_line(ml)
        .build()
        .map_err(|e| format!("Invalid regex: {}", e))?;

    let before = params.before.or(params.context).unwrap_or(0);
    let after = params.after.or(params.context).unwrap_or(0);
    let show_line_numbers = params.line_numbers.unwrap_or(true);
    let limit = params.head_limit.unwrap_or(250);
    let offset = params.offset.unwrap_or(0);

    let file_glob = resolve_file_glob(params.glob.as_deref(), params.file_type.as_deref());

    let mut filenames: Vec<String> = Vec::new();
    let mut total_matches: usize = 0;
    let mut files_with_matches: usize = 0;
    let mut all_matches: Vec<(String, usize, String)> = Vec::new();
    let mut file_contents: HashMap<String, String> = HashMap::new();

    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();

        if !match_glob(&path_str, &file_glob) {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut file_had_match = false;
        for (line_idx, line) in content.lines().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            total_matches += 1;
            if !file_had_match {
                files_with_matches += 1;
                filenames.push(path_str.clone());
                file_had_match = true;
            }
            all_matches.push((path_str.clone(), line_idx, line.to_string()));
        }

        if file_had_match {
            file_contents.insert(path_str.clone(), content);
        }
    }

    let skipped = std::cmp::min(offset, all_matches.len());
    let selected: Vec<_> = all_matches.iter().skip(skipped).take(limit).collect();
    let applied_limit = limit;
    let applied_offset = offset;

    match mode {
        "files_with_matches" => {
            let limited_files: Vec<String> = filenames.iter().skip(skipped).take(limit).cloned().collect();
            Ok(json!({
                "mode": "files_with_matches",
                "num_files": files_with_matches,
                "filenames": limited_files,
                "applied_limit": applied_limit,
                "applied_offset": applied_offset,
            }))
        }
        "content" => {
            let mut output_parts: Vec<String> = Vec::new();
            for (file, line_idx, _line) in &selected {
                let content = match file_contents.get(file) {
                    Some(c) => c,
                    None => continue,
                };
                let lines_vec: Vec<&str> = content.lines().collect();
                let start = line_idx.saturating_sub(before);
                let end = std::cmp::min(line_idx + after + 1, lines_vec.len());
                for i in start..end {
                    let prefix = if show_line_numbers {
                        format!("{}:{}:", file, i + 1)
                    } else {
                        format!("{}:", file)
                    };
                    output_parts.push(format!("{}{}", prefix, lines_vec[i]));
                }
            }
            Ok(json!({
                "mode": "content",
                "num_files": files_with_matches,
                "filenames": filenames,
                "content": output_parts.join("\n"),
                "num_matches": total_matches,
                "applied_limit": applied_limit,
                "applied_offset": applied_offset,
            }))
        }
        "count" => {
            let mut file_counts: Vec<Value> = Vec::new();
            for fname in &filenames {
                let content = match file_contents.get(fname) {
                    Some(c) => c,
                    None => continue,
                };
                let cnt = content.lines().filter(|l| re.is_match(l)).count();
                file_counts.push(json!({"file": fname, "count": cnt}));
            }
            let limited_counts: Vec<Value> = file_counts.into_iter().skip(skipped).take(limit).collect();
            Ok(json!({
                "mode": "count",
                "num_files": files_with_matches,
                "num_matches": total_matches,
                "counts": limited_counts,
                "applied_limit": applied_limit,
                "applied_offset": applied_offset,
            }))
        }
        _ => Err("Unreachable".to_string()),
    }
}

fn resolve_file_glob(glob: Option<&str>, file_type: Option<&str>) -> String {
    if let Some(ft) = file_type {
        let ext = match ft {
            "rust" => "*.rs",
            "python" => "*.py",
            "javascript" => "*.js",
            "typescript" => "*.ts",
            "java" => "*.java",
            "c" => "*.c",
            "cpp" => "*.cpp",
            "go" => "*.go",
            "ruby" => "*.rb",
            "swift" => "*.swift",
            "kotlin" => "*.kt",
            "scala" => "*.scala",
            "haskell" => "*.hs",
            "lua" => "*.lua",
            "perl" => "*.pl",
            "php" => "*.php",
            "shell" => "*.sh",
            "sql" => "*.sql",
            "html" => "*.html",
            "css" => "*.css",
            "json" => "*.json",
            "yaml" => "*.yml",
            "toml" => "*.toml",
            "xml" => "*.xml",
            "markdown" => "*.md",
            "dockerfile" => "Dockerfile",
            _ => ft,
        };
        return ext.to_string();
    }
    glob.unwrap_or("*").to_string()
}

fn match_glob(path: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if let Ok(glob_matcher) = glob::Pattern::new(pattern) {
        return glob_matcher.matches(file_name);
    }
    file_name.contains(pattern.trim_start_matches('*'))
}

fn execute_web_fetch(input: &Value) -> Result<Value, String> {
    let params: WebFetchInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let started = Instant::now();

    let handle = tokio::runtime::Handle::try_current();
    let (code, ct, content) = match handle {
        Ok(h) => {
            tokio::task::block_in_place(|| {
                h.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(20))
                        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .redirect(reqwest::redirect::Policy::limited(10))
                        .build()
                        .map_err(|e| format!("HTTP client: {}", e))?;
                    let resp = client.get(&params.url).send().await.map_err(|e| format!("Request: {}", e))?;
                    let code = resp.status().as_u16();
                    let ct = resp.headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let body = resp.text().await.map_err(|e| format!("Read: {}", e))?;
                    let content = if ct.contains("html") { html_to_text(&body) } else { body.chars().take(8000).collect() };
                    Ok::<_, String>((code, ct, content))
                })
            })
        }
        Err(_) => {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .map_err(|e| format!("HTTP client: {}", e))?;
            let resp = client.get(&params.url).send().map_err(|e| format!("Request: {}", e))?;
            let code = resp.status().as_u16();
            let ct = resp.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = resp.text().map_err(|e| format!("Read: {}", e))?;
            let content = if ct.contains("html") { html_to_text(&body) } else { body.chars().take(8000).collect() };
            Ok((code, ct, content))
        }
    }?;

    Ok(json!({
        "url": params.url, "status_code": code,
        "content": content, "content_type": ct,
        "duration_ms": started.elapsed().as_millis(),
    }))
}

fn execute_web_search(input: &Value) -> Result<Value, String> {
    let params: WebSearchInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let started = Instant::now();

    let handle = tokio::runtime::Handle::try_current();
    let html_result = match handle {
        Ok(h) => {
            tokio::task::block_in_place(|| {
                h.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(15))
                        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .build()
                        .map_err(|e| format!("HTTP client: {}", e))?;
                    
                    // 优先使用 DuckDuckGo Lite
                    let lite_url = format!("https://lite.duckduckgo.com/lite/?q={}", urlencode(&params.query));
                    let resp = client.get(&lite_url)
                        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
                        .send().await.map_err(|e| format!("Search: {}", e))?;
                    let status = resp.status();
                    let body = resp.text().await.map_err(|e| format!("Read: {}", e))?;
                    
                    if status.as_u16() == 200 && (body.contains("result-link") || body.contains("result__a")) {
                        return Ok::<_, String>((status, body, "lite".to_string()));
                    }
                    
                    // 备选: DuckDuckGo HTML
                    let html_url = format!("https://html.duckduckgo.com/html/?q={}", urlencode(&params.query));
                    let resp = client.get(&html_url)
                        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
                        .send().await.map_err(|e| format!("Search: {}", e))?;
                    let status2 = resp.status();
                    let body2 = resp.text().await.map_err(|e| format!("Read: {}", e))?;
                    
                    if status2.as_u16() == 200 && body2.contains("result__a") {
                        return Ok((status2, body2, "html".to_string()));
                    }
                    
                    // 备选: DuckDuckGo Instant Answer API
                    let api_url = format!("https://api.duckduckgo.com/?q={}&format=json&no_html=1", urlencode(&params.query));
                    let resp = client.get(&api_url).send().await.map_err(|e| format!("API: {}", e))?;
                    let api_body = resp.text().await.map_err(|e| format!("Read: {}", e))?;
                    
                    Ok((status, api_body, "api".to_string()))
                })
            })
        }
        Err(_) => {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .build()
                .map_err(|e| format!("HTTP client: {}", e))?;
            
            let lite_url = format!("https://lite.duckduckgo.com/lite/?q={}", urlencode(&params.query));
            let resp = client.get(&lite_url)
                .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
                .send().map_err(|e| format!("Search: {}", e))?;
            let status = resp.status();
            let body = resp.text().map_err(|e| format!("Read: {}", e))?;
            
            if status.as_u16() == 200 && (body.contains("result-link") || body.contains("result__a")) {
                Ok((status, body, "lite".to_string()))
            } else {
                let api_url = format!("https://api.duckduckgo.com/?q={}&format=json&no_html=1", urlencode(&params.query));
                let resp = client.get(&api_url).send().map_err(|e| format!("API: {}", e))?;
                let api_body = resp.text().map_err(|e| format!("Read: {}", e))?;
                Ok((status, api_body, "api".to_string()))
            }
        }
    };

    match html_result {
        Ok((status, body, source)) => {
            let mut results = Vec::new();
            
            if source == "lite" {
                results = extract_ddg_lite_results(&body);
                if results.is_empty() {
                    results = extract_ddg_results(&body);
                }
            } else if source == "html" {
                results = extract_ddg_results(&body);
            } else if source == "api" {
                results = extract_ddg_api_results(&body);
            }
            
            if results.is_empty() && status.as_u16() != 200 && source != "api" {
                return Ok(json!({
                    "query": params.query,
                    "duration_seconds": started.elapsed().as_secs_f64(),
                    "results": [],
                    "error": format!("搜索引擎返回非200状态码: {}", status),
                    "suggestion": "网络搜索不可用，请基于自身知识回答"
                }));
            }
            
            if let Some(ref allowed) = params.allowed_domains {
                results.retain(|r| {
                    r["url"].as_str().map_or(false, |url| {
                        allowed.iter().any(|d| url.contains(d.as_str()))
                    })
                });
            }
            if let Some(ref blocked) = params.blocked_domains {
                results.retain(|r| {
                    r["url"].as_str().map_or(true, |url| {
                        !blocked.iter().any(|d| url.contains(d.as_str()))
                    })
                });
            }
            results.truncate(8);
            Ok(json!({
                "query": params.query,
                "duration_seconds": started.elapsed().as_secs_f64(),
                "results": results,
            }))
        }
        Err(e) => {
            Ok(json!({
                "query": params.query,
                "duration_seconds": started.elapsed().as_secs_f64(),
                "results": [],
                "error": format!("搜索请求失败: {}", e),
                "suggestion": "网络搜索不可用，请基于自身知识回答"
            }))
        }
    }
}

fn execute_tool_search(input: &Value) -> Result<Value, String> {
    let params: ToolSearchInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let q = params.query.to_lowercase();
    let max = params.max_results.unwrap_or(10);
    let all = vec![
        ("glob_search", "Find files by glob pattern. Supports **, *, ? wildcards. Part of system:skills namespace."),
        ("grep_search", "Search file contents with a regex pattern. Part of system:skills namespace."),
        ("WebFetch", "Fetch a URL and convert it into readable text. Network tool."),
        ("WebSearch", "Search the web for current information. Network tool."),
        ("ToolSearch", "Search available tools by name or keyword. System tool."),
    ];
    let matches: Vec<Value> = all.iter()
        .filter(|(n, d)| n.to_lowercase().contains(&q) || d.to_lowercase().contains(&q))
        .take(max)
        .map(|(n, d)| json!({"name": n, "description": d, "source": "system:skills"}))
        .collect();
    Ok(json!({"matches": matches, "count": matches.len(), "query": params.query}))
}

// ===== File and Bash tool inputs =====

#[derive(Debug, Deserialize)]
struct FileReadInput {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FileWriteInput {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct FileListInput {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BashInput {
    command: String,
    description: Option<String>,
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FileEditInput {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PowerShellInput {
    command: String,
    timeout: Option<u64>,
    description: Option<String>,
    run_in_background: Option<bool>,
}

fn execute_file_read(input: &Value) -> Result<Value, String> {
    let params: FileReadInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let content = std::fs::read_to_string(&params.path).map_err(|e| format!("Read error: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let start = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(usize::MAX);
    let selected: Vec<String> = lines.iter().skip(start).take(limit).map(|l| l.to_string()).collect();
    let total = lines.len();
    Ok(json!({
        "path": params.path, "total_lines": total,
        "offset": start, "lines": selected, "returned": selected.len(),
    }))
}

fn execute_file_write(input: &Value) -> Result<Value, String> {
    let params: FileWriteInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    if let Some(parent) = std::path::Path::new(&params.path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Mkdir error: {}", e))?;
    }
    std::fs::write(&params.path, &params.content).map_err(|e| format!("Write error: {}", e))?;
    Ok(json!({"path": params.path, "bytes_written": params.content.len(), "success": true}))
}

fn execute_file_list(input: &Value) -> Result<Value, String> {
    let params: FileListInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    let dir = params.path.as_deref().unwrap_or(".");
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(dir).map_err(|e| format!("List error: {}", e))?;
    for entry in read_dir.flatten() {
        let ft = entry.file_type().ok();
        let kind = if ft.map_or(false, |t| t.is_dir()) { "dir" } else { "file" };
        if let Ok(name) = entry.file_name().into_string() {
            entries.push(json!({"name": name, "type": kind}));
        }
    }
    Ok(json!({"path": dir, "entries": entries, "count": entries.len()}))
}

fn execute_bash(input: &Value) -> Result<Value, String> {
    let params: BashInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;
    use std::process::Command;
    let timeout_ms = params.timeout.unwrap_or(60_000);
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };

    let mut child = Command::new(shell)
        .arg(flag)
        .arg(&params.command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Spawn error: {}", e))?;

    let start = std::time::Instant::now();
    let max_dur = std::time::Duration::from_millis(timeout_ms);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = read_with_timeout(child.stdout.take(), 5000);
                let stderr = read_with_timeout(child.stderr.take(), 5000);
                let code = status.code().unwrap_or(-1);
                break json!({
                    "command": params.command, "exit_code": code,
                    "stdout": stdout, "stderr": stderr,
                    "duration_ms": start.elapsed().as_millis() as u64,
                });
            }
            Ok(None) => {
                if start.elapsed() > max_dur {
                    let _ = child.kill();
                    kill_process_group(&child);
                    let stdout = read_with_timeout(child.stdout.take(), 2000);
                    let stderr = read_with_timeout(child.stderr.take(), 2000);
                    break json!({
                        "command": params.command, "timed_out": true,
                        "stdout": stdout, "stderr": stderr,
                        "error": format!("Timeout after {}ms", timeout_ms),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => break json!({"command": params.command, "error": e.to_string()}),
        }
    };
    Ok(result)
}

fn read_with_timeout<R: std::io::Read + Send + 'static>(reader: Option<R>, timeout_ms: u64) -> String {
    let Some(reader) = reader else {
        return String::new();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::io::read_to_string(reader).unwrap_or_default();
        let _ = tx.send(output);
    });
    match rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
        Ok(output) => output,
        Err(_) => String::new(),
    }
}

#[cfg(unix)]
fn kill_process_group(child: &std::process::Child) {
    let pgid = child.id();
    let _ = std::process::Command::new("kill")
        .arg(format!("-{}", pgid))
        .output();
}

#[cfg(not(unix))]
fn kill_process_group(_child: &std::process::Child) {}

fn execute_file_edit(input: &Value) -> Result<Value, String> {
    let params: FileEditInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;

    let content = std::fs::read_to_string(&params.path).map_err(|e| format!("Read error: {}", e))?;

    let count = content.matches(&params.old_string).count();
    if count == 0 {
        return Err(format!("old_string not found in {}", params.path));
    }
    if count > 1 && !params.replace_all.unwrap_or(false) {
        return Err(format!(
            "old_string found {} times in {}. Set replace_all=true to replace all occurrences.",
            count, params.path
        ));
    }

    let old_lines: Vec<&str> = params.old_string.lines().collect();
    let new_lines: Vec<&str> = params.new_string.lines().collect();

    let diff = generate_diff(&params.path, &old_lines, &new_lines);

    let new_content = if params.replace_all.unwrap_or(false) {
        content.replace(&params.old_string, &params.new_string)
    } else {
        content.replacen(&params.old_string, &params.new_string, 1)
    };

    let replacements = if params.replace_all.unwrap_or(false) { count } else { 1 };

    std::fs::write(&params.path, &new_content).map_err(|e| format!("Write error: {}", e))?;

    Ok(json!({
        "path": params.path,
        "success": true,
        "replacements": replacements,
        "diff": diff,
    }))
}

fn generate_diff(path: &str, old_lines: &[&str], new_lines: &[&str]) -> String {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", file_name));
    diff.push_str(&format!("+++ b/{}\n", file_name));

    let old_count = old_lines.len();
    let new_count = new_lines.len();
    diff.push_str(&format!("@@ -1,{} +1,{} @@\n", old_count, new_count));

    for line in old_lines {
        diff.push_str(&format!("-{}\n", line));
    }
    for line in new_lines {
        diff.push_str(&format!("+{}\n", line));
    }

    diff
}

fn execute_powershell(input: &Value) -> Result<Value, String> {
    let params: PowerShellInput = serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {}", e))?;

    let exe = if cfg!(target_os = "windows") { "powershell" } else { "pwsh" };

    let exe_path = match which_powershell(exe) {
        Some(p) => p,
        None => return Err(format!("{} not found on this system", exe)),
    };

    let timeout_ms = params.timeout.unwrap_or(60_000);

    let mut child = std::process::Command::new(&exe_path)
        .args(["-NoProfile", "-NonInteractive", "-Command", &params.command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Spawn error: {}", e))?;

    let start = std::time::Instant::now();
    let max_dur = std::time::Duration::from_millis(timeout_ms);

    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = std::io::read_to_string(child.stdout.take().unwrap()).unwrap_or_default();
                let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap_or_default();
                let code = status.code().unwrap_or(-1);
                break json!({
                    "command": params.command, "exit_code": code,
                    "stdout": stdout, "stderr": stderr,
                    "duration_ms": start.elapsed().as_millis() as u64,
                    "shell": exe,
                });
            }
            Ok(None) => {
                if start.elapsed() > max_dur {
                    let _ = child.kill();
                    break json!({
                        "command": params.command, "timed_out": true,
                        "error": format!("Timeout after {}ms", timeout_ms),
                        "shell": exe,
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => break json!({"command": params.command, "error": e.to_string(), "shell": exe}),
        }
    };
    Ok(result)
}

fn which_powershell(exe: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe);
        if candidate.exists() {
            return candidate.to_str().map(|s| s.to_string());
        }
        let with_ext = dir.join(format!("{}.exe", exe));
        if with_ext.exists() {
            return with_ext.to_str().map(|s| s.to_string());
        }
    }
    None
}
fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut prev_space = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => { in_tag = false; if !text.is_empty() && !text.ends_with(' ') { text.push(' '); prev_space = true; } }
            _ if in_tag => {}
            ch if ch.is_whitespace() => { if !prev_space { text.push(' '); prev_space = true; } }
            _ => { text.push(ch); prev_space = false; }
        }
    }
    let decoded = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&nbsp;", " ");
    let mut result = String::with_capacity(decoded.len());
    let mut ps = false;
    for ch in decoded.chars() {
        if ch.is_whitespace() { if !ps { result.push(' '); ps = true; } }
        else { result.push(ch); ps = false; }
    }
    result.truncate(8000);
    result
}

fn extract_ddg_results(html: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let mut rem = html;
    while let Some(s) = rem.find("result__a") {
        let after = &rem[s..];
        let Some(hp) = after.find("href=") else { rem = &after[1..]; continue; };
        let hs = &after[hp + 5..];
        let Some((url, rest)) = extract_quoted_str(hs) else { rem = &after[1..]; continue; };
        let Some(ct) = rest.find('>') else { rem = &after[1..]; continue; };
        let at = &rest[ct + 1..];
        let Some(ea) = at.find("</a>") else { rem = &after[1..]; continue; };
        let title = html_to_text(&at[..ea]);
        rem = &at[ea + 4..];
        let snippet = if let Some(sp) = rem.find("result__snippet") {
            let sa = &rem[sp..];
            if let Some(tc) = sa.find('>') {
                let sc = &sa[tc + 1..];
                if let Some(es) = sc.find("</") { html_to_text(&sc[..es]) } else { String::new() }
            } else { String::new() }
        } else { String::new() };
        if !title.trim().is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            results.push(json!({"title": title.trim(), "url": url, "snippet": snippet.trim()}));
        }
    }
    results
}

fn extract_ddg_api_results(body: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let Ok(data) = serde_json::from_str::<Value>(body) else { return results; };

    // Abstract result
    if let Some(abstract_text) = data["AbstractText"].as_str() {
        if !abstract_text.is_empty() {
            results.push(json!({
                "title": data["AbstractSource"].as_str().unwrap_or(""),
                "url": data["AbstractURL"].as_str().unwrap_or(""),
                "snippet": abstract_text,
            }));
        }
    }

    // Related topics
    if let Some(topics) = data["RelatedTopics"].as_array() {
        for topic in topics {
            if let Some(text) = topic["Text"].as_str() {
                if !text.is_empty() {
                    results.push(json!({
                        "title": text.split_whitespace().take(5).collect::<Vec<_>>().join(" "),
                        "url": topic["FirstURL"].as_str().unwrap_or(""),
                        "snippet": text,
                    }));
                }
            } else if let Some(sub_topics) = topic["Topics"].as_array() {
                for sub in sub_topics {
                    if let Some(text) = sub["Text"].as_str() {
                        if !text.is_empty() {
                            results.push(json!({
                                "title": text.split_whitespace().take(5).collect::<Vec<_>>().join(" "),
                                "url": sub["FirstURL"].as_str().unwrap_or(""),
                                "snippet": text,
                            }));
                        }
                    }
                }
            }
            if results.len() >= 8 { break; }
        }
    }

    results
}

fn extract_ddg_lite_results(html: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let mut rem = html;
    loop {
        let Some(link_start) = rem.find("class=\"result-link\"") else { break; };
        let link_section = &rem[link_start..];
        
        let Some(href_pos) = link_section.find("href=") else { rem = &link_section[1..]; continue; };
        let href_str = &link_section[href_pos + 5..];
        let Some((url, after_url)) = extract_quoted_str(href_str) else { rem = &link_section[1..]; continue; };
        
        let Some(gt_pos) = after_url.find('>') else { rem = &link_section[1..]; continue; };
        let title_start = &after_url[gt_pos + 1..];
        let Some(title_end) = title_start.find("</a>") else { rem = &link_section[1..]; continue; };
        let title = html_to_text(&title_start[..title_end]);
        
        let snippet = if let Some(snip_pos) = after_url.find("class=\"result-snippet\"") {
            let snip_section = &after_url[snip_pos..];
            if let Some(gt) = snip_section.find('>') {
                let snip_text = &snip_section[gt + 1..];
                if let Some(end_tag) = snip_text.find("</td>") {
                    html_to_text(&snip_text[..end_tag])
                } else if let Some(end_tag) = snip_text.find("</") {
                    html_to_text(&snip_text[..end_tag])
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        
        if !title.trim().is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            results.push(json!({"title": title.trim(), "url": url, "snippet": snippet.trim()}));
        }
        
        rem = &title_start[title_end + 4..];
    }
    results
}

fn extract_quoted_str(input: &str) -> Option<(String, &str)> {
    let q = input.chars().next()?;
    if q != '"' && q != '\'' { return None; }
    let rest = &input[q.len_utf8()..];
    let end = rest.find(q)?;
    Some((rest[..end].to_string(), &rest[end + q.len_utf8()..]))
}

fn urlencode(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        ' ' => "+".to_string(),
        c => format!("%{:02X}", c as u8),
    }).collect()
}

static SKILL_CREATOR_GATEWAY: OnceCell<std::sync::Arc<crate::gateway::unified_gateway::UnifiedGateway>> = OnceCell::new();

pub fn init_skill_creator_gateway(gateway: std::sync::Arc<crate::gateway::unified_gateway::UnifiedGateway>) {
    let _ = SKILL_CREATOR_GATEWAY.set(gateway);
}

fn execute_create_skill(input: &Value) -> Result<Value, String> {
    let description = input["description"].as_str().unwrap_or("").to_string();
    if description.is_empty() {
        return Err("description is required".to_string());
    }

    let skill_name_hint = input["skill_name_hint"].as_str().map(String::from);
    let category_hint = input["category_hint"].as_str().map(String::from);
    let security_level_override = input["security_level_override"].as_str().map(String::from);

    if let Some(gateway) = SKILL_CREATOR_GATEWAY.get() {
        let rt = tokio::runtime::Handle::current();
        let graph_store = std::sync::Arc::new(crate::skill_graph::SkillGraphStore::new());
        let registry = std::sync::Arc::new(crate::tools::SkillRegistry::new());
        let config = crate::skill_graph::SkillCreatorConfig::default();
        let creator = crate::skill_graph::SkillCreator::new(
            gateway.clone(), graph_store, registry, config,
        );

        let request = crate::skill_graph::CreateSkillRequest {
            description,
            skill_name_hint,
            category_hint,
            security_level_override,
        };

        let result = rt.block_on(async { creator.create_from_description(request).await })
            .map_err(|e| format!("创建 Skill 失败: {:?}", e))?;

        Ok(json!({
            "skill_iri": result.skill_iri,
            "name": result.name,
            "registered": true,
            "json_ld": result.json_ld,
        }))
    } else {
        let name = skill_name_hint.unwrap_or_else(|| {
            description.split_whitespace().take(2).collect::<Vec<_>>().join("_").to_lowercase()
        });
        let category = category_hint.unwrap_or_else(|| "system".to_string());

        Ok(json!({
            "skill_iri": format!("iri://skills/{}", name),
            "name": name,
            "description": description,
            "category": category,
            "registered": false,
            "note": "Gateway 未初始化，仅返回模板。请通过 SkillCreator API 创建完整 Skill。"
        }))
    }
}

fn execute_convert_skill(input: &Value) -> Result<Value, String> {
    let markdown_content = input["markdown_content"].as_str().unwrap_or("").to_string();
    if markdown_content.is_empty() {
        return Err("markdown_content is required".to_string());
    }
    let source_path = input["source_path"].as_str().map(String::from);

    if let Some(gateway) = SKILL_CREATOR_GATEWAY.get() {
        let rt = tokio::runtime::Handle::current();
        let graph_store = std::sync::Arc::new(crate::skill_graph::SkillGraphStore::new());
        let registry = std::sync::Arc::new(crate::tools::SkillRegistry::new());
        let config = crate::skill_graph::SkillCreatorConfig::default();
        let creator = crate::skill_graph::SkillCreator::new(
            gateway.clone(), graph_store, registry, config,
        );

        let request = crate::skill_graph::ConvertMarkdownRequest {
            markdown_content,
            source_path,
        };

        let result = rt.block_on(async { creator.convert_from_markdown(request).await })
            .map_err(|e| format!("转换 Skill 失败: {:?}", e))?;

        Ok(json!({
            "skill_iri": result.skill_iri,
            "name": result.name,
            "registered": true,
            "json_ld": result.json_ld,
        }))
    } else {
        let def = crate::skill_graph::SkillCreator::convert_markdown_static(&markdown_content)
            .map_err(|e| format!("静态解析失败: {:?}", e))?;

        Ok(json!({
            "skill_iri": format!("iri://skills/{}", def.name),
            "name": def.name,
            "description": def.description,
            "steps": def.steps.len(),
            "tags": def.tags,
            "registered": false,
            "note": "Gateway 未初始化，使用静态解析。完整转换需通过 SkillCreator API。"
        }))
    }
}

// ========== 知识图谱工具实现 ==========

fn execute_knowledge_extract(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let text = input["text"].as_str().unwrap_or("").to_string();
    if text.is_empty() {
        return Err("text 参数不能为空".to_string());
    }
    let domain = input["domain"].as_str().map(String::from);

    let api_url = std::env::var("ONE_API_URL")
        .or_else(|_| std::env::var("OPENAI_API_BASE"))
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let api_key = std::env::var("ONE_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| "未配置 API 密钥: 请设置 ONE_API_KEY 或 OPENAI_API_KEY 环境变量".to_string())?;
    let model = std::env::var("KG_EXTRACT_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let ontology = OntologyManager::new();
    let temp_store = KnowledgeGraphStore::new()
        .map_err(|e| format!("创建临时存储失败: {}", e))?;
    let extractor = KnowledgeExtractor::new(
        ontology,
        temp_store,
        api_url,
        api_key,
        model,
    );

    let result = extractor.extract(&text, domain.as_deref())?;

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
    let graph = store.default_graph();
    store.write_quads(&result.quads, graph)?;

    Ok(json!({
        "success": true,
        "entity_count": result.entity_count,
        "relation_count": result.relation_count,
        "quad_count": result.quads.len(),
        "graph": graph,
    }))
}

fn execute_knowledge_query(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let sparql = input["sparql"].as_str().unwrap_or("").to_string();
    if sparql.is_empty() {
        return Err("sparql 参数不能为空".to_string());
    }
    let named_graph = input["named_graph"].as_str().map(String::from);

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
    let results = store.query_sparql(&sparql, named_graph.as_deref())?;

    Ok(json!({
        "success": true,
        "results": results,
        "count": results.len(),
    }))
}

fn execute_knowledge_search(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let keyword = input["keyword"].as_str().unwrap_or("").to_string();
    if keyword.is_empty() {
        return Err("keyword 参数不能为空".to_string());
    }
    let entity_type = input["entity_type"].as_str().map(String::from);

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
    let results = store.search_entities(&keyword, entity_type.as_deref())?;

    Ok(json!({
        "success": true,
        "results": results,
        "count": results.len(),
        "keyword": keyword,
    }))
}

fn execute_knowledge_neighbors(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let entity_id = input["entity_id"].as_str().unwrap_or("").to_string();
    if entity_id.is_empty() {
        return Err("entity_id 参数不能为空".to_string());
    }
    let depth = input["depth"].as_u64().unwrap_or(1).min(3) as usize;

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
    let result = store.get_neighbors(&entity_id, depth)?;

    Ok(json!({
        "success": true,
        "result": result,
    }))
}

fn execute_knowledge_import_json(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let json_data_str = input["json_data"].as_str().unwrap_or("");
    if json_data_str.is_empty() {
        return Err("json_data 参数不能为空".to_string());
    }
    let mapping_config_str = input["mapping_config"].as_str().unwrap_or("");
    if mapping_config_str.is_empty() {
        return Err("mapping_config 参数不能为空".to_string());
    }

    let json_data: Value = serde_json::from_str(json_data_str)
        .map_err(|e| format!("json_data JSON 解析失败: {}", e))?;
    let mapping: Value = serde_json::from_str(mapping_config_str)
        .map_err(|e| format!("mapping_config JSON 解析失败: {}", e))?;

    let id_field = mapping["id_field"].as_str().unwrap_or("id");
    let type_field = mapping["type_field"].as_str().unwrap_or("type");
    let label_field = mapping["label_field"].as_str().unwrap_or("label");
    let desc_field = mapping["description_field"].as_str();

    let items = match json_data {
        Value::Array(arr) => arr,
        Value::Object(_) => vec![json_data],
        _ => return Err("json_data 必须是 JSON 对象或数组".to_string()),
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for item in &items {
        let id = item[id_field].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let node_type = item[type_field].as_str().unwrap_or("Concept").to_string();
        let label = item[label_field].as_str().unwrap_or(&id).to_string();
        let description = desc_field.and_then(|f| item[f].as_str()).map(String::from);

        let mut properties = std::collections::HashMap::new();
        if let Some(obj) = item.as_object() {
            for (key, value) in obj {
                if key != id_field && key != type_field && key != label_field {
                    if desc_field == Some(key.as_str()) { continue; }
                    properties.insert(key.clone(), value.clone());
                }
            }
        }

        nodes.push(NodeDef {
            id: id.clone(),
            node_type,
            label,
            description,
            properties,
        });

        if let Some(relations) = mapping["relations"].as_array() {
            for rel in relations {
                let field = rel["field"].as_str().unwrap_or("");
                let relation = rel["relation"].as_str().unwrap_or("relatedTo");
                let target_prefix = rel["target_prefix"].as_str().unwrap_or("");

                if let Some(target_val) = item[field].as_str() {
                    let target_id = if target_prefix.is_empty() {
                        target_val.to_string()
                    } else {
                        format!("{}{}", target_prefix.trim_end_matches('/'), target_val.trim_start_matches('/'))
                    };
                    if !target_id.is_empty() {
                        edges.push(EdgeDef {
                            source: id.clone(),
                            target: target_id,
                            relation: relation.to_string(),
                            properties: std::collections::HashMap::new(),
                        });
                    }
                }
            }
        }
    }

    if nodes.is_empty() {
        return Ok(json!({
            "success": true,
            "entity_count": 0,
            "relation_count": 0,
            "message": "未找到可导入的实体",
        }));
    }

    let graph = {
        let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
        store.default_graph().to_string()
    };

    let extraction = crate::tools::knowledge_graph::types::LLMExtractionOutput {
        nodes: nodes.clone(),
        edges: edges.clone(),
    };
    let result = RdfMapper::map_extraction(&extraction, &graph);

    {
        let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
        store.write_quads(&result.quads, &graph)?;
    }

    Ok(json!({
        "success": true,
        "entity_count": result.entity_count,
        "relation_count": result.relation_count,
        "quad_count": result.quads.len(),
        "graph": graph,
    }))
}

fn execute_ontology_register(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let terms = input["terms"].as_array().ok_or("terms 参数必须是数组")?;
    if terms.is_empty() {
        return Err("terms 数组不能为空".to_string());
    }

    let graph = "graph:ontology";
    let mut quads = Vec::new();

    for term in terms {
        let iri = term["iri"].as_str().unwrap_or("").to_string();
        let label = term["label"].as_str().unwrap_or("").to_string();
        let description = term["description"].as_str().unwrap_or("").to_string();
        let term_type = term["term_type"].as_str().unwrap_or("").to_string();

        if iri.is_empty() || label.is_empty() {
            continue;
        }

        let type_iri = match term_type.as_str() {
            "Class" => "http://www.w3.org/2000/01/rdf-schema#Class",
            "Property" => "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property",
            "Relation" => "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property",
            _ => "http://www.w3.org/2000/01/rdf-schema#Resource",
        };

        quads.push(RdfQuad {
            subject: iri.clone(),
            predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
            object: RdfValue::Iri(type_iri.to_string()),
            graph: Some(graph.to_string()),
        });
        quads.push(RdfQuad {
            subject: iri.clone(),
            predicate: "http://www.w3.org/2000/01/rdf-schema#label".to_string(),
            object: RdfValue::Literal(label),
            graph: Some(graph.to_string()),
        });
        if !description.is_empty() {
            quads.push(RdfQuad {
                subject: iri.clone(),
                predicate: "http://www.w3.org/2000/01/rdf-schema#comment".to_string(),
                object: RdfValue::Literal(description),
                graph: Some(graph.to_string()),
            });
        }
        quads.push(RdfQuad {
            subject: iri,
            predicate: "https://agentos.ontology/meta/termType".to_string(),
            object: RdfValue::Literal(term_type),
            graph: Some(graph.to_string()),
        });
    }

    let registered = quads.len() / 3;
    {
        let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
        store.write_quads(&quads, graph)?;
    }

    Ok(json!({
        "success": true,
        "registered_terms": registered,
        "graph": graph,
    }))
}

fn execute_knowledge_bridge_with_store(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let entity_id = input["entity_id"].as_str().unwrap_or("").to_string();
    if entity_id.is_empty() {
        return Err("entity_id 参数不能为空".to_string());
    }
    let skill_iri = input["skill_iri"].as_str().unwrap_or("").to_string();
    if skill_iri.is_empty() {
        return Err("skill_iri 参数不能为空".to_string());
    }
    let relation_type_str = input["relation_type"].as_str().unwrap_or("HasSkill");

    let relation = match relation_type_str {
        "HasSkill" => BridgeRelationType::HasSkill,
        "ApplicableIn" => BridgeRelationType::ApplicableIn,
        "RelatedTo" => BridgeRelationType::RelatedTo,
        _ => return Err(format!("不支持的关系类型: {}，可选: HasSkill, ApplicableIn, RelatedTo", relation_type_str)),
    };

    let entity_iri = format!("iri://entity/{}", entity_id);
    let predicate = match relation {
        BridgeRelationType::HasSkill => "https://agentos.ontology/bridge/hasSkill",
        BridgeRelationType::ApplicableIn => "https://agentos.ontology/bridge/applicableIn",
        BridgeRelationType::RelatedTo => "https://agentos.ontology/bridge/relatedTo",
    };

    let bridge_graph = "graph:bridge";
    let quad = RdfQuad {
        subject: entity_iri,
        predicate: predicate.to_string(),
        object: RdfValue::Iri(skill_iri.to_string()),
        graph: Some(bridge_graph.to_string()),
    };

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;
    store.write_quads(&[quad], bridge_graph)?;

    Ok(json!({
        "success": true,
        "entity_id": entity_id,
        "skill_iri": skill_iri,
        "relation_type": relation_type_str,
    }))
}

fn execute_knowledge_extract_code(input: &Value, kg_store: &Arc<Mutex<KnowledgeGraphStore>>) -> Result<Value, String> {
    let file_path = input["file_path"].as_str().unwrap_or("").to_string();
    if file_path.is_empty() {
        return Err("file_path 参数不能为空".to_string());
    }
    let graph = input["named_graph"].as_str().unwrap_or("graph:code").to_string();
    let force = input["force"].as_bool().unwrap_or(false);

    let store = kg_store.lock().map_err(|e| format!("获取存储锁失败: {}", e))?;

    if force {
        let result = CodeAstExtractor::extract_from_file(&file_path, &graph)?;
        store.delete_quads_by_subject_prefix(&format!("iri://entity/file:{}", file_path), &graph)?;
        store.write_quads(&result.quads, &graph)?;
        Ok(json!({
            "success": true,
            "file_path": file_path,
            "mode": "force",
            "entity_count": result.entity_count,
            "relation_count": result.relation_count,
            "quad_count": result.quads.len(),
            "graph": graph,
        }))
    } else {
        use crate::tools::knowledge_graph::code_ast::IncrementalResult;
        let result = CodeAstExtractor::extract_incremental(&file_path, &graph, &store)?;
        match result {
            IncrementalResult::Unchanged => Ok(json!({
                "success": true,
                "file_path": file_path,
                "mode": "incremental",
                "status": "unchanged",
                "message": "文件内容未变化，跳过 AST 提取",
            })),
            IncrementalResult::Created { entity_count, relation_count, quad_count } => Ok(json!({
                "success": true,
                "file_path": file_path,
                "mode": "incremental",
                "status": "created",
                "entity_count": entity_count,
                "relation_count": relation_count,
                "quad_count": quad_count,
                "graph": graph,
            })),
            IncrementalResult::Updated { entity_count, relation_count, quad_count, deleted_quads } => Ok(json!({
                "success": true,
                "file_path": file_path,
                "mode": "incremental",
                "status": "updated",
                "entity_count": entity_count,
                "relation_count": relation_count,
                "quad_count": quad_count,
                "deleted_quads": deleted_quads,
                "graph": graph,
            })),
        }
    }
}
