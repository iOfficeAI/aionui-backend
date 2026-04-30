use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::pool::PoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Sqlite, SqlitePool};
use tracing::{info, warn};

use crate::error::DbError;

/// Maximum number of connections in the pool.
const MAX_CONNECTIONS: u32 = 5;

/// SQLite busy timeout in milliseconds.
const BUSY_TIMEOUT_MS: u64 = 5000;

/// Wraps a SQLite connection pool with lifecycle management.
#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Closes all connections in the pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Initialize a file-backed SQLite database.
///
/// Creates the database file and parent directories if they don't exist,
/// configures pragmas (foreign_keys, busy_timeout, journal_mode=WAL),
/// runs migrations, and ensures the system default user exists.
///
/// If initialization fails on an existing file, only explicit corruption-like
/// failures attempt recovery by backing up the corrupted file and creating a
/// fresh database. Migration mismatches and lock contention fail fast.
pub async fn init_database(path: &Path) -> Result<Database, DbError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| DbError::Init(format!("Failed to create database directory: {e}")))?;
    }

    match try_init_file(path).await {
        Ok(db) => Ok(db),
        Err(e) if path.exists() && should_attempt_recovery(&e) => {
            warn!("Database initialization failed, attempting recovery: {e}");
            recover_and_retry(path, e).await
        }
        Err(e) => Err(e),
    }
}

/// Initialize an in-memory SQLite database (for testing).
///
/// Uses a single connection to ensure all queries share the same in-memory database.
/// Note: WAL journal mode is not available for in-memory databases.
pub async fn init_database_memory() -> Result<Database, DbError> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|e| DbError::Init(format!("Invalid memory connection string: {e}")))?
        .foreign_keys(true)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));

    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(DbError::Query)?;

    run_migrations(&pool).await?;
    ensure_system_user(&pool).await?;

    info!("In-memory database initialized");
    Ok(Database { pool })
}

async fn try_init_file(path: &Path) -> Result<Database, DbError> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
        .journal_mode(SqliteJournalMode::Wal);

    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(opts)
        .await
        .map_err(DbError::Query)?;

    run_migrations(&pool).await?;
    ensure_system_user(&pool).await?;

    info!("Database initialized at {}", path.display());
    Ok(Database { pool })
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
    sqlx::migrate!().run(pool).await.map_err(DbError::Migration)?;
    ensure_schema_columns(pool).await
}

/// Ensure columns expected by Rust models exist in the database.
///
/// `CREATE TABLE IF NOT EXISTS` does not modify existing tables, so columns
/// added after a table was first created may be missing. This function
/// safely adds any missing columns via `ALTER TABLE ADD COLUMN`.
async fn ensure_schema_columns(pool: &SqlitePool) -> Result<(), DbError> {
    let expected: &[(&str, &str, &str)] = &[
        ("cron_jobs", "skill_content", "TEXT"),
        ("cron_jobs", "description", "TEXT"),
        ("conversations", "pinned", "INTEGER NOT NULL DEFAULT 0"),
        ("conversations", "pinned_at", "INTEGER"),
    ];

    for &(table, column, col_def) in expected {
        let exists: bool = sqlx::query_scalar("SELECT COUNT(*) > 0 FROM pragma_table_info(?) WHERE name = ?")
            .bind(table)
            .bind(column)
            .fetch_one(pool)
            .await
            .map_err(DbError::Query)?;

        if !exists {
            let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {col_def}");
            sqlx::query(&sql).execute(pool).await.map_err(DbError::Query)?;
            info!("Added missing column {table}.{column}");
        }
    }
    Ok(())
}

/// Ensure the system default user exists.
///
/// Uses INSERT OR IGNORE so it is safe to call on every startup.
/// The system user has an empty password hash, which signals "needs setup".
async fn ensure_system_user(pool: &SqlitePool) -> Result<(), DbError> {
    let now = aionui_common::now_ms();
    sqlx::query(
        "INSERT OR IGNORE INTO users (id, username, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("system_default_user")
    .bind("system")
    .bind("")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(DbError::Query)?;
    Ok(())
}

async fn recover_and_retry(path: &Path, original_error: DbError) -> Result<Database, DbError> {
    let backup_path = format!("{}.backup.{}", path.display(), aionui_common::now_ms());
    warn!("Backing up corrupted database to: {backup_path}");

    std::fs::rename(path, &backup_path).map_err(|e| {
        DbError::Init(format!(
            "Recovery failed: could not backup corrupted database: {e}. \
             Original error: {original_error}"
        ))
    })?;

    match try_init_file(path).await {
        Ok(db) => {
            warn!("Database recovered. Backup at: {backup_path}");
            Ok(db)
        }
        Err(retry_err) => Err(DbError::Init(format!(
            "Recovery failed after backup: {retry_err}. Original error: {original_error}"
        ))),
    }
}

fn should_attempt_recovery(err: &DbError) -> bool {
    match err {
        DbError::Migration(_) => false,
        DbError::NotFound(_) | DbError::Conflict(_) => false,
        DbError::Query(_) | DbError::Init(_) => is_corruption_like_error(err),
    }
}

fn is_corruption_like_error(err: &DbError) -> bool {
    let message = err.to_string().to_ascii_lowercase();

    [
        "sqlite_corrupt",
        "database disk image is malformed",
        "file is not a database",
        "sqlite_notadb",
        "malformed database schema",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_skips_migration_version_mismatch() {
        let err = DbError::Migration(sqlx::migrate::MigrateError::VersionMismatch(6));

        assert!(
            !should_attempt_recovery(&err),
            "migration checksum mismatch must not trigger recovery"
        );
    }

    #[test]
    fn recovery_skips_lock_contention_errors() {
        let err = DbError::Init("database is locked".into());

        assert!(
            !should_attempt_recovery(&err),
            "lock contention must not trigger recovery"
        );
    }

    #[test]
    fn recovery_allows_corruption_like_errors() {
        let err = DbError::Init("database disk image is malformed".into());

        assert!(
            should_attempt_recovery(&err),
            "corruption-like failures should trigger recovery"
        );
    }
}
