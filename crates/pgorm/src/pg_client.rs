//! Unified Postgres client with built-in monitoring and SQL checking.
//!
//! `PgClient` is the recommended way to interact with PostgreSQL. It combines:
//! - Automatic model registration via `#[derive(Model)]`
//! - SQL validation against registered schemas
//! - Query monitoring and statistics
//! - Configurable timeouts and slow query detection
//!
//! # Example
//!
//! ```ignore
//! use pgorm::{create_pool, PgClient, Model, FromRow};
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
//! // Create PgClient - models auto-registered, monitoring enabled by default
//! let pg = PgClient::new(client);
//!
//! // All queries are monitored and SQL-checked automatically
//! let products = Product::select_all(&pg).await?;
//!
//! // Get query statistics
//! println!("Stats: {:?}", pg.stats());
//! ```

use crate::check::SchemaRegistry;
use crate::checked_client::ModelRegistration;
use crate::error::{OrmError, OrmResult};
use crate::monitor::{
    LoggingMonitor, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryStats, StatsMonitor,
};
use crate::GenericClient;

// Re-export CheckMode from checked_client for public API
pub use crate::checked_client::CheckMode;

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Configuration for `PgClient`.
#[derive(Debug, Clone)]
pub struct PgClientConfig {
    /// SQL check mode.
    pub check_mode: CheckMode,
    /// Query timeout duration.
    pub query_timeout: Option<Duration>,
    /// Slow query threshold for alerting.
    pub slow_query_threshold: Option<Duration>,
    /// Whether to collect query statistics.
    pub stats_enabled: bool,
    /// Whether to log queries.
    pub logging_enabled: bool,
    /// Minimum duration to log (filters out fast queries).
    pub log_min_duration: Option<Duration>,
}

impl Default for PgClientConfig {
    fn default() -> Self {
        Self {
            check_mode: CheckMode::WarnOnly,
            query_timeout: None,
            slow_query_threshold: None,
            stats_enabled: true,
            logging_enabled: false,
            log_min_duration: None,
        }
    }
}

impl PgClientConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set SQL check mode.
    pub fn check_mode(mut self, mode: CheckMode) -> Self {
        self.check_mode = mode;
        self
    }

    /// Enable strict SQL checking.
    pub fn strict(mut self) -> Self {
        self.check_mode = CheckMode::Strict;
        self
    }

    /// Disable SQL checking.
    pub fn no_check(mut self) -> Self {
        self.check_mode = CheckMode::Disabled;
        self
    }

    /// Set query timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.query_timeout = Some(duration);
        self
    }

    /// Set slow query threshold.
    pub fn slow_threshold(mut self, duration: Duration) -> Self {
        self.slow_query_threshold = Some(duration);
        self
    }

    /// Enable query statistics collection.
    pub fn with_stats(mut self) -> Self {
        self.stats_enabled = true;
        self
    }

    /// Disable query statistics collection.
    pub fn no_stats(mut self) -> Self {
        self.stats_enabled = false;
        self
    }

    /// Enable query logging.
    pub fn with_logging(mut self) -> Self {
        self.logging_enabled = true;
        self
    }

    /// Enable query logging with minimum duration filter.
    pub fn log_slow_queries(mut self, min_duration: Duration) -> Self {
        self.logging_enabled = true;
        self.log_min_duration = Some(min_duration);
        self
    }
}

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

        Self {
            client,
            registry: Arc::new(registry),
            stats: Arc::new(StatsMonitor::new()),
            logging_monitor,
            custom_monitor: None,
            hook: None,
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
            config: PgClientConfig::default(),
        }
    }

    /// Add a custom query monitor.
    pub fn with_monitor<M: QueryMonitor + 'static>(mut self, monitor: M) -> Self {
        self.custom_monitor = Some(Arc::new(monitor));
        self
    }

    /// Add a query hook.
    pub fn with_hook<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hook = Some(Arc::new(hook));
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

    /// Check SQL against the registry.
    fn check_sql(&self, sql: &str) -> OrmResult<()> {
        match self.config.check_mode {
            CheckMode::Disabled => Ok(()),
            CheckMode::WarnOnly => {
                let issues = self.registry.check_sql(sql);
                for issue in issues {
                    eprintln!("[pgorm warn] SQL check: {:?}", issue);
                }
                Ok(())
            }
            CheckMode::Strict => {
                let issues = self.registry.check_sql(sql);
                if issues.is_empty() {
                    Ok(())
                } else {
                    let messages: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
                    Err(OrmError::validation(format!(
                        "SQL check failed: {}",
                        messages.join("; ")
                    )))
                }
            }
        }
    }

    /// Process hook before query.
    fn process_hook(&self, ctx: &QueryContext) -> Result<Option<String>, OrmError> {
        use crate::monitor::HookAction;

        if let Some(hook) = &self.hook {
            match hook.before_query(ctx) {
                HookAction::Continue => Ok(None),
                HookAction::ModifySql(sql) => Ok(Some(sql)),
                HookAction::Abort(reason) => Err(OrmError::validation(format!(
                    "Query aborted by hook: {}",
                    reason
                ))),
            }
        } else {
            Ok(None)
        }
    }

    /// Report query result to monitors.
    fn report_result(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        // Always report to stats monitor if enabled
        if self.config.stats_enabled {
            self.stats.on_query_complete(ctx, duration, result);
        }

        // Report to logging monitor if enabled
        if let Some(ref logging) = self.logging_monitor {
            logging.on_query_complete(ctx, duration, result);
        }

        // Report to custom monitor if set
        if let Some(ref monitor) = self.custom_monitor {
            monitor.on_query_complete(ctx, duration, result);
        }

        // Check slow query threshold
        if let Some(threshold) = self.config.slow_query_threshold {
            if duration > threshold {
                if let Some(ref logging) = self.logging_monitor {
                    logging.on_slow_query(ctx, duration);
                }
                if let Some(ref monitor) = self.custom_monitor {
                    monitor.on_slow_query(ctx, duration);
                }
            }
        }

        // Hook after query
        if let Some(ref hook) = self.hook {
            hook.after_query(ctx, duration, result);
        }
    }
}

impl<C: GenericClient> PgClient<C> {
    /// Execute with timeout if configured.
    async fn execute_with_timeout<T, F>(&self, future: F) -> OrmResult<T>
    where
        F: std::future::Future<Output = OrmResult<T>>,
    {
        match self.config.query_timeout {
            Some(timeout) => tokio::time::timeout(timeout, future)
                .await
                .map_err(|_| OrmError::Timeout(timeout))?,
            None => future.await,
        }
    }
}

impl<C: GenericClient> GenericClient for PgClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        // Check SQL first
        self.check_sql(sql)?;

        let ctx = QueryContext::new(sql, params.len());

        // Process hook
        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        // Execute
        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query(effective_sql, params))
            .await;
        let duration = start.elapsed();

        // Report
        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {:?}", d)),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.check_sql(sql)?;

        let ctx = QueryContext::new(sql, params.len());
        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_one(effective_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound(_)) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {:?}", d)),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.check_sql(sql)?;

        let ctx = QueryContext::new(sql, params.len());
        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_opt(effective_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(Some(_)) => QueryResult::OptionalRow(true),
            Ok(None) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {:?}", d)),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.check_sql(sql)?;

        let ctx = QueryContext::new(sql, params.len());
        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.execute(effective_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(n) => QueryResult::Affected(*n),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {:?}", d)),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = PgClientConfig::default();
        assert_eq!(config.check_mode, CheckMode::WarnOnly);
        assert!(config.stats_enabled);
        assert!(!config.logging_enabled);
    }

    #[test]
    fn test_config_builder() {
        let config = PgClientConfig::new()
            .strict()
            .timeout(Duration::from_secs(30))
            .with_logging();

        assert_eq!(config.check_mode, CheckMode::Strict);
        assert_eq!(config.query_timeout, Some(Duration::from_secs(30)));
        assert!(config.logging_enabled);
    }
}
