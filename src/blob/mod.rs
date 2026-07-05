//! Blob 对象存储抽象（原文持久化）。
//!
//! 为知识库上传的原始文件提供落盘/对象存储能力，是「重建索引 / 原文预览 / 溯源」的前置。
//! 两个实现：`MinioBlobStore`（S3 兼容，生产默认）与 `LocalFsBlobStore`（PVC 兜底）。
//! 由 `open_blob_store()` 依环境变量选择后端。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::CoreError;

mod minio;
pub use minio::MinioBlobStore;

/// 原文对象存储接口。key 形如 `tenant:default/kb/<kbid>/<sha256>`。
#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), CoreError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, CoreError>;
    async fn delete(&self, key: &str) -> Result<(), CoreError>;
    async fn exists(&self, key: &str) -> Result<bool, CoreError>;
    /// 后端标识（写入文档台账 blob_ref.backend）。
    fn backend(&self) -> &'static str;
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

/// 依环境变量构造 BlobStore。
/// `BLOB_BACKEND`=minio|local；缺省时：设了 `MINIO_ENDPOINT` 走 minio，否则 local。
pub fn open_blob_store() -> Option<Arc<dyn BlobStore>> {
    let backend = env_nonempty("BLOB_BACKEND").unwrap_or_else(|| {
        if env_nonempty("MINIO_ENDPOINT").is_some() {
            "minio".into()
        } else {
            "local".into()
        }
    });
    match backend.as_str() {
        "minio" => match MinioBlobStore::from_env() {
            Ok(s) => {
                tracing::info!(bucket = %s.bucket_name(), "BlobStore: MinIO 已接入");
                Some(Arc::new(s))
            }
            Err(e) => {
                tracing::warn!("BlobStore: MinIO 初始化失败({e})，回退 LocalFs");
                Some(Arc::new(LocalFsBlobStore::from_env()))
            }
        },
        _ => {
            let s = LocalFsBlobStore::from_env();
            tracing::info!(root = %s.root_display(), "BlobStore: LocalFs 已接入");
            Some(Arc::new(s))
        }
    }
}

// ── LocalFs 实现 ──────────────────────────────────────────────────────────────

/// 本地文件系统实现（生产可落在 waos-data PVC 的 `data/blobs`）。
pub struct LocalFsBlobStore {
    root: PathBuf,
}

impl LocalFsBlobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn from_env() -> Self {
        let root = env_nonempty("BLOB_LOCAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::api::http::data_dir().join("blobs"));
        Self { root }
    }
    fn root_display(&self) -> String {
        self.root.display().to_string()
    }
    /// 拒绝路径穿越，返回 root 下的安全绝对路径。
    fn safe_path(&self, key: &str) -> Result<PathBuf, CoreError> {
        if key.is_empty() || key.split('/').any(|c| c == ".." || c == ".") {
            return Err(CoreError::Internal { message: format!("非法 blob key: {key}") });
        }
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl BlobStore for LocalFsBlobStore {
    async fn put(&self, key: &str, bytes: &[u8], _content_type: &str) -> Result<(), CoreError> {
        let path = self.safe_path(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| CoreError::Internal {
                message: format!("blob mkdir: {e}"),
            })?;
        }
        tokio::fs::write(&path, bytes).await.map_err(|e| CoreError::Internal {
            message: format!("blob write: {e}"),
        })
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>, CoreError> {
        let path = self.safe_path(key)?;
        tokio::fs::read(&path).await.map_err(|e| CoreError::Internal {
            message: format!("blob read: {e}"),
        })
    }
    async fn delete(&self, key: &str) -> Result<(), CoreError> {
        let path = self.safe_path(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::Internal { message: format!("blob delete: {e}") }),
        }
    }
    async fn exists(&self, key: &str) -> Result<bool, CoreError> {
        let path = self.safe_path(key)?;
        Ok(tokio::fs::metadata(&path).await.is_ok())
    }
    fn backend(&self) -> &'static str {
        "local"
    }
}
