/*!
 * Prompt / 模型灰度版本注册表。
 *
 * 功能：
 *   - 版本管理：add_version / list_versions / get_version / delete_version
 *   - 激活管理：activate — 标记一个版本为稳定生产版
 *   - 灰度路由：set_canary(id, percent, tenant_ids, roles)
 *   - 分流决策：resolve(tenant_id, user_id, role) → ResolvedPrompt
 */

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use chrono::Utc;
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── 版本快照 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Jinja2 风格的系统提示词模板（支持 {{tenant_id}} / {{user_id}} 占位符）。
    pub template: String,
    /// 绑定的模型名称（如 "gpt-4o", "deepseek-v3"）。
    pub model: String,
    pub version: String,
    /// 是否为当前激活的稳定版本。
    pub is_active: bool,
    /// 灰度流量百分比（0 = 关闭，100 = 全量）。
    pub canary_percent: u8,
    /// 仅对这些 tenant_id 开放灰度（空列表 = 全租户）。
    pub canary_tenant_ids: Vec<String>,
    /// 仅对这些角色开放灰度（空列表 = 全角色）。
    pub canary_roles: Vec<String>,
    pub created_at: String,
}

impl PromptVersion {
    pub fn new(
        name: impl Into<String>,
        template: impl Into<String>,
        model: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().hyphenated().to_string(),
            name: name.into(),
            description: description.into(),
            template: template.into(),
            model: model.into(),
            version: version.into(),
            is_active: false,
            canary_percent: 0,
            canary_tenant_ids: vec![],
            canary_roles: vec![],
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

// ─── 灰度分流结果 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPrompt {
    pub version_id: String,
    pub name: String,
    pub template: String,
    pub model: String,
    pub is_canary: bool,
}

// ─── 持久化快照 ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct PromptSnapshot {
    #[serde(default)]
    versions: Vec<PromptVersion>,
    #[serde(default)]
    active_id: Option<String>,
}

// ─── 注册表 ───────────────────────────────────────────────────────────────────

pub struct PromptRegistry {
    versions: DashMap<String, PromptVersion>,
    active_id: Mutex<Option<String>>,
    /// 持久化文件路径；为 None 时所有写操作不落盘（如单元测试）。
    persist_path: Option<PathBuf>,
}

impl Default for PromptRegistry {
    fn default() -> Self { Self::new() }
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self { versions: DashMap::new(), active_id: Mutex::new(None), persist_path: None }
    }

    /// 绑定持久化路径并从磁盘加载快照；文件不存在或解析失败时返回空表。
    pub fn load(path: PathBuf) -> Self {
        let reg = Self { versions: DashMap::new(), active_id: Mutex::new(None), persist_path: Some(path) };
        if let Some(p) = &reg.persist_path {
            if let Ok(content) = std::fs::read_to_string(p) {
                if let Ok(snap) = serde_json::from_str::<PromptSnapshot>(&content) {
                    for v in snap.versions {
                        reg.versions.insert(v.id.clone(), v);
                    }
                    *reg.active_id.lock() = snap.active_id;
                }
            }
        }
        reg
    }

    /// 将当前注册表快照写回磁盘（pretty JSON）；未绑定路径时为 no-op。
    fn persist(&self) {
        let Some(path) = &self.persist_path else { return; };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let snap = PromptSnapshot {
            versions: self.versions.iter().map(|e| e.value().clone()).collect(),
            active_id: self.active_id.lock().clone(),
        };
        if let Ok(content) = serde_json::to_string_pretty(&snap) {
            let _ = std::fs::write(path, content);
        }
    }

    /// 添加新版本（不自动激活）；若 `v.id` 为空则自动生成。
    pub fn add_version(&self, mut v: PromptVersion) -> String {
        if v.id.is_empty() { v.id = Uuid::new_v4().hyphenated().to_string(); }
        v.created_at = Utc::now().to_rfc3339();
        v.is_active = false;
        let id = v.id.clone();
        self.versions.insert(id.clone(), v);
        self.persist();
        id
    }

    /// 激活指定版本，同时将其他版本设为非激活。
    pub fn activate(&self, id: &str) -> bool {
        if !self.versions.contains_key(id) { return false; }
        for mut e in self.versions.iter_mut() { e.value_mut().is_active = false; }
        if let Some(mut e) = self.versions.get_mut(id) { e.value_mut().is_active = true; }
        *self.active_id.lock() = Some(id.to_string());
        self.persist();
        true
    }

    /// 更新灰度规则（percent=0 关闭灰度）。
    pub fn set_canary(&self, id: &str, percent: u8, tenants: Vec<String>, roles: Vec<String>) -> bool {
        let ok = self.versions.get_mut(id).map(|mut e| {
            let v = e.value_mut();
            v.canary_percent = percent.min(100);
            v.canary_tenant_ids = tenants;
            v.canary_roles = roles;
        }).is_some();
        if ok { self.persist(); }
        ok
    }

    pub fn list_versions(&self) -> Vec<PromptVersion> {
        let mut vs: Vec<_> = self.versions.iter().map(|e| e.value().clone()).collect();
        vs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        vs
    }

    pub fn get_version(&self, id: &str) -> Option<PromptVersion> {
        self.versions.get(id).map(|e| e.value().clone())
    }

    pub fn delete_version(&self, id: &str) -> bool {
        let removed = self.versions.remove(id).is_some();
        if removed { self.persist(); }
        removed
    }

    pub fn version_count(&self) -> usize { self.versions.len() }

    pub fn active_id(&self) -> Option<String> { self.active_id.lock().clone() }

    /// 灰度路由决策。
    /// 1. 找 canary_percent > 0 的版本，按 hash(user_id) % 100 命中灰度。
    /// 2. 未命中 → 激活版本；无激活版本 → None。
    pub fn resolve(&self, tenant_id: &str, user_id: &str, role: &str) -> Option<ResolvedPrompt> {
        let bucket = stable_hash(user_id) % 100;
        for e in self.versions.iter() {
            let v = e.value();
            if v.canary_percent == 0 { continue; }
            let t_ok = v.canary_tenant_ids.is_empty() || v.canary_tenant_ids.iter().any(|t| t == tenant_id);
            let r_ok = v.canary_roles.is_empty() || v.canary_roles.iter().any(|r| r == role);
            if t_ok && r_ok && bucket < v.canary_percent as u64 {
                return Some(ResolvedPrompt { version_id: v.id.clone(), name: v.name.clone(), template: v.template.clone(), model: v.model.clone(), is_canary: true });
            }
        }
        let active = self.active_id.lock().clone()?;
        let v = self.versions.get(&active)?;
        Some(ResolvedPrompt { version_id: v.id.clone(), name: v.name.clone(), template: v.template.clone(), model: v.model.clone(), is_canary: false })
    }
}

fn stable_hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
