//! Application settings.
//!
//! A JSON value per dotted key. Upstream keeps these in electron-store, which
//! has no equivalent here, so this table is new rather than ported.
//!
//! [`set`] refuses keys that look like credentials. That is not defence against
//! an attacker — it is defence against the ordinary mistake of adding
//! `providers.openai.apiKey` to a settings screen, which would put a key in
//! SQLite where the whole design says it must not be.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::{Row as _, SqlitePool};

use crate::StorageError;

/// Substrings that make a settings key illegal.
const REJECTED_KEY_MARKERS: [&str; 8] = [
    "apikey",
    "api_key",
    "secret",
    "password",
    "token",
    "credential",
    "passphrase",
    "bearer",
];

/// Reasons a settings key can be rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("settings key is empty")]
    Empty,

    #[error(
        "settings key {key:?} contains {marker:?}, which suggests a credential. \
         Credentials go in the OS credential store; store a key reference here instead."
    )]
    LooksLikeCredential { key: String, marker: &'static str },
}

/// Check a settings key before it is used.
pub fn check_key(key: &str) -> Result<(), KeyError> {
    if key.trim().is_empty() {
        return Err(KeyError::Empty);
    }
    let lower = key.to_lowercase();
    // `keyRef` / `key_ref` are the legitimate way to point at a credential.
    let sanitised = lower.replace("keyref", "").replace("key_ref", "");
    for marker in REJECTED_KEY_MARKERS {
        if sanitised.contains(marker) {
            return Err(KeyError::LooksLikeCredential {
                key: key.to_owned(),
                marker,
            });
        }
    }
    Ok(())
}

/// Read a setting, deserialising it into `T`.
///
/// Returns `Ok(None)` both when the key is absent and when the stored JSON no
/// longer fits `T` — a setting whose shape changed should fall back to the
/// default rather than making the application refuse to start. The mismatch is
/// logged.
pub async fn get<T: DeserializeOwned>(
    pool: &SqlitePool,
    key: &str,
) -> Result<Option<T>, StorageError> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else { return Ok(None) };
    let raw: String = row.try_get("value")?;

    match serde_json::from_str::<T>(&raw) {
        Ok(value) => Ok(Some(value)),
        Err(err) => {
            // The value is not logged: a setting can legitimately contain
            // user text, and this crate does not decide what is safe to print.
            tracing::warn!(key, error = %err, "stored setting does not fit its type; using the default");
            Ok(None)
        }
    }
}

/// Read a setting, or `default` if absent or unparseable.
pub async fn get_or<T: DeserializeOwned>(
    pool: &SqlitePool,
    key: &str,
    default: T,
) -> Result<T, StorageError> {
    Ok(get(pool, key).await?.unwrap_or(default))
}

/// Write a setting.
pub async fn set<T: Serialize>(
    pool: &SqlitePool,
    key: &str,
    value: &T,
) -> Result<(), StorageError> {
    set_on(pool, key, value).await
}

/// Write using a pool or a transaction's SQLite connection. The same key and
/// serialization checks apply inside transactions, so callers cannot bypass
/// the credential boundary by choosing a different executor.
pub async fn set_on<'e, T: Serialize, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    key: &str,
    value: &T,
) -> Result<(), StorageError> {
    check_key(key).map_err(|e| StorageError::Import(e.to_string()))?;

    let encoded = serde_json::to_string(value)
        .map_err(|e| StorageError::Import(format!("encode {key}: {e}")))?;

    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(key)
    .bind(encoded)
    .execute(executor)
    .await?;

    Ok(())
}

/// Remove a setting. Absent keys are not an error.
pub async fn remove(pool: &SqlitePool, key: &str) -> Result<(), StorageError> {
    remove_on(pool, key).await
}

/// Remove using a pool or a transaction connection; absent keys remain a no-op.
pub async fn remove_on<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    key: &str,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(executor)
        .await?;
    Ok(())
}

/// Every key currently stored, sorted. Values are not returned: a caller that
/// wants one should ask for it by name and by type.
pub async fn keys(pool: &SqlitePool) -> Result<Vec<String>, StorageError> {
    let rows = sqlx::query("SELECT key FROM settings ORDER BY key")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| r.try_get("key").map_err(StorageError::from))
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn round_trips_a_scalar_and_a_struct() {
        let db = db().await;
        let p = db.pool();

        set(p, "dictation.hotkey", &"Control+Super").await.unwrap();
        assert_eq!(
            get::<String>(p, "dictation.hotkey")
                .await
                .unwrap()
                .as_deref(),
            Some("Control+Super")
        );

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Route {
            provider: String,
            model: String,
        }
        let route = Route {
            provider: "berget".into(),
            model: "kb-whisper-large".into(),
        };
        set(p, "transcription.route", &route).await.unwrap();
        assert_eq!(
            get::<Route>(p, "transcription.route").await.unwrap(),
            Some(route)
        );
    }

    #[tokio::test]
    async fn writing_twice_updates_rather_than_duplicating() {
        let db = db().await;
        let p = db.pool();
        set(p, "a.b", &1_i64).await.unwrap();
        set(p, "a.b", &2_i64).await.unwrap();
        assert_eq!(get::<i64>(p, "a.b").await.unwrap(), Some(2));
        assert_eq!(keys(p).await.unwrap(), vec!["a.b".to_owned()]);
    }

    #[tokio::test]
    async fn absent_key_is_none_not_an_error() {
        let db = db().await;
        assert_eq!(get::<String>(db.pool(), "never.set").await.unwrap(), None);
        assert_eq!(get_or(db.pool(), "never.set", 7_i64).await.unwrap(), 7);
    }

    #[tokio::test]
    async fn a_setting_whose_type_changed_falls_back_instead_of_failing() {
        let db = db().await;
        let p = db.pool();
        set(p, "was.a.string", &"hello").await.unwrap();
        // Read it as a number: the shape changed between releases.
        assert_eq!(get::<i64>(p, "was.a.string").await.unwrap(), None);
        assert_eq!(get_or(p, "was.a.string", 42_i64).await.unwrap(), 42);
    }

    #[test]
    fn credential_shaped_keys_are_rejected() {
        for key in [
            "providers.openai.apiKey",
            "providers.openai.api_key",
            "auth.token",
            "smtp.password",
            "corti.clientSecret",
            "local.bearer",
        ] {
            assert!(
                matches!(check_key(key), Err(KeyError::LooksLikeCredential { .. })),
                "{key}"
            );
        }
        assert_eq!(check_key(""), Err(KeyError::Empty));
    }

    #[test]
    fn key_reference_keys_are_allowed() {
        // The legitimate pattern: settings point at a credential slot by name.
        check_key("transcription.route.keyRef").unwrap();
        check_key("transcription.route.key_ref").unwrap();
        check_key("dictation.hotkey").unwrap();
        check_key("providers.openai.baseUrl").unwrap();
    }

    #[tokio::test]
    async fn set_refuses_a_credential_shaped_key_at_the_database_boundary() {
        let db = db().await;
        let err = set(db.pool(), "providers.openai.apiKey", &"sk-live")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("credential"));
        // Nothing was written.
        assert!(keys(db.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn remove_is_idempotent() {
        let db = db().await;
        remove(db.pool(), "not.there").await.unwrap();
        set(db.pool(), "there", &true).await.unwrap();
        remove(db.pool(), "there").await.unwrap();
        assert_eq!(get::<bool>(db.pool(), "there").await.unwrap(), None);
    }

    #[tokio::test]
    async fn settings_executor_writes_follow_transaction_commit_and_rollback() {
        let db = db().await;
        set(db.pool(), "transaction.original", &1).await.unwrap();
        let mut tx = db.pool().begin().await.unwrap();
        set_on(&mut *tx, "transaction.new", &2).await.unwrap();
        remove_on(&mut *tx, "transaction.original").await.unwrap();
        assert!(set_on(&mut *tx, "auth.token", &"blocked").await.is_err());
        tx.rollback().await.unwrap();
        assert_eq!(
            get::<i32>(db.pool(), "transaction.original").await.unwrap(),
            Some(1)
        );
        assert_eq!(
            get::<i32>(db.pool(), "transaction.new").await.unwrap(),
            None
        );
        let mut tx = db.pool().begin().await.unwrap();
        set_on(&mut *tx, "transaction.new", &2).await.unwrap();
        remove_on(&mut *tx, "transaction.original").await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            get::<i32>(db.pool(), "transaction.original").await.unwrap(),
            None
        );
        assert_eq!(
            get::<i32>(db.pool(), "transaction.new").await.unwrap(),
            Some(2)
        );
    }
}
