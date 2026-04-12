use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router, middleware};
use serde::Serialize;

use aionui_auth::{
    AuthRouterState, CookieConfig, JwtService, QrTokenStore, auth_routes, csrf_middleware,
    resolve_jwt_secret, security_headers_middleware,
};
use aionui_db::{Database, IUserRepository, SqliteUserRepository};

/// Application configuration parsed from CLI arguments.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub data_dir: String,
}

impl AppConfig {
    /// Format as `host:port` for socket binding.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Path to the SQLite database file.
    pub fn database_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.data_dir).join("aionui.db")
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: aionui_common::constants::DEFAULT_HOST.to_string(),
            port: aionui_common::constants::DEFAULT_PORT,
            data_dir: "data".to_string(),
        }
    }
}

/// Shared application services for dependency injection.
pub struct AppServices {
    pub database: Database,
    pub jwt_service: Arc<JwtService>,
    pub user_repo: Arc<dyn IUserRepository>,
    pub cookie_config: Arc<CookieConfig>,
    pub qr_token_store: Arc<QrTokenStore>,
}

impl AppServices {
    /// Build application services from an initialized database.
    ///
    /// Resolves JWT secret (env → db → generate), constructs all shared
    /// services, and persists a newly generated secret to the database.
    pub async fn from_database(database: Database) -> anyhow::Result<Self> {
        let user_repo: Arc<dyn IUserRepository> =
            Arc::new(SqliteUserRepository::new(database.pool().clone()));

        // Resolve JWT secret: env var → system user db field → random generation
        let env_secret = std::env::var("JWT_SECRET").ok();
        let system_user = user_repo
            .get_system_user()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get system user: {e}"))?;

        let db_secret = system_user
            .as_ref()
            .and_then(|u| u.jwt_secret.as_deref())
            .filter(|s| !s.is_empty());

        let (secret, is_new) = resolve_jwt_secret(env_secret.as_deref(), db_secret);

        // Persist newly generated secret to database
        if is_new
            && let Some(user) = &system_user
        {
            user_repo
                .update_jwt_secret(&user.id, &secret)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to persist JWT secret: {e}"))?;
            tracing::info!("Generated and persisted new JWT secret");
        }

        Ok(Self {
            database,
            jwt_service: Arc::new(JwtService::new(secret)),
            user_repo,
            cookie_config: Arc::new(CookieConfig::from_env()),
            qr_token_store: Arc::new(QrTokenStore::new()),
        })
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Create the application router with all routes and global middleware.
///
/// Middleware stack (outermost → innermost):
/// 1. Security response headers (X-Frame-Options, etc.)
/// 2. CSRF protection (Double Submit Cookie)
/// 3. Route handlers (auth routes + health check)
pub fn create_router(services: &AppServices) -> Router {
    let auth_state = AuthRouterState {
        jwt_service: services.jwt_service.clone(),
        user_repo: services.user_repo.clone(),
        cookie_config: services.cookie_config.clone(),
        qr_token_store: services.qr_token_store.clone(),
    };

    Router::new()
        .route("/health", get(health_check))
        .merge(auth_routes(auth_state))
        .layer(middleware::from_fn_with_state(
            services.cookie_config.clone(),
            csrf_middleware,
        ))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 25808);
        assert_eq!(config.data_dir, "data");
    }

    #[test]
    fn test_app_config_socket_addr() {
        let config = AppConfig {
            host: "0.0.0.0".to_string(),
            port: 3000,
            data_dir: "data".to_string(),
        };
        assert_eq!(config.socket_addr(), "0.0.0.0:3000");
    }

    #[test]
    fn test_app_config_database_path() {
        let config = AppConfig {
            host: "127.0.0.1".to_string(),
            port: 25808,
            data_dir: "/tmp/aionui".to_string(),
        };
        assert_eq!(
            config.database_path(),
            std::path::PathBuf::from("/tmp/aionui/aionui.db")
        );
    }

    #[tokio::test]
    async fn test_app_services_from_memory_db() {
        let db = aionui_db::init_database_memory().await.unwrap();
        let services = AppServices::from_database(db).await.unwrap();

        // JWT service should be functional
        let token = services
            .jwt_service
            .sign("test_user", "testuser")
            .unwrap();
        let payload = services.jwt_service.verify(&token).unwrap();
        assert_eq!(payload.user_id, "test_user");

        // User repo should have system user
        let has_users = services.user_repo.has_users().await.unwrap();
        assert!(!has_users); // system user has empty password → not counted

        services.database.close().await;
    }

    #[tokio::test]
    async fn test_jwt_secret_persisted_to_db() {
        let db = aionui_db::init_database_memory().await.unwrap();
        let services = AppServices::from_database(db).await.unwrap();

        // System user should now have a jwt_secret persisted
        let system_user = services.user_repo.get_system_user().await.unwrap();
        let jwt_secret = system_user.unwrap().jwt_secret;
        assert!(jwt_secret.is_some());
        assert!(!jwt_secret.unwrap().is_empty());

        services.database.close().await;
    }
}
