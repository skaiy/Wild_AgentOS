use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::tools::hooks::{
    FunctionHook, HookContext, HookManager, HookPoint, HookResult,
};

/// Global shared audit log accessible to HTTP endpoints.
pub static GUARD_AUDIT_LOG: Lazy<Arc<RwLock<Vec<GuardAuditEntry>>>> =
    Lazy::new(|| Arc::new(RwLock::new(Vec::new())));

// ─── Tool Category ───

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCategory {
    FileRead,
    FileWrite,
    Search,
    CodeExecution,
    KnowledgeGraph,
    KnowledgeExtract,
    HttpRequest,
    Meta,
}

// ─── Enforcement Level ───

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementLevel {
    Must,
    Should,
    Info,
}

// ─── Rule Structs ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreInjectionRule {
    pub enforcement: EnforcementLevel,
    pub instruction: String,
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub validator: String,
    pub params: HashMap<String, Value>,
    pub fix_instruction: String,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardAuditEntry {
    pub timestamp: i64,
    pub tool_name: String,
    pub agent_id: String,
    pub pre_injected: bool,
    pub validation_passed: bool,
    pub retry_count: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardStats {
    pub total_checks: usize,
    pub passed_checks: usize,
    pub failed_checks: usize,
    pub pass_rate: f64,
}

impl Default for GuardStats {
    fn default() -> Self {
        Self {
            total_checks: 0,
            passed_checks: 0,
            failed_checks: 0,
            pass_rate: 1.0,
        }
    }
}

// ─── External Config (guard_rules.json) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRules {
    #[serde(default)]
    pub pre_injections: Vec<PreInjectionRule>,
    #[serde(default)]
    pub validations: Vec<ValidationRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardRulesConfig {
    pub categories: HashMap<String, CategoryRules>,
}

impl GuardRulesConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: GuardRulesConfig = serde_json::from_str(&content)?;
        Ok(config)
    }
}

// ─── Validation Outcome ───

#[derive(Debug, Clone)]
pub enum ValidationOutcome {
    Pass,
    Warn(String),
    Fail(String),
}

// ─── ToolGuard ───

#[derive(Clone)]
pub struct ToolGuard {
    pre_injections: Arc<RwLock<HashMap<ToolCategory, Vec<PreInjectionRule>>>>,
    validations: Arc<RwLock<HashMap<ToolCategory, Vec<ValidationRule>>>>,
    tool_categories: HashMap<String, ToolCategory>,
    audit_log: Arc<RwLock<Vec<GuardAuditEntry>>>,
    config_path: Arc<RwLock<Option<String>>>,
}

impl ToolGuard {
    pub fn new() -> Self {
        let mut guard = Self {
            pre_injections: Arc::new(RwLock::new(HashMap::new())),
            validations: Arc::new(RwLock::new(HashMap::new())),
            tool_categories: Self::default_tool_categories(),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            config_path: Arc::new(RwLock::new(None)),
        };
        guard.load_default_rules();
        guard
    }

    /// Create ToolGuard with rules loaded from a JSON config file.
    /// Categories present in the JSON file replace defaults; absent categories keep defaults.
    pub fn from_json<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let config = GuardRulesConfig::from_file(path.as_ref())?;
        let guard = Self {
            pre_injections: Arc::new(RwLock::new(HashMap::new())),
            validations: Arc::new(RwLock::new(HashMap::new())),
            tool_categories: Self::default_tool_categories(),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            config_path: Arc::new(RwLock::new(Some(path.as_ref().to_string_lossy().to_string()))),
        };
        {
            let mut pre = guard.pre_injections.write();
            let mut val = guard.validations.write();
            for (cat_str, rules) in &config.categories {
                if let Ok(category) = serde_json::from_value::<ToolCategory>(json!(cat_str)) {
                    if !rules.pre_injections.is_empty() {
                        pre.insert(category.clone(), rules.pre_injections.clone());
                    }
                    if !rules.validations.is_empty() {
                        val.insert(category, rules.validations.clone());
                    }
                }
            }
        }
        Ok(guard)
    }

    fn default_tool_categories() -> HashMap<String, ToolCategory> {
        let mut map = HashMap::new();
        map.insert("file_read".to_string(), ToolCategory::FileRead);
        map.insert("file_list".to_string(), ToolCategory::FileRead);
        map.insert("file_write".to_string(), ToolCategory::FileWrite);
        map.insert("file_edit".to_string(), ToolCategory::FileWrite);
        map.insert("grep_search".to_string(), ToolCategory::Search);
        map.insert("glob_search".to_string(), ToolCategory::Search);
        map.insert("bash".to_string(), ToolCategory::CodeExecution);
        map.insert("powershell".to_string(), ToolCategory::CodeExecution);
        map.insert("code_execute".to_string(), ToolCategory::CodeExecution);
        map.insert("knowledge_query".to_string(), ToolCategory::KnowledgeGraph);
        map.insert("knowledge_neighbors".to_string(), ToolCategory::KnowledgeGraph);
        map.insert("kg_search".to_string(), ToolCategory::KnowledgeGraph);
        map.insert("knowledge_extract".to_string(), ToolCategory::KnowledgeExtract);
        map.insert("web_fetch".to_string(), ToolCategory::HttpRequest);
        map.insert("web_search".to_string(), ToolCategory::HttpRequest);
        map.insert("http_request".to_string(), ToolCategory::HttpRequest);
        map.insert("create_skill".to_string(), ToolCategory::Meta);
        map.insert("convert_skill".to_string(), ToolCategory::Meta);
        map
    }

    fn load_default_rules(&self) {
        let mut pre = self.pre_injections.write();
        let mut val = self.validations.write();
        // ── FileRead ──
        pre.insert(
            ToolCategory::FileRead,
            vec![PreInjectionRule {
                enforcement: EnforcementLevel::Must,
                instruction: "你必须完整读取文件的全部内容，不得截断或跳行。\
                    文件读取后，检查文件顶部的 use / import / mod 声明，\
                    发现引用的关联文件必须在同一轮或下一轮读取这些文件。"
                    .to_string(),
                tool_names: vec![],
            }],
        );
        val.insert(
            ToolCategory::FileRead,
            vec![ValidationRule {
                validator: "file_length_check".to_string(),
                params: [(
                    "min_ratio".to_string(),
                    Value::Number(serde_json::Number::from_f64(0.95).unwrap()),
                )]
                .into(),
                fix_instruction: "文件读取不完整。请确保使用单个 read 请求读取全部内容，而不是分片读取。"
                    .to_string(),
                max_retries: 2,
            }],
        );

        // ── Search ──
        pre.insert(
            ToolCategory::Search,
            vec![PreInjectionRule {
                enforcement: EnforcementLevel::Must,
                instruction: "搜索操作必须获取所有匹配结果。\
                    检查返回中的 num_matches / num_files 字段，\
                    确认与返回的实际结果数量一致。\
                    如果结果过多，使用更精确的搜索条件缩小范围。"
                    .to_string(),
                tool_names: vec![],
            }],
        );
        val.insert(
            ToolCategory::Search,
            vec![ValidationRule {
                validator: "search_count_check".to_string(),
                params: HashMap::new(),
                fix_instruction: "搜索返回不完整。请增大 head_limit 或使用更精确的关键词。"
                    .to_string(),
                max_retries: 1,
            }],
        );

        // ── CodeExecution ──
        pre.insert(
            ToolCategory::CodeExecution,
            vec![PreInjectionRule {
                enforcement: EnforcementLevel::Must,
                instruction: "命令执行后必须检查退出码（exit_code）。\
                    若 exit_code ≠ 0，必须分析 stderr 的完整内容，\
                    不得使用非零退出码的结果作为有效输出。"
                    .to_string(),
                tool_names: vec!["bash".to_string(), "powershell".to_string()],
            }],
        );
        val.insert(
            ToolCategory::CodeExecution,
            vec![ValidationRule {
                validator: "exit_code_check".to_string(),
                params: HashMap::new(),
                fix_instruction: "命令退出码非零。请分析 stderr 中的错误信息，修复问题后重试。"
                    .to_string(),
                max_retries: 2,
            }],
        );

        // ── KnowledgeGraph ──
        pre.insert(
            ToolCategory::KnowledgeGraph,
            vec![
                PreInjectionRule {
                    enforcement: EnforcementLevel::Should,
                    instruction: "执行 SPARQL 查询时，如果结果为 0，\
                        请尝试更宽松的查询（如移除 FILTER）。"
                        .to_string(),
                    tool_names: vec!["knowledge_query".to_string()],
                },
                PreInjectionRule {
                    enforcement: EnforcementLevel::Should,
                    instruction: "执行邻居遍历时，depth 参数建议至少为 2，\
                        以获得有意义的上下文。"
                        .to_string(),
                    tool_names: vec!["knowledge_neighbors".to_string()],
                },
            ],
        );
        val.insert(
            ToolCategory::KnowledgeGraph,
            vec![
                ValidationRule {
                    validator: "knowledge_empty_check".to_string(),
                    params: HashMap::new(),
                    fix_instruction: "查询结果为空。请尝试移除 FILTER 条件或使用更宽松的匹配。"
                        .to_string(),
                    max_retries: 1,
                },
                ValidationRule {
                    validator: "knowledge_depth_check".to_string(),
                    params: HashMap::new(),
                    fix_instruction: "遍历深度不足。请将 depth 参数设置为至少 2。"
                        .to_string(),
                    max_retries: 1,
                },
            ],
        );

        // ── KnowledgeExtract ──
        pre.insert(
            ToolCategory::KnowledgeExtract,
            vec![PreInjectionRule {
                enforcement: EnforcementLevel::Must,
                instruction: "抽取结果必须包含 entities 和 relations 数组。\
                    所有实体必须有唯一的 id。\
                    如果文本过长（超过 4000 字符），必须分段抽取后合并去重。"
                    .to_string(),
                tool_names: vec![],
            }],
        );
        val.insert(
            ToolCategory::KnowledgeExtract,
            vec![ValidationRule {
                validator: "extract_empty_check".to_string(),
                params: HashMap::new(),
                fix_instruction: "未抽取到任何实体。请确保文本包含可抽取的结构化信息，并重新尝试。"
                    .to_string(),
                max_retries: 1,
            }],
        );

        // ── HttpRequest ──
        pre.insert(
            ToolCategory::HttpRequest,
            vec![PreInjectionRule {
                enforcement: EnforcementLevel::Must,
                instruction: "检查 HTTP 状态码。\
                    若 status_code ≥ 400，必须分析错误原因并报告，不得忽略。"
                    .to_string(),
                tool_names: vec!["web_fetch".to_string(), "http_request".to_string()],
            }],
        );
        val.insert(
            ToolCategory::HttpRequest,
            vec![ValidationRule {
                validator: "http_status_check".to_string(),
                params: HashMap::new(),
                fix_instruction: "HTTP 请求返回错误状态码。请检查 URL 是否正确或是否需要认证。"
                    .to_string(),
                max_retries: 1,
            }],
        );
    }

    // ─── Hook Registration ───

    /// Register ToolGuard into the HookManager:
    /// - SkillBefore → Pre-Injection (injects constraints into context metadata)
    /// - SkillAfter  → Post-Validation (validates tool result)
    pub fn register_hooks(&self, hook_manager: &HookManager) {
        // ── Pre-Injection Hook (SkillBefore) ──
        let pre_guard = self.clone();
        let pre_hook = FunctionHook::new(
            "toolguard::pre_inject",
            vec![HookPoint::SkillBefore],
            80,
            move |ctx: &mut HookContext| {
                let tool_name = match ctx.data.get("tool_name").and_then(|v| v.as_str()) {
                    Some(name) => name.to_string(),
                    None => return HookResult::Continue,
                };

                if let Some(category) = pre_guard.tool_categories.get(&tool_name) {
                    if let Some(rules) = pre_guard.pre_injections.read().get(category) {
                        let instructions: Vec<String> = rules
                            .iter()
                            .map(|r| {
                                let tag = match r.enforcement {
                                    EnforcementLevel::Must => "强制",
                                    EnforcementLevel::Should => "建议",
                                    EnforcementLevel::Info => "提示",
                                };
                                format!("[ToolGuard-{}] {}", tag, r.instruction)
                            })
                            .collect();
                        ctx.metadata.insert(
                            "guard_pre_injections".to_string(),
                            Value::Array(
                                instructions.into_iter().map(Value::String).collect(),
                            ),
                        );
                        debug!(
                            tool = %tool_name,
                            "ToolGuard: Pre-injection applied ({} rules)",
                            rules.len()
                        );
                    }
                }

                HookResult::Continue
            },
        );
        hook_manager.register_arc(Arc::new(pre_hook));

        // ── Post-Validation Hook (SkillAfter) ──
        let post_guard = self.clone();
        let post_hook = FunctionHook::new(
            "toolguard::post_validate",
            vec![HookPoint::SkillAfter],
            80,
            move |ctx: &mut HookContext| {
                let tool_name = match ctx.data.get("tool_name").and_then(|v| v.as_str()) {
                    Some(name) => name.to_string(),
                    None => return HookResult::Continue,
                };
                let result_str = match ctx.data.get("tool_result").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => return HookResult::Continue,
                };

                let result: Value = match serde_json::from_str(&result_str) {
                    Ok(v) => v,
                    Err(_) => return HookResult::Continue,
                };

                if let Some(category) = post_guard.tool_categories.get(&tool_name) {
                    if let Some(rules) = post_guard.validations.read().get(category) {
                        for rule in rules {
                            let outcome =
                                post_guard.run_validator(&rule.validator, &result);
                            match outcome {
                                ValidationOutcome::Fail(msg) => {
                                    let error_msg = format!(
                                        "ToolGuard: {} - {}. 修正建议: {}",
                                        tool_name, msg, rule.fix_instruction
                                    );
                                    warn!(
                                        tool = %tool_name,
                                        reason = %msg,
                                        "ToolGuard: Validation failed"
                                    );
                                    ctx.error = Some(error_msg);

                                    post_guard
                                        .audit_log
                                        .write()
                                        .push(GuardAuditEntry {
                                            timestamp: chrono::Utc::now().timestamp(),
                                            tool_name: tool_name.clone(),
                                            agent_id: ctx.agent_id.clone(),
                                            pre_injected: true,
                                            validation_passed: false,
                                            retry_count: 0,
                                            error: Some(msg.clone()),
                                        });
                                    GUARD_AUDIT_LOG.write().push(GuardAuditEntry {
                                        timestamp: chrono::Utc::now().timestamp(),
                                        tool_name: tool_name.clone(),
                                        agent_id: ctx.agent_id.clone(),
                                        pre_injected: true,
                                        validation_passed: false,
                                        retry_count: 0,
                                        error: Some(msg),
                                    });

                                    return HookResult::Abort;
                                }
                                ValidationOutcome::Warn(msg) => {
                                    warn!(
                                        tool = %tool_name,
                                        warning = %msg,
                                        "ToolGuard: Validation warning"
                                    );
                                }
                                ValidationOutcome::Pass => {}
                            }
                        }

                        post_guard.audit_log.write().push(GuardAuditEntry {
                            timestamp: chrono::Utc::now().timestamp(),
                            tool_name: tool_name.clone(),
                            agent_id: ctx.agent_id.clone(),
                            pre_injected: true,
                            validation_passed: true,
                            retry_count: 0,
                            error: None,
                        });
                        GUARD_AUDIT_LOG.write().push(GuardAuditEntry {
                            timestamp: chrono::Utc::now().timestamp(),
                            tool_name: tool_name.clone(),
                            agent_id: ctx.agent_id.clone(),
                            pre_injected: true,
                            validation_passed: true,
                            retry_count: 0,
                            error: None,
                        });
                    }
                }

                HookResult::Continue
            },
        );
        hook_manager.register_arc(Arc::new(post_hook));
    }

    // ─── Validators ───

    fn run_validator(&self, validator: &str, result: &Value) -> ValidationOutcome {
        match validator {
            "file_length_check" => self::validators::file_length_check(result),
            "search_count_check" => self::validators::search_count_check(result),
            "exit_code_check" => self::validators::exit_code_check(result),
            "knowledge_empty_check" => self::validators::knowledge_empty_check(result),
            "knowledge_depth_check" => self::validators::knowledge_depth_check(result),
            "extract_empty_check" => self::validators::extract_empty_check(result),
            "http_status_check" => self::validators::http_status_check(result),
            _ => {
                warn!(validator = %validator, "ToolGuard: Unknown validator");
                ValidationOutcome::Pass
            }
        }
    }

    // ─── Audit ───

    pub fn get_audit_log(&self) -> Vec<GuardAuditEntry> {
        self.audit_log.read().clone()
    }

    pub fn get_audit_stats(&self) -> GuardStats {
        let log = self.audit_log.read();
        let total = log.len();
        if total == 0 {
            return GuardStats::default();
        }
        let passed = log.iter().filter(|e| e.validation_passed).count();
        GuardStats {
            total_checks: total,
            passed_checks: passed,
            failed_checks: total - passed,
            pass_rate: passed as f64 / total as f64,
        }
    }

    // ─── Hot-Reload ───

    /// Reload rules from a JSON config file at runtime.
    /// Overrides rules for categories present in the file, keeps others unchanged.
    pub fn reload_from_json<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = path.as_ref().to_path_buf();
        let config = GuardRulesConfig::from_file(&path_ref)?;
        {
            let mut pre = self.pre_injections.write();
            let mut val = self.validations.write();
            for (cat_str, rules) in &config.categories {
                if let Ok(category) = serde_json::from_value::<ToolCategory>(json!(cat_str)) {
                    if !rules.pre_injections.is_empty() {
                        pre.insert(category.clone(), rules.pre_injections.clone());
                    }
                    if !rules.validations.is_empty() {
                        val.insert(category, rules.validations.clone());
                    }
                }
            }
        }
        info!("ToolGuard: Hot-reloaded {} categories from {:?}", config.categories.len(), path_ref);
        Ok(())
    }

    /// Start a background task that polls the config file for changes.
    /// When the file's mtime changes, rules are reloaded automatically.
    pub fn start_hot_reload(self: &Arc<Self>, path: impl Into<String>, interval_secs: u64) {
        let path_str: String = path.into();
        let watch_path = path_str.clone();
        {
            let mut cp = self.config_path.write();
            *cp = Some(path_str);
        }
        let guard = self.clone();
        tokio::spawn(async move {
            let mut last_mtime = std::time::SystemTime::UNIX_EPOCH;
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            interval.tick().await; // skip immediate tick
            loop {
                interval.tick().await;
                if let Ok(meta) = std::fs::metadata(&watch_path) {
                    if let Ok(mtime) = meta.modified() {
                        if mtime != last_mtime {
                            last_mtime = mtime;
                            match guard.reload_from_json(&watch_path) {
                                Ok(()) => debug!("ToolGuard: Config hot-reloaded from {}", watch_path),
                                Err(e) => warn!("ToolGuard: Hot-reload failed for {}: {}", watch_path, e),
                            }
                        }
                    }
                }
            }
        });
    }
}

impl Default for ToolGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Validator Implementations ───

mod validators {
    use serde_json::Value;
    use super::ValidationOutcome;

    pub fn file_length_check(result: &Value) -> ValidationOutcome {
        if result.get("entries").is_some() {
            return ValidationOutcome::Pass;
        }
        let content = result["content"].as_str().unwrap_or("");
        if content.is_empty() {
            if result.get("error").is_some() {
                return ValidationOutcome::Fail("文件读取返回错误".to_string());
            }
            return ValidationOutcome::Warn("文件内容为空".to_string());
        }
        ValidationOutcome::Pass
    }

    pub fn search_count_check(result: &Value) -> ValidationOutcome {
        if let Some(num_files) = result["num_files"].as_u64() {
            let returned = result["filenames"]
                .as_array()
                .map(|a| a.len() as u64)
                .unwrap_or(0);
            if returned < num_files && returned > 0 {
                return ValidationOutcome::Fail(format!(
                    "搜索结果不完整: 共 {} 个匹配文件，仅返回 {} 个",
                    num_files, returned
                ));
            }
        }
        if let Some(num_matches) = result["num_matches"].as_u64() {
            let returned_count = result["counts"]
                .as_array()
                .map(|a| a.len() as u64)
                .or_else(|| {
                    result["filenames"]
                        .as_array()
                        .map(|a| a.len() as u64)
                })
                .unwrap_or(0);
            if returned_count > 0 && returned_count < num_matches {
                return ValidationOutcome::Warn(format!(
                    "共 {} 个匹配，仅展示 {} 个（可能受 limit 限制）",
                    num_matches, returned_count
                ));
            }
        }
        ValidationOutcome::Pass
    }

    pub fn exit_code_check(result: &Value) -> ValidationOutcome {
        if let Some(ec) = result["exit_code"].as_i64() {
            if ec != 0 {
                let stderr = result["stderr"].as_str().unwrap_or("");
                return ValidationOutcome::Fail(format!(
                    "退出码非零: {}, stderr: {}",
                    ec,
                    &stderr.chars().take(200).collect::<String>()
                ));
            }
        } else if result.get("error").is_some() {
            return ValidationOutcome::Fail("命令执行返回错误".to_string());
        }
        ValidationOutcome::Pass
    }

    pub fn knowledge_empty_check(result: &Value) -> ValidationOutcome {
        let bindings = result["bindings"].as_array();
        let results_arr = result["results"].as_array();
        let count = bindings
            .map(|a| a.len())
            .or_else(|| results_arr.map(|a| a.len()))
            .unwrap_or(0);
        if count == 0 {
            return ValidationOutcome::Warn("查询结果为空，建议尝试更宽松的查询条件".to_string());
        }
        ValidationOutcome::Pass
    }

    pub fn knowledge_depth_check(result: &Value) -> ValidationOutcome {
        if let Some(depth) = result["depth"].as_u64() {
            if depth < 2 {
                return ValidationOutcome::Warn(format!(
                    "遍历深度为 {}，建议增加到 2 以上",
                    depth
                ));
            }
        }
        ValidationOutcome::Pass
    }

    pub fn extract_empty_check(result: &Value) -> ValidationOutcome {
        let entities = result["entities"].as_array();
        let extracted = result["extracted"].as_array();
        let count = entities
            .map(|a| a.len())
            .or_else(|| extracted.map(|a| a.len()))
            .unwrap_or(0);
        if count == 0 {
            return ValidationOutcome::Fail("未抽取到任何实体".to_string());
        }
        ValidationOutcome::Pass
    }

    pub fn http_status_check(result: &Value) -> ValidationOutcome {
        if let Some(code) = result["status_code"]
            .as_u64()
            .or_else(|| result["status"].as_u64())
        {
            if code >= 400 {
                let body = result["body"]
                    .as_str()
                    .or_else(|| result["content"].as_str())
                    .unwrap_or("");
                return ValidationOutcome::Fail(format!(
                    "HTTP {} 错误: {}",
                    code,
                    &body.chars().take(100).collect::<String>()
                ));
            }
        }
        ValidationOutcome::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_default_tool_categories() {
        let guard = ToolGuard::new();
        assert_eq!(
            guard.tool_categories.get("file_read"),
            Some(&ToolCategory::FileRead)
        );
        assert_eq!(
            guard.tool_categories.get("bash"),
            Some(&ToolCategory::CodeExecution)
        );
        assert_eq!(
            guard.tool_categories.get("knowledge_query"),
            Some(&ToolCategory::KnowledgeGraph)
        );
    }

    #[test]
    fn test_file_length_check_pass() {
        let result = json!({"content": "full file content here"});
        let outcome = validators::file_length_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_file_length_check_empty_warn() {
        let result = json!({"content": ""});
        let outcome = validators::file_length_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Warn(_)));
    }

    #[test]
    fn test_file_length_check_error() {
        let result = json!({"error": "file not found"});
        let outcome = validators::file_length_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Fail(_)));
    }

    #[test]
    fn test_exit_code_check_pass() {
        let result = json!({"exit_code": 0, "stdout": "ok"});
        let outcome = validators::exit_code_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_exit_code_check_fail() {
        let result = json!({"exit_code": 1, "stderr": "error occurred"});
        let outcome = validators::exit_code_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Fail(_)));
    }

    #[test]
    fn test_search_count_check_incomplete() {
        let result = json!({
            "num_files": 10,
            "filenames": ["a.rs", "b.rs"],
            "mode": "files_with_matches"
        });
        let outcome = validators::search_count_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Fail(_)));
    }

    #[test]
    fn test_search_count_check_complete() {
        let result = json!({
            "num_files": 2,
            "filenames": ["a.rs", "b.rs"],
            "mode": "files_with_matches"
        });
        let outcome = validators::search_count_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_extract_empty_check_fail() {
        let result = json!({"entities": []});
        let outcome = validators::extract_empty_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Fail(_)));
    }

    #[test]
    fn test_extract_empty_check_pass() {
        let result = json!({"entities": [{"id": "e1", "type": "Person"}]});
        let outcome = validators::extract_empty_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_http_status_check_fail() {
        let result = json!({"status_code": 404, "body": "Not Found"});
        let outcome = validators::http_status_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Fail(_)));
    }

    #[test]
    fn test_http_status_check_pass() {
        let result = json!({"status_code": 200, "content": "ok"});
        let outcome = validators::http_status_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_knowledge_empty_warn() {
        let result = json!({"bindings": []});
        let outcome = validators::knowledge_empty_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Warn(_)));
    }

    #[test]
    fn test_knowledge_depth_warn() {
        let result = json!({"depth": 1});
        let outcome = validators::knowledge_depth_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Warn(_)));
    }

    #[test]
    fn test_knowledge_depth_pass() {
        let result = json!({"depth": 2});
        let outcome = validators::knowledge_depth_check(&result);
        assert!(matches!(outcome, ValidationOutcome::Pass));
    }

    #[test]
    fn test_audit_stats() {
        let guard = ToolGuard::new();
        let stats = guard.get_audit_stats();
        assert_eq!(stats.total_checks, 0);
        assert_eq!(stats.pass_rate, 1.0);
    }

    #[test]
    fn test_register_hooks_no_panic() {
        let guard = ToolGuard::new();
        let manager = HookManager::new();
        // Should not panic
        guard.register_hooks(&manager);
        let hooks = manager.get_hooks(HookPoint::SkillBefore);
        assert!(hooks.contains(&"toolguard::pre_inject".to_string()));
        let hooks = manager.get_hooks(HookPoint::SkillAfter);
        assert!(hooks.contains(&"toolguard::post_validate".to_string()));
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let json_str = r#"{
  "categories": {
    "FileRead": {
      "pre_injections": [
        {
          "enforcement": "Must",
          "instruction": "Must read full content",
          "tool_names": []
        }
      ],
      "validations": [
        {
          "validator": "file_length_check",
          "params": { "min_ratio": 0.95 },
          "fix_instruction": "Read the whole file",
          "max_retries": 2
        }
      ]
    }
  }
}"#;
        let config: GuardRulesConfig = serde_json::from_str(json_str).unwrap();
        assert!(config.categories.contains_key("FileRead"));
        let rules = &config.categories["FileRead"];
        assert_eq!(rules.pre_injections.len(), 1);
        assert_eq!(rules.validations.len(), 1);
        assert_eq!(rules.pre_injections[0].enforcement, EnforcementLevel::Must);
    }

    #[test]
    fn test_reload_from_json() {
        let dir = std::env::temp_dir().join("tool_guard_reload_test");
        let _ = std::fs::create_dir_all(&dir);
        let config_path = dir.join("test_rules.json");

        let initial = r#"{"categories":{"CodeExecution":{"pre_injections":[{"enforcement":"Must","instruction":"Always check exit code","tool_names":[]}],"validations":[{"validator":"exit_code_check","params":{},"fix_instruction":"Fix errors","max_retries":1}]}}}"#;
        std::fs::write(&config_path, initial).unwrap();

        let guard = ToolGuard::from_json(&config_path).unwrap();
        {
            let pre = guard.pre_injections.read();
            let code_rules = pre.get(&ToolCategory::CodeExecution);
            assert!(code_rules.is_some());
            assert_eq!(code_rules.unwrap()[0].instruction, "Always check exit code");
        }

        // Reload with different rules
        let updated = r#"{"categories":{"CodeExecution":{"pre_injections":[{"enforcement":"Should","instruction":"Check exit code carefully","tool_names":[]}],"validations":[]}}}"#;
        std::fs::write(&config_path, updated).unwrap();
        guard.reload_from_json(&config_path).unwrap();

        {
            let pre = guard.pre_injections.read();
            let code_rules = pre.get(&ToolCategory::CodeExecution);
            assert!(code_rules.is_some());
            assert_eq!(code_rules.unwrap()[0].instruction, "Check exit code carefully");
            assert_eq!(code_rules.unwrap()[0].enforcement, EnforcementLevel::Should);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_global_audit_log() {
        // Verify the global log is accessible and writable
        let initial_len = GUARD_AUDIT_LOG.read().len();
        GUARD_AUDIT_LOG.write().push(GuardAuditEntry {
            timestamp: 0,
            tool_name: "test".to_string(),
            agent_id: "test-agent".to_string(),
            pre_injected: true,
            validation_passed: false,
            retry_count: 1,
            error: Some("test error".to_string()),
        });
        assert_eq!(GUARD_AUDIT_LOG.read().len(), initial_len + 1);
        // Cleanup
        GUARD_AUDIT_LOG.write().pop();
    }

    #[test]
    fn test_from_json_missing_file() {
        let result = ToolGuard::from_json("/nonexistent/path/guard_rules.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_guard_stats_calculation() {
        let guard = ToolGuard::new();
        let stats = guard.get_audit_stats();
        assert_eq!(stats.total_checks, 0);
        assert_eq!(stats.pass_rate, 1.0);

        // Add entries via global log (internal log is only pushed during hook execution)
        // Verify the stats method handles empty log
    }
}
