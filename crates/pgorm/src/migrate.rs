//! SQL migrations via [`refinery`].
//!
//! `pgorm` keeps migration definitions in your application crate (or a dedicated migrations crate)
//! and provides small helpers to run them with consistent error handling.
//!
//! # Example (embedded SQL migrations)
//!
//! ```ignore
//! use pgorm::{create_pool, migrate};
//! use std::env;
//!
//! mod embedded {
//!     use pgorm::embed_migrations;
//!     embed_migrations!("./migrations");
//! }
//!
//! # async fn main_impl() -> pgorm::OrmResult<()> {
//! let pool = create_pool(&env::var("DATABASE_URL")?)?;
//! migrate::run_pool(&pool, embedded::migrations::runner()).await?;
//! Ok(())
//! # }
//! ```

use crate::error::OrmResult;

pub use refinery::{Error, Migration, Report, Runner, Target, embed_migrations};

/// Run migrations on a single PostgreSQL connection.
pub async fn run(client: &mut tokio_postgres::Client, runner: Runner) -> OrmResult<Report> {
    Ok(runner.run_async(client).await?)
}

/// Acquire a connection from a pool and run migrations on it.
#[cfg(feature = "pool")]
pub async fn run_pool(pool: &deadpool_postgres::Pool, runner: Runner) -> OrmResult<Report> {
    let mut client = pool.get().await?;
    run(&mut client, runner).await
}

