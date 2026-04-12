mod database;
mod error;
pub mod models;
mod repository;

pub use database::{init_database, init_database_memory, Database};
pub use error::DbError;
pub use repository::{IUserRepository, SqliteUserRepository};

// Re-export sqlx pool type for downstream crates
pub use sqlx::SqlitePool;
