//! Connection pool utilities

use crate::error::{OrmError, OrmResult};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;

/// Create a connection pool from a database URL
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
    let pg_config: tokio_postgres::Config = database_url
        .parse()
        .map_err(|e: tokio_postgres::Error| OrmError::Connection(e.to_string()))?;

    let mgr_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let mgr = Manager::from_config(pg_config, NoTls, mgr_config);

    Pool::builder(mgr)
        .max_size(max_size)
        .build()
        .map_err(|e| OrmError::Pool(e.to_string()))
}
