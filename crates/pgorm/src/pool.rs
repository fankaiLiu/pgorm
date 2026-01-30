//! Connection pool utilities

use crate::error::{OrmError, OrmResult};
use deadpool_postgres::{Manager, ManagerConfig, Pool, PoolBuilder, RecyclingMethod};
use tokio_postgres::NoTls;
use tokio_postgres::Socket;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};

/// Create a connection pool from a database URL.
///
/// This is a convenience helper that uses `NoTls` and small default settings
/// (suitable for local/dev). For production, prefer:
/// - [`create_pool_with_tls`] if your DB requires TLS
/// - [`create_pool_with_manager_config`] to inject pool/manager tuning
///
/// # Example
///
/// ```ignore
/// let pool = pgorm::create_pool("postgres://user:pass@localhost/db")?;
/// let client = pool.get().await?;
/// ```
pub fn create_pool(database_url: &str) -> OrmResult<Pool> {
    create_pool_with_config(database_url, 16)
}

/// Create a connection pool with custom configuration
pub fn create_pool_with_config(database_url: &str, max_size: usize) -> OrmResult<Pool> {
    create_pool_with_manager_config(database_url, NoTls, default_manager_config(), |builder| {
        builder.max_size(max_size)
    })
}

/// Create a connection pool using a custom TLS connector.
///
/// This is the recommended entrypoint for production use when your database requires TLS.
pub fn create_pool_with_tls<T>(database_url: &str, tls: T) -> OrmResult<Pool>
where
    T: MakeTlsConnect<Socket> + Clone + Sync + Send + 'static,
    T::Stream: Sync + Send,
    T::TlsConnect: Sync + Send,
    <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    create_pool_with_manager_config(database_url, tls, default_manager_config(), |b| {
        b.max_size(16)
    })
}

/// Create a connection pool with injected `deadpool_postgres::ManagerConfig` and `PoolBuilder`.
///
/// Use this when you need to tune pool settings (timeouts, recycling strategy, max size, etc.)
/// from your application configuration.
pub fn create_pool_with_manager_config<T>(
    database_url: &str,
    tls: T,
    manager_config: ManagerConfig,
    configure_pool: impl FnOnce(PoolBuilder) -> PoolBuilder,
) -> OrmResult<Pool>
where
    T: MakeTlsConnect<Socket> + Clone + Sync + Send + 'static,
    T::Stream: Sync + Send,
    T::TlsConnect: Sync + Send,
    <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    let pg_config: tokio_postgres::Config = database_url
        .parse()
        .map_err(|e: tokio_postgres::Error| OrmError::Connection(e.to_string()))?;

    let mgr = Manager::from_config(pg_config, tls, manager_config);
    configure_pool(Pool::builder(mgr))
        .build()
        .map_err(|e| OrmError::Pool(e.to_string()))
}

fn default_manager_config() -> ManagerConfig {
    ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    }
}
