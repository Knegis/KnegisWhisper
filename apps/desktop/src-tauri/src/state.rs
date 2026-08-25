//! Application state, and the one place the database and credential store are
//! constructed.
//!
//! Held in Tauri's managed state so commands receive it rather than reaching for
//! a global. That is not ceremony: it is what makes the credential store
//! substitutable, and it means nothing can quietly open a second connection pool
//! to the same SQLite file.

use std::sync::Arc;

use kw_security::{CredentialStore, OsCredentialStore};
use kw_storage::Db;

/// Everything a command might need.
#[derive(Clone)]
pub struct AppState {
    db: Db,
    credentials: Arc<dyn CredentialStore>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // No credential store internals, no connection string.
        f.write_str("AppState { db: <open>, credentials: <os> }")
    }
}

impl AppState {
    /// Open the database at its usual location and bring it up to date.
    pub async fn open() -> Result<Self, kw_storage::StorageError> {
        let path = kw_storage::paths::database_path();
        tracing::info!(path = %path.display(), "opening the database");
        let db = Db::open(&path).await?;
        Ok(Self {
            db,
            credentials: Arc::new(OsCredentialStore::new()),
        })
    }

    /// For tests: an in-memory database and an in-memory credential store, so a
    /// test never touches the developer's real keychain.
    #[cfg(test)]
    pub async fn in_memory() -> Result<Self, kw_storage::StorageError> {
        Ok(Self {
            db: Db::open_in_memory().await?,
            credentials: Arc::new(kw_security::MemoryCredentialStore::new()),
        })
    }

    #[must_use]
    pub const fn db(&self) -> &Db {
        &self.db
    }

    #[must_use]
    pub fn credentials(&self) -> &dyn CredentialStore {
        self.credentials.as_ref()
    }
}
