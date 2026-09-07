//! Desktop capabilities injected into the HTTP server; no Tauri dependency.
use crate::wire::ApiError;
use serde_json::Value;
use std::path::{Path, PathBuf};
pub const MAX_UPLOAD_BYTES: usize = 500 * 1024 * 1024;
/// Owns a server-generated temporary file. Never constructed from a request path.
#[derive(Debug)]
pub struct Upload {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}
impl Upload {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for Upload {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
#[async_trait::async_trait]
pub trait Runtime: Send + Sync {
    async fn transcribe(&self, upload: Upload) -> Result<Value, ApiError>;
    async fn cancel(&self, id: &str) -> Result<Value, ApiError>;
    async fn recording_start(&self, provider: &str, system_audio: bool) -> Result<Value, ApiError>;
    async fn recording_stop(&self) -> Result<Value, ApiError>;
    async fn status(&self) -> Result<Value, ApiError>;
    async fn models(&self) -> Result<Value, ApiError>;
}
