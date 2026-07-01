use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::Value;

use crate::templates::template_engine::TemplateEngine;
use crate::utils::data_paths::{resolve_project_subpath, resolve_user_subpath};

fn builtin_fallback(role: &str) -> &'static str {
    match role {
        "pa" => "你是计划 Agent(PA)。分析任务并制定执行计划。完成后输出 JSON 格式结果。",
        "da" => "你是执行 Agent(DA)。执行任务，创建产物。优先使用 web_search 获取最新信息。完成后输出 JSON 格式结果。",
        "ca" => "你是检查 Agent(CA)。验证执行结果是否满足要求。完成后输出 JSON 格式结果。",
        "aa" => "你是决策 Agent(AA)。基于审计结果做最终决策和总结。完成后输出 JSON 格式结果。",
        _ => "",
    }
}

#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub user_prefix: Option<String>,
    pub role_overrides: HashMap<String, String>,
    pub env_refs: Vec<String>,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self { user_prefix: None, role_overrides: HashMap::new(), env_refs: Vec::new() }
    }
}

pub struct PromptLoader {
    config: PromptConfig,
    engine: Arc<TemplateEngine>,
    /// (content, mtime) 缓存，按文件路径索引
    file_cache: std::sync::Mutex<HashMap<PathBuf, (String, SystemTime)>>,
}

impl PromptLoader {
    pub fn new(config: PromptConfig, engine: Arc<TemplateEngine>) -> Self {
        Self { config, engine, file_cache: std::sync::Mutex::new(HashMap::new()) }
    }

    /// 带 mtime 检测的文件读取缓存。文件未变更时跳过 IO。
    fn read_cached(&self, path: &PathBuf) -> Option<String> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;

        if let Ok(cache) = self.file_cache.lock() {
            if let Some((cached_content, cached_mtime)) = cache.get(path) {
                if *cached_mtime == modified {
                    return Some(cached_content.clone());
                }
            }
        }

        let content = std::fs::read_to_string(path).ok()?;
        if let Ok(mut cache) = self.file_cache.lock() {
            cache.insert(path.clone(), (content.clone(), modified));
        }
        Some(content)
    }

    pub fn load(&self, role: &str, template: &str, vars: &HashMap<String, Value>) -> String {
        let fname = format!("{}/{}.md", role, template);

        if let Some(path) = resolve_user_subpath(&format!("prompts/{fname}")) {
            if let Some(content) = self.read_cached(&path) {
                return self.post_process(role, &Self::render_string(&content, vars));
            }
        }

        if let Some(proj_path) = resolve_project_subpath(&format!("prompts/{fname}")) {
            if let Some(content) = self.read_cached(&proj_path) {
                return self.post_process(role, &Self::render_string(&content, vars));
            }
        }

        if let Ok(content) = self.engine.render_prompt(role, template, vars, false, None) {
            return self.post_process(role, &content);
        }

        let fallback = builtin_fallback(role);
        self.post_process(role, &Self::render_string(fallback, vars))
    }

    fn post_process(&self, role: &str, content: &str) -> String {
        let mut result = content.to_string();

        for ref_name in &self.config.env_refs {
            if let Ok(val) = std::env::var(ref_name) {
                result = result.replace(&format!("${{{}}}", ref_name), &val);
            }
        }

        if let Some(ref prefix) = self.config.user_prefix {
            result = format!("{}\n\n{}", prefix, result);
        }

        if let Some(ref override_content) = self.config.role_overrides.get(role) {
            result = format!("{}\n\n---\n\n## 用户追加约束\n{}", result, override_content);
        }

        result
    }

    pub fn render_string(template: &str, variables: &HashMap<String, Value>) -> String {
        let mut result = template.to_string();
        for (key, value) in variables {
            let placeholder = format!("{{{}}}", key);
            let replacement = match value {
                Value::String(s) => s.clone(),
                Value::Object(_) | Value::Array(_) => serde_json::to_string_pretty(value).unwrap_or_default(),
                Value::Null => String::new(),
                _ => value.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
        result
    }
}
