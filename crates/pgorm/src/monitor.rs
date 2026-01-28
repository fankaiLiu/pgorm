//! Query monitoring and hooks for SQL execution.
//!
//! This module provides traits and utilities for:
//! - Monitoring SQL execution time
//! - Hooking into SQL execution lifecycle (before/after execution)
//! - Logging and metrics collection
//!
//! # Example
//!
//! ```rust,ignore
//! use pgorm::monitor::{QueryMonitor, QueryHook, QueryContext, HookAction, InstrumentedClient};
//! use std::time::Duration;
//!
//! // Simple logging monitor
//! struct LoggingMonitor;
//!
//! impl QueryMonitor for LoggingMonitor {
//!     fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
//!         println!("[{:?}] {} - {:?}", duration, ctx.sql, result);
//!     }
//! }
//!
//! // Use with instrumented client
//! let client = InstrumentedClient::new(db_client)
//!     .with_monitor(LoggingMonitor);
//! ```

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// The type of SQL operation being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    /// SELECT query
    Select,
    /// INSERT statement
    Insert,
    /// UPDATE statement
    Update,
    /// DELETE statement
    Delete,
    /// Other SQL (e.g., DDL, custom)
    Other,
}

impl QueryType {
    /// Detect query type from SQL string.
    pub fn from_sql(sql: &str) -> Self {
        let trimmed = sql.trim_start().to_uppercase();
        if trimmed.starts_with("SELECT") || trimmed.starts_with("WITH") {
            QueryType::Select
        } else if trimmed.starts_with("INSERT") {
            QueryType::Insert
        } else if trimmed.starts_with("UPDATE") {
            QueryType::Update
        } else if trimmed.starts_with("DELETE") {
            QueryType::Delete
        } else {
            QueryType::Other
        }
    }
}

/// Context information about the query being executed.
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// The SQL statement.
    pub sql: String,
    /// Number of parameters.
    pub param_count: usize,
    /// Detected query type.
    pub query_type: QueryType,
    /// Optional query name/tag for identification.
    pub tag: Option<String>,
}

impl QueryContext {
    /// Create a new query context.
    pub fn new(sql: &str, param_count: usize) -> Self {
        Self {
            sql: sql.to_string(),
            param_count,
            query_type: QueryType::from_sql(sql),
            tag: None,
        }
    }

    /// Add a tag to identify this query.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}

/// Result of a query execution for monitoring purposes.
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// Query returned rows.
    Rows(usize),
    /// Query affected rows (for mutations).
    Affected(u64),
    /// Query returned a single optional row.
    OptionalRow(bool),
    /// Query failed with an error.
    Error(String),
}

impl fmt::Display for QueryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryResult::Rows(n) => write!(f, "{} rows", n),
            QueryResult::Affected(n) => write!(f, "{} affected", n),
            QueryResult::OptionalRow(found) => {
                write!(f, "{}", if *found { "1 row" } else { "0 rows" })
            }
            QueryResult::Error(e) => write!(f, "error: {}", e),
        }
    }
}

/// Trait for monitoring SQL query execution.
///
/// Implement this trait to collect metrics, log queries, or integrate
/// with observability systems.
pub trait QueryMonitor: Send + Sync {
    /// Called before a query is executed.
    ///
    /// Default implementation does nothing.
    fn on_query_start(&self, _ctx: &QueryContext) {}

    /// Called after a query completes (success or failure).
    ///
    /// # Arguments
    /// * `ctx` - Query context information
    /// * `duration` - Time taken to execute the query
    /// * `result` - The result of the query
    fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult);

    /// Called when a slow query is detected.
    ///
    /// Default implementation does nothing. Override to add alerting.
    fn on_slow_query(&self, _ctx: &QueryContext, _duration: Duration) {}
}

/// Action to take after a hook processes a query.
#[derive(Debug, Clone)]
pub enum HookAction {
    /// Continue with the original query.
    Continue,
    /// Continue with a modified SQL statement.
    ModifySql(String),
    /// Abort the query with an error.
    Abort(String),
}

/// Trait for hooking into the query execution lifecycle.
///
/// Hooks can inspect, modify, or abort queries before they are executed.
pub trait QueryHook: Send + Sync {
    /// Called before a query is executed.
    ///
    /// Return `HookAction::Continue` to proceed normally,
    /// `HookAction::ModifySql` to change the SQL, or
    /// `HookAction::Abort` to cancel the query.
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        let _ = ctx;
        HookAction::Continue
    }

    /// Called after a query completes successfully.
    ///
    /// This is called before monitors receive the completion event.
    fn after_query(&self, _ctx: &QueryContext, _duration: Duration, _result: &QueryResult) {}
}

/// A no-op monitor that does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMonitor;

impl QueryMonitor for NoopMonitor {
    fn on_query_complete(&self, _ctx: &QueryContext, _duration: Duration, _result: &QueryResult) {}
}

/// A logging monitor that prints queries to stderr.
#[derive(Debug, Clone)]
pub struct LoggingMonitor {
    /// Minimum duration to log (filters out fast queries).
    pub min_duration: Option<Duration>,
    /// Whether to log the full SQL or truncate.
    pub max_sql_length: Option<usize>,
    /// Prefix for log messages.
    pub prefix: String,
}

impl Default for LoggingMonitor {
    fn default() -> Self {
        Self {
            min_duration: None,
            max_sql_length: Some(200),
            prefix: "[pgorm]".to_string(),
        }
    }
}

impl LoggingMonitor {
    /// Create a new logging monitor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only log queries slower than this duration.
    pub fn min_duration(mut self, duration: Duration) -> Self {
        self.min_duration = Some(duration);
        self
    }

    /// Set maximum SQL length to display.
    pub fn max_sql_length(mut self, len: usize) -> Self {
        self.max_sql_length = Some(len);
        self
    }

    /// Set prefix for log messages.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    fn truncate_sql(&self, sql: &str) -> String {
        match self.max_sql_length {
            Some(max) if sql.len() > max => format!("{}...", &sql[..max]),
            _ => sql.to_string(),
        }
    }
}

impl QueryMonitor for LoggingMonitor {
    fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        if let Some(min) = self.min_duration {
            if duration < min {
                return;
            }
        }

        let sql = self.truncate_sql(&ctx.sql);
        let tag = ctx.tag.as_deref().unwrap_or("-");
        eprintln!(
            "{} [{:?}] [{}] {:?} | {} | {}",
            self.prefix, ctx.query_type, tag, duration, result, sql
        );
    }

    fn on_slow_query(&self, ctx: &QueryContext, duration: Duration) {
        let sql = self.truncate_sql(&ctx.sql);
        eprintln!(
            "{} SLOW QUERY [{:?}]: {:?} | {}",
            self.prefix, ctx.query_type, duration, sql
        );
    }
}

/// A monitor that tracks query statistics.
#[derive(Debug, Default)]
pub struct StatsMonitor {
    stats: std::sync::RwLock<QueryStats>,
}

/// Collected query statistics.
#[derive(Debug, Clone, Default)]
pub struct QueryStats {
    /// Total number of queries executed.
    pub total_queries: u64,
    /// Total number of failed queries.
    pub failed_queries: u64,
    /// Total execution time.
    pub total_duration: Duration,
    /// Number of SELECT queries.
    pub select_count: u64,
    /// Number of INSERT queries.
    pub insert_count: u64,
    /// Number of UPDATE queries.
    pub update_count: u64,
    /// Number of DELETE queries.
    pub delete_count: u64,
    /// Slowest query duration.
    pub max_duration: Duration,
    /// Slowest query SQL.
    pub slowest_query: Option<String>,
}

impl StatsMonitor {
    /// Create a new stats monitor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a snapshot of current statistics.
    pub fn stats(&self) -> QueryStats {
        self.stats.read().unwrap().clone()
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        *self.stats.write().unwrap() = QueryStats::default();
    }
}

impl QueryMonitor for StatsMonitor {
    fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        let mut stats = self.stats.write().unwrap();
        stats.total_queries += 1;
        stats.total_duration += duration;

        match ctx.query_type {
            QueryType::Select => stats.select_count += 1,
            QueryType::Insert => stats.insert_count += 1,
            QueryType::Update => stats.update_count += 1,
            QueryType::Delete => stats.delete_count += 1,
            QueryType::Other => {}
        }

        if matches!(result, QueryResult::Error(_)) {
            stats.failed_queries += 1;
        }

        if duration > stats.max_duration {
            stats.max_duration = duration;
            stats.slowest_query = Some(ctx.sql.clone());
        }
    }
}

/// A composite monitor that delegates to multiple monitors.
pub struct CompositeMonitor {
    monitors: Vec<Arc<dyn QueryMonitor>>,
}

impl CompositeMonitor {
    /// Create an empty composite monitor.
    pub fn new() -> Self {
        Self {
            monitors: Vec::new(),
        }
    }

    /// Add a monitor.
    pub fn add<M: QueryMonitor + 'static>(mut self, monitor: M) -> Self {
        self.monitors.push(Arc::new(monitor));
        self
    }

    /// Add an Arc-wrapped monitor.
    pub fn add_arc(mut self, monitor: Arc<dyn QueryMonitor>) -> Self {
        self.monitors.push(monitor);
        self
    }
}

impl Default for CompositeMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryMonitor for CompositeMonitor {
    fn on_query_start(&self, ctx: &QueryContext) {
        for monitor in &self.monitors {
            monitor.on_query_start(ctx);
        }
    }

    fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        for monitor in &self.monitors {
            monitor.on_query_complete(ctx, duration, result);
        }
    }

    fn on_slow_query(&self, ctx: &QueryContext, duration: Duration) {
        for monitor in &self.monitors {
            monitor.on_slow_query(ctx, duration);
        }
    }
}

/// A composite hook that runs multiple hooks in sequence.
pub struct CompositeHook {
    hooks: Vec<Arc<dyn QueryHook>>,
}

impl CompositeHook {
    /// Create an empty composite hook.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Add a hook.
    pub fn add<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hooks.push(Arc::new(hook));
        self
    }
}

impl Default for CompositeHook {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryHook for CompositeHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        let mut current_ctx = ctx.clone();
        for hook in &self.hooks {
            match hook.before_query(&current_ctx) {
                HookAction::Continue => {}
                HookAction::ModifySql(new_sql) => {
                    current_ctx.sql = new_sql;
                }
                action @ HookAction::Abort(_) => return action,
            }
        }
        if current_ctx.sql != ctx.sql {
            HookAction::ModifySql(current_ctx.sql)
        } else {
            HookAction::Continue
        }
    }

    fn after_query(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        for hook in &self.hooks {
            hook.after_query(ctx, duration, result);
        }
    }
}

/// An instrumented database client that wraps a `GenericClient` with monitoring.
pub struct InstrumentedClient<C> {
    client: C,
    monitor: Arc<dyn QueryMonitor>,
    hook: Option<Arc<dyn QueryHook>>,
    slow_query_threshold: Option<Duration>,
}

impl<C: GenericClient> InstrumentedClient<C> {
    /// Create a new instrumented client with no monitoring.
    pub fn new(client: C) -> Self {
        Self {
            client,
            monitor: Arc::new(NoopMonitor),
            hook: None,
            slow_query_threshold: None,
        }
    }

    /// Set the query monitor.
    pub fn with_monitor<M: QueryMonitor + 'static>(mut self, monitor: M) -> Self {
        self.monitor = Arc::new(monitor);
        self
    }

    /// Set the query monitor from an Arc.
    pub fn with_monitor_arc(mut self, monitor: Arc<dyn QueryMonitor>) -> Self {
        self.monitor = monitor;
        self
    }

    /// Set a query hook.
    pub fn with_hook<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hook = Some(Arc::new(hook));
        self
    }

    /// Set a query hook from an Arc.
    pub fn with_hook_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hook = Some(hook);
        self
    }

    /// Set the slow query threshold.
    ///
    /// Queries taking longer than this will trigger `on_slow_query`.
    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = Some(threshold);
        self
    }

    /// Get a reference to the inner client.
    pub fn inner(&self) -> &C {
        &self.client
    }

    /// Get the inner client, consuming this wrapper.
    pub fn into_inner(self) -> C {
        self.client
    }

    fn process_hook(&self, ctx: &QueryContext) -> Result<Option<String>, OrmError> {
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

    fn report_result(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        if let Some(hook) = &self.hook {
            hook.after_query(ctx, duration, result);
        }

        self.monitor.on_query_complete(ctx, duration, result);

        if let Some(threshold) = self.slow_query_threshold {
            if duration > threshold {
                self.monitor.on_slow_query(ctx, duration);
            }
        }
    }
}

impl<C: GenericClient> GenericClient for InstrumentedClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        let ctx = QueryContext::new(sql, params.len());
        self.monitor.on_query_start(&ctx);

        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self.client.query(effective_sql, params).await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let ctx = QueryContext::new(sql, params.len());
        self.monitor.on_query_start(&ctx);

        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self.client.query_one(effective_sql, params).await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound { .. }) => QueryResult::OptionalRow(false),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let ctx = QueryContext::new(sql, params.len());
        self.monitor.on_query_start(&ctx);

        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self.client.query_opt(effective_sql, params).await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(Some(_)) => QueryResult::OptionalRow(true),
            Ok(None) => QueryResult::OptionalRow(false),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        let ctx = QueryContext::new(sql, params.len());
        self.monitor.on_query_start(&ctx);

        let modified_sql = self.process_hook(&ctx)?;
        let effective_sql = modified_sql.as_deref().unwrap_or(sql);

        let start = Instant::now();
        let result = self.client.execute(effective_sql, params).await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(n) => QueryResult::Affected(*n),
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
    fn test_query_type_detection() {
        assert_eq!(QueryType::from_sql("SELECT * FROM users"), QueryType::Select);
        assert_eq!(
            QueryType::from_sql("  select * FROM users"),
            QueryType::Select
        );
        assert_eq!(
            QueryType::from_sql("WITH cte AS (SELECT 1) SELECT * FROM cte"),
            QueryType::Select
        );
        assert_eq!(
            QueryType::from_sql("INSERT INTO users (name) VALUES ($1)"),
            QueryType::Insert
        );
        assert_eq!(
            QueryType::from_sql("UPDATE users SET name = $1"),
            QueryType::Update
        );
        assert_eq!(
            QueryType::from_sql("DELETE FROM users WHERE id = $1"),
            QueryType::Delete
        );
        assert_eq!(
            QueryType::from_sql("CREATE TABLE users (id INT)"),
            QueryType::Other
        );
    }

    #[test]
    fn test_logging_monitor_truncation() {
        let monitor = LoggingMonitor::new().max_sql_length(10);
        assert_eq!(monitor.truncate_sql("SELECT * FROM users"), "SELECT * F...");
        assert_eq!(monitor.truncate_sql("SELECT 1"), "SELECT 1");
    }

    #[test]
    fn test_stats_monitor() {
        let monitor = StatsMonitor::new();
        let ctx = QueryContext::new("SELECT * FROM users", 0);

        monitor.on_query_complete(&ctx, Duration::from_millis(10), &QueryResult::Rows(5));
        monitor.on_query_complete(&ctx, Duration::from_millis(20), &QueryResult::Rows(3));

        let stats = monitor.stats();
        assert_eq!(stats.total_queries, 2);
        assert_eq!(stats.select_count, 2);
        assert_eq!(stats.total_duration, Duration::from_millis(30));
    }

    #[test]
    fn test_composite_hook_modify() {
        struct AddCommentHook;
        impl QueryHook for AddCommentHook {
            fn before_query(&self, ctx: &QueryContext) -> HookAction {
                HookAction::ModifySql(format!("/* instrumented */ {}", ctx.sql))
            }
        }

        let hook = CompositeHook::new().add(AddCommentHook);
        let ctx = QueryContext::new("SELECT 1", 0);

        match hook.before_query(&ctx) {
            HookAction::ModifySql(sql) => assert_eq!(sql, "/* instrumented */ SELECT 1"),
            _ => panic!("Expected ModifySql"),
        }
    }

    #[test]
    fn test_composite_hook_abort() {
        struct BlockDeleteHook;
        impl QueryHook for BlockDeleteHook {
            fn before_query(&self, ctx: &QueryContext) -> HookAction {
                if ctx.query_type == QueryType::Delete {
                    HookAction::Abort("DELETE not allowed".to_string())
                } else {
                    HookAction::Continue
                }
            }
        }

        let hook = CompositeHook::new().add(BlockDeleteHook);
        let ctx = QueryContext::new("DELETE FROM users", 0);

        match hook.before_query(&ctx) {
            HookAction::Abort(reason) => assert_eq!(reason, "DELETE not allowed"),
            _ => panic!("Expected Abort"),
        }
    }
}
