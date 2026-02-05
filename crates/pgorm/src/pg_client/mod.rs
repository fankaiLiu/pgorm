//! Unified Postgres client with built-in monitoring and SQL checking.
//!
//! `PgClient` is the recommended way to interact with PostgreSQL. It combines:
//! - Automatic model registration via `#[derive(Model)]`
//! - SQL validation against registered schemas
//! - Query monitoring and statistics
//! - Configurable timeouts and slow query detection
//! - Dynamic SQL execution with type-safe mapping
//!
//! # Example
//!
//! ```ignore
//! use pgorm::{create_pool, PgClient, PgClientConfig, Model, FromRow};
//! use std::time::Duration;
//!
//! #[derive(Debug, FromRow, Model)]
//! #[orm(table = "products")]
//! struct Product {
//!     #[orm(id)]
//!     id: i64,
//!     name: String,
//! }
//!
//! let pool = create_pool(&database_url)?;
//! let client = pool.get().await?;
//!
//! // Create PgClient with configuration
//! let pg = PgClient::with_config(client, PgClientConfig::new()
//!     .timeout(Duration::from_secs(30))
//!     .slow_threshold(Duration::from_secs(1))
//!     .with_logging());
//!
//! // Model-based queries (monitored)
//! let products = Product::select_all(&pg).await?;
//!
//! // Dynamic SQL queries (also monitored)
//! let users: Vec<User> = pg.sql_query_as(
//!     "SELECT * FROM users WHERE status = $1",
//!     &[&"active"]
//! ).await?;
//!
//! let count = pg.sql_execute(
//!     "UPDATE users SET status = $1 WHERE last_login < $2",
//!     &[&"inactive", &cutoff_date]
//! ).await?;
//!
//! // Get query statistics
//! println!("Stats: {:?}", pg.stats());
//! ```

mod check;
pub mod config;
mod execute;
mod statement_cache;
mod stream;

pub use check::ModelCheckResult;
pub use config::{
    CheckMode, DangerousDmlPolicy, PgClientConfig, SelectWithoutLimitPolicy, SqlPolicy,
    StatementCacheConfig,
};

use crate::check::SchemaRegistry;
use crate::checked_client::ModelRegistration;
#[cfg(feature = "tracing")]
use crate::monitor::TracingSqlHook;
use crate::monitor::{
    CompositeHook, LoggingMonitor, QueryHook, QueryMonitor, QueryStats, StatsMonitor,
};
use statement_cache::StatementCache;
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// Unified Postgres client with monitoring and SQL checking.
///
/// This is the recommended client for most use cases. It provides:
/// - Automatic model registration from `#[derive(Model)]`
/// - SQL validation against registered schemas
/// - Query monitoring and statistics (enabled by default)
/// - Configurable timeouts and slow query detection
///
/// # Example
///
/// ```ignore
/// // Basic usage
/// let pg = PgClient::new(client);
/// let products = Product::select_all(&pg).await?;
///
/// // With configuration
/// let pg = PgClient::with_config(client, PgClientConfig::new()
///     .strict()
///     .timeout(Duration::from_secs(30))
///     .with_logging());
///
/// // Get statistics
/// let stats = pg.stats();
/// ```
pub struct PgClient<C> {
    client: C,
    registry: Arc<SchemaRegistry>,
    stats: Arc<StatsMonitor>,
    logging_monitor: Option<LoggingMonitor>,
    custom_monitor: Option<Arc<dyn QueryMonitor>>,
    hook: Option<Arc<dyn QueryHook>>,
    #[cfg(feature = "tracing")]
    tracing_sql_hook: Option<TracingSqlHook>,
    statement_cache: Option<StatementCache>,
    config: PgClientConfig,
}

impl<C> PgClient<C> {
    /// Create a new `PgClient` with default configuration.
    ///
    /// - Models are auto-registered via inventory
    /// - Statistics collection is enabled
    /// - SQL checking is in warn-only mode
    pub fn new(client: C) -> Self {
        Self::with_config(client, PgClientConfig::default())
    }

    /// Create a new `PgClient` with custom configuration.
    pub fn with_config(client: C, config: PgClientConfig) -> Self {
        let mut registry = SchemaRegistry::new();
        for reg in inventory::iter::<ModelRegistration> {
            (reg.register_fn)(&mut registry);
        }

        let logging_monitor = if config.logging_enabled {
            let mut monitor = LoggingMonitor::new();
            if let Some(min) = config.log_min_duration {
                monitor = monitor.min_duration(min);
            }
            Some(monitor)
        } else {
            None
        };

        let statement_cache = (config.statement_cache.enabled
            && config.statement_cache.capacity > 0)
            .then(|| StatementCache::new(config.statement_cache.capacity));

        Self {
            client,
            registry: Arc::new(registry),
            stats: Arc::new(StatsMonitor::new()),
            logging_monitor,
            custom_monitor: None,
            hook: None,
            #[cfg(feature = "tracing")]
            tracing_sql_hook: None,
            statement_cache,
            config,
        }
    }

    /// Create a `PgClient` without auto-registration.
    pub fn new_empty(client: C) -> Self {
        Self {
            client,
            registry: Arc::new(SchemaRegistry::new()),
            stats: Arc::new(StatsMonitor::new()),
            logging_monitor: None,
            custom_monitor: None,
            hook: None,
            #[cfg(feature = "tracing")]
            tracing_sql_hook: None,
            statement_cache: None,
            config: PgClientConfig::default(),
        }
    }

    /// Add a custom query monitor.
    pub fn with_monitor<M: QueryMonitor + 'static>(mut self, monitor: M) -> Self {
        self.custom_monitor = Some(Arc::new(monitor));
        self
    }

    /// Add a custom query monitor from an `Arc`.
    pub fn with_monitor_arc(mut self, monitor: Arc<dyn QueryMonitor>) -> Self {
        self.custom_monitor = Some(monitor);
        self
    }

    /// Add a query hook.
    pub fn with_hook<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hook = Some(Arc::new(hook));
        self
    }

    /// Add a query hook from an `Arc`.
    pub fn with_hook_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hook = Some(hook);
        self
    }

    /// Add a query hook.
    ///
    /// If a hook is already set, this composes it with the new hook (existing first).
    pub fn add_hook<H: QueryHook + 'static>(self, hook: H) -> Self {
        self.add_hook_arc(Arc::new(hook))
    }

    /// Add a query hook from an `Arc`.
    ///
    /// If a hook is already set, this composes it with the new hook (existing first).
    pub fn add_hook_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hook = Some(match self.hook.take() {
            None => hook,
            Some(existing) => Arc::new(CompositeHook::new().add_arc(existing).add_arc(hook)),
        });
        self
    }

    /// Get a reference to the schema registry.
    pub fn registry(&self) -> &SchemaRegistry {
        &self.registry
    }

    /// Get current query statistics.
    pub fn stats(&self) -> QueryStats {
        self.stats.stats()
    }

    /// Reset query statistics.
    pub fn reset_stats(&self) {
        self.stats.reset();
    }

    /// Get a reference to the inner client.
    pub fn inner(&self) -> &C {
        &self.client
    }

    /// Consume this wrapper and return the inner client.
    pub fn into_inner(self) -> C {
        self.client
    }

    /// Get the current configuration.
    pub fn config(&self) -> &PgClientConfig {
        &self.config
    }

    /// Emit the final SQL that will actually be executed via `tracing` (target: `pgorm.sql`).
    ///
    /// Requires crate feature `tracing`.
    #[cfg(feature = "tracing")]
    pub fn with_tracing_sql(mut self) -> Self {
        self.tracing_sql_hook = Some(TracingSqlHook::new());
        self
    }

    /// Same as [`PgClient::with_tracing_sql`], but allows custom hook configuration
    /// (e.g. `TracingSqlHook::new().no_truncate()`).
    ///
    /// Requires crate feature `tracing`.
    #[cfg(feature = "tracing")]
    pub fn with_tracing_sql_hook(mut self, hook: TracingSqlHook) -> Self {
        self.tracing_sql_hook = Some(hook);
        self
    }
}
