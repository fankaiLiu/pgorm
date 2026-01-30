//! Query monitoring and hooks for SQL execution.
//!
//! This module provides traits and utilities for:
//! - Monitoring SQL execution time
//! - Hooking into SQL execution lifecycle (before/after execution)
//! - Logging and metrics collection
//! - Query timeout with best-effort cancellation
//!
//! # Example
//!
//! ```rust,ignore
//! use pgorm::monitor::{QueryMonitor, QueryHook, QueryContext, HookAction, InstrumentedClient, MonitorConfig};
//! use std::time::Duration;
//!
//! // Simple logging monitor
//! struct LoggingMonitor;
//!
//! impl QueryMonitor for LoggingMonitor {
//!     fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
//!         println!("[{:?}] {} - {:?}", duration, ctx.canonical_sql, result);
//!     }
//! }
//!
//! // Use with instrumented client
//! let config = MonitorConfig::new()
//!     .with_query_timeout(Duration::from_secs(30))
//!     .with_slow_query_threshold(Duration::from_secs(5))
//!     .enable_monitoring();
//!
//! let client = InstrumentedClient::new(db_client)
//!     .with_config(config)
//!     .with_monitor(LoggingMonitor);
//! ```

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

#[cfg(feature = "tracing")]
use tracing::Level;

fn truncate_sql_bytes(sql: &str, max_bytes: usize) -> &str {
    if sql.len() <= max_bytes {
        return sql;
    }
    let mut end = max_bytes;
    while end > 0 && !sql.is_char_boundary(end) {
        end -= 1;
    }
    &sql[..end]
}

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
        fn strip_sql_prefix(sql: &str) -> &str {
            let mut s = sql;
            loop {
                let before = s;
                s = s.trim_start();
                if s.starts_with("--") {
                    if let Some(pos) = s.find('\n') {
                        s = &s[pos + 1..];
                        continue;
                    }
                    return "";
                }
                if s.starts_with("/*") {
                    if let Some(pos) = s.find("*/") {
                        s = &s[pos + 2..];
                        continue;
                    }
                    return "";
                }
                if s.starts_with('(') {
                    s = &s[1..];
                    continue;
                }
                if s == before {
                    break;
                }
            }
            s
        }

        fn starts_with_keyword(s: &str, keyword: &str) -> bool {
            match s.get(0..keyword.len()) {
                Some(prefix) => prefix.eq_ignore_ascii_case(keyword),
                None => false,
            }
        }

        let trimmed = strip_sql_prefix(sql);
        if starts_with_keyword(trimmed, "SELECT") || starts_with_keyword(trimmed, "WITH") {
            QueryType::Select
        } else if starts_with_keyword(trimmed, "INSERT") {
            QueryType::Insert
        } else if starts_with_keyword(trimmed, "UPDATE") {
            QueryType::Update
        } else if starts_with_keyword(trimmed, "DELETE") {
            QueryType::Delete
        } else {
            QueryType::Other
        }
    }
}

/// Context information about the query being executed.
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// Canonical SQL used for statement cache keys and metrics aggregation.
    pub canonical_sql: String,
    /// The SQL statement actually executed against Postgres.
    pub exec_sql: String,
    /// Number of parameters.
    pub param_count: usize,
    /// Detected query type.
    pub query_type: QueryType,
    /// Optional query name/tag for identification.
    pub tag: Option<String>,
    /// Optional structured fields for observability (low-cardinality).
    pub fields: BTreeMap<String, String>,
}

impl QueryContext {
    /// Create a new query context.
    pub fn new(sql: &str, param_count: usize) -> Self {
        Self {
            canonical_sql: sql.to_string(),
            exec_sql: sql.to_string(),
            param_count,
            query_type: QueryType::from_sql(sql),
            tag: None,
            fields: BTreeMap::new(),
        }
    }

    /// Add a tag to identify this query.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Add a structured field (low-cardinality).
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
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
            QueryResult::Rows(n) => write!(f, "{n} rows"),
            QueryResult::Affected(n) => write!(f, "{n} affected"),
            QueryResult::OptionalRow(found) => {
                write!(f, "{}", if *found { "1 row" } else { "0 rows" })
            }
            QueryResult::Error(e) => write!(f, "error: {e}"),
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
    ModifySql {
        /// SQL to execute against Postgres.
        exec_sql: String,
        /// Optional override for canonical SQL (cache/metrics key).
        canonical_sql: Option<String>,
    },
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
            Some(max) if sql.len() > max => format!("{}...", truncate_sql_bytes(sql, max)),
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

        let canonical = self.truncate_sql(&ctx.canonical_sql);
        let sql = if ctx.exec_sql != ctx.canonical_sql {
            format!(
                "canonical: {} | exec: {}",
                canonical,
                self.truncate_sql(&ctx.exec_sql)
            )
        } else {
            canonical
        };
        let tag = ctx.tag.as_deref().unwrap_or("-");
        eprintln!(
            "{} [{:?}] [{}] {:?} | {} | {}",
            self.prefix, ctx.query_type, tag, duration, result, sql
        );
    }

    fn on_slow_query(&self, ctx: &QueryContext, duration: Duration) {
        let canonical = self.truncate_sql(&ctx.canonical_sql);
        let sql = if ctx.exec_sql != ctx.canonical_sql {
            format!(
                "canonical: {} | exec: {}",
                canonical,
                self.truncate_sql(&ctx.exec_sql)
            )
        } else {
            canonical
        };
        eprintln!(
            "{} SLOW QUERY [{:?}]: {:?} | {}",
            self.prefix, ctx.query_type, duration, sql
        );
    }
}

/// A `tracing`-based debug hook that emits the SQL that will actually be executed.
///
/// This logs **before** the query is executed (via [`QueryHook::before_query`]), so it works even
/// when monitoring is disabled (e.g. `InstrumentedClient` without `enable_monitoring()`).
///
/// Enable via the crate feature: `pgorm = { features = ["tracing"] }`.
#[cfg(feature = "tracing")]
#[derive(Debug, Clone)]
pub struct TracingSqlHook {
    /// Tracing event level to emit at.
    pub level: Level,
    /// Truncate long SQL strings (in chars). `None` means no truncation.
    pub max_sql_length: Option<usize>,
}

#[cfg(feature = "tracing")]
impl Default for TracingSqlHook {
    fn default() -> Self {
        Self {
            level: Level::DEBUG,
            max_sql_length: Some(200),
        }
    }
}

#[cfg(feature = "tracing")]
impl TracingSqlHook {
    /// Create a new hook with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the tracing event level.
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Set maximum SQL length to display.
    pub fn max_sql_length(mut self, len: usize) -> Self {
        self.max_sql_length = Some(len);
        self
    }

    /// Disable SQL truncation.
    pub fn no_truncate(mut self) -> Self {
        self.max_sql_length = None;
        self
    }

    fn truncate_sql(&self, sql: &str) -> String {
        match self.max_sql_length {
            Some(max) if sql.len() > max => format!("{}...", truncate_sql_bytes(sql, max)),
            _ => sql.to_string(),
        }
    }

    fn emit(&self, ctx: &QueryContext, exec_sql: &str, canonical_sql: Option<&str>) {
        let tag = ctx.tag.as_deref().unwrap_or("-");
        let fields = tracing::field::debug(&ctx.fields);
        match canonical_sql {
            Some(canonical_sql) => match self.level {
                Level::ERROR => tracing::error!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    canonical_sql = %canonical_sql,
                    fields = fields,
                ),
                Level::WARN => tracing::warn!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    canonical_sql = %canonical_sql,
                    fields = fields,
                ),
                Level::INFO => tracing::info!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    canonical_sql = %canonical_sql,
                    fields = fields,
                ),
                Level::DEBUG => tracing::debug!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    canonical_sql = %canonical_sql,
                    fields = fields,
                ),
                Level::TRACE => tracing::trace!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    canonical_sql = %canonical_sql,
                    fields = fields,
                ),
            },
            None => match self.level {
                Level::ERROR => tracing::error!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    fields = fields,
                ),
                Level::WARN => tracing::warn!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    fields = fields,
                ),
                Level::INFO => tracing::info!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    fields = fields,
                ),
                Level::DEBUG => tracing::debug!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    fields = fields,
                ),
                Level::TRACE => tracing::trace!(
                    target: "pgorm.sql",
                    query_type = ?ctx.query_type,
                    tag,
                    param_count = ctx.param_count,
                    sql = %exec_sql,
                    fields = fields,
                ),
            },
        }
    }
}

#[cfg(feature = "tracing")]
impl QueryHook for TracingSqlHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        let exec_sql = self.truncate_sql(&ctx.exec_sql);
        let canonical_sql =
            (ctx.exec_sql != ctx.canonical_sql).then(|| self.truncate_sql(&ctx.canonical_sql));
        self.emit(ctx, &exec_sql, canonical_sql.as_deref());
        HookAction::Continue
    }
}

/// A monitor that tracks query statistics.
#[derive(Debug)]
pub struct StatsMonitor {
    total_queries: std::sync::atomic::AtomicU64,
    failed_queries: std::sync::atomic::AtomicU64,
    total_duration_nanos: std::sync::atomic::AtomicU64,
    select_count: std::sync::atomic::AtomicU64,
    insert_count: std::sync::atomic::AtomicU64,
    update_count: std::sync::atomic::AtomicU64,
    delete_count: std::sync::atomic::AtomicU64,
    max_duration_nanos: std::sync::atomic::AtomicU64,
    slowest_query: std::sync::Mutex<Option<String>>,
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
        use std::sync::atomic::Ordering;

        QueryStats {
            total_queries: self.total_queries.load(Ordering::Relaxed),
            failed_queries: self.failed_queries.load(Ordering::Relaxed),
            total_duration: Duration::from_nanos(self.total_duration_nanos.load(Ordering::Relaxed)),
            select_count: self.select_count.load(Ordering::Relaxed),
            insert_count: self.insert_count.load(Ordering::Relaxed),
            update_count: self.update_count.load(Ordering::Relaxed),
            delete_count: self.delete_count.load(Ordering::Relaxed),
            max_duration: Duration::from_nanos(self.max_duration_nanos.load(Ordering::Relaxed)),
            slowest_query: self.slowest_query.lock().unwrap().clone(),
        }
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        use std::sync::atomic::Ordering;

        self.total_queries.store(0, Ordering::Relaxed);
        self.failed_queries.store(0, Ordering::Relaxed);
        self.total_duration_nanos.store(0, Ordering::Relaxed);
        self.select_count.store(0, Ordering::Relaxed);
        self.insert_count.store(0, Ordering::Relaxed);
        self.update_count.store(0, Ordering::Relaxed);
        self.delete_count.store(0, Ordering::Relaxed);
        self.max_duration_nanos.store(0, Ordering::Relaxed);
        *self.slowest_query.lock().unwrap() = None;
    }
}

impl Default for StatsMonitor {
    fn default() -> Self {
        Self {
            total_queries: std::sync::atomic::AtomicU64::new(0),
            failed_queries: std::sync::atomic::AtomicU64::new(0),
            total_duration_nanos: std::sync::atomic::AtomicU64::new(0),
            select_count: std::sync::atomic::AtomicU64::new(0),
            insert_count: std::sync::atomic::AtomicU64::new(0),
            update_count: std::sync::atomic::AtomicU64::new(0),
            delete_count: std::sync::atomic::AtomicU64::new(0),
            max_duration_nanos: std::sync::atomic::AtomicU64::new(0),
            slowest_query: std::sync::Mutex::new(None),
        }
    }
}

impl QueryMonitor for StatsMonitor {
    fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        use std::sync::atomic::Ordering;

        let duration_nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);

        self.total_queries.fetch_add(1, Ordering::Relaxed);
        let prev_total = self
            .total_duration_nanos
            .fetch_add(duration_nanos, Ordering::Relaxed);
        if prev_total.checked_add(duration_nanos).is_none() {
            // Saturate instead of wrapping on overflow (e.g. long-running, high-QPS services).
            self.total_duration_nanos.store(u64::MAX, Ordering::Relaxed);
        }

        match ctx.query_type {
            QueryType::Select => {
                self.select_count.fetch_add(1, Ordering::Relaxed);
            }
            QueryType::Insert => {
                self.insert_count.fetch_add(1, Ordering::Relaxed);
            }
            QueryType::Update => {
                self.update_count.fetch_add(1, Ordering::Relaxed);
            }
            QueryType::Delete => {
                self.delete_count.fetch_add(1, Ordering::Relaxed);
            }
            QueryType::Other => {}
        }

        if matches!(result, QueryResult::Error(_)) {
            self.failed_queries.fetch_add(1, Ordering::Relaxed);
        }

        // Update max duration + slowest query only when we actually become the new max.
        let mut current_max = self.max_duration_nanos.load(Ordering::Relaxed);
        while duration_nanos > current_max {
            match self.max_duration_nanos.compare_exchange_weak(
                current_max,
                duration_nanos,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    *self.slowest_query.lock().unwrap() = Some(ctx.canonical_sql.clone());
                    break;
                }
                Err(updated) => current_max = updated,
            }
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
    #[allow(clippy::should_implement_trait)]
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
    #[allow(clippy::should_implement_trait)]
    pub fn add<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hooks.push(Arc::new(hook));
        self
    }

    /// Add an Arc-wrapped hook.
    pub fn add_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hooks.push(hook);
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
                HookAction::ModifySql {
                    exec_sql,
                    canonical_sql,
                } => {
                    current_ctx.exec_sql = exec_sql;
                    if let Some(canonical_sql) = canonical_sql {
                        current_ctx.canonical_sql = canonical_sql;
                    }
                    current_ctx.query_type = QueryType::from_sql(&current_ctx.canonical_sql);
                }
                action @ HookAction::Abort(_) => return action,
            }
        }
        if current_ctx.exec_sql != ctx.exec_sql || current_ctx.canonical_sql != ctx.canonical_sql {
            HookAction::ModifySql {
                exec_sql: current_ctx.exec_sql,
                canonical_sql: (current_ctx.canonical_sql != ctx.canonical_sql)
                    .then_some(current_ctx.canonical_sql),
            }
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

/// Configuration for query monitoring and timeouts.
///
/// By default, monitoring is disabled and must be explicitly enabled.
#[derive(Debug, Clone, Default)]
pub struct MonitorConfig {
    /// Query timeout duration. `None` means no timeout (default).
    pub query_timeout: Option<Duration>,
    /// Slow query threshold for alerting.
    pub slow_query_threshold: Option<Duration>,
    /// Whether monitoring is enabled.
    pub monitoring_enabled: bool,
}

impl MonitorConfig {
    /// Create a new configuration with defaults (monitoring disabled, no timeout).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the query timeout duration.
    ///
    /// Queries exceeding this duration will be cancelled and return a timeout error.
    /// Default is `None` (no timeout).
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = Some(timeout);
        self
    }

    /// Set the slow query threshold.
    ///
    /// Queries exceeding this duration will trigger `on_slow_query` callbacks.
    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = Some(threshold);
        self
    }

    /// Enable monitoring.
    ///
    /// Monitoring must be explicitly enabled for monitors to receive events.
    pub fn enable_monitoring(mut self) -> Self {
        self.monitoring_enabled = true;
        self
    }

    /// Disable monitoring.
    pub fn disable_monitoring(mut self) -> Self {
        self.monitoring_enabled = false;
        self
    }
}

/// An instrumented database client that wraps a `GenericClient` with monitoring.
///
/// Monitoring must be explicitly enabled via `MonitorConfig::enable_monitoring()`.
pub struct InstrumentedClient<C> {
    client: C,
    monitor: Arc<dyn QueryMonitor>,
    hook: Option<Arc<dyn QueryHook>>,
    config: MonitorConfig,
}

impl<C: GenericClient> InstrumentedClient<C> {
    /// Create a new instrumented client with no monitoring.
    pub fn new(client: C) -> Self {
        Self {
            client,
            monitor: Arc::new(NoopMonitor),
            hook: None,
            config: MonitorConfig::default(),
        }
    }

    /// Set the monitor configuration.
    pub fn with_config(mut self, config: MonitorConfig) -> Self {
        self.config = config;
        self
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

    /// Set the slow query threshold.
    ///
    /// Queries taking longer than this will trigger `on_slow_query`.
    #[deprecated(
        since = "0.2.0",
        note = "Use `with_config(MonitorConfig::new().with_slow_query_threshold(...))` instead"
    )]
    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.config.slow_query_threshold = Some(threshold);
        self
    }

    /// Set the query timeout.
    ///
    /// Queries exceeding this duration will be cancelled.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.config.query_timeout = Some(timeout);
        self
    }

    /// Enable monitoring.
    pub fn enable_monitoring(mut self) -> Self {
        self.config.monitoring_enabled = true;
        self
    }

    /// Disable monitoring.
    pub fn disable_monitoring(mut self) -> Self {
        self.config.monitoring_enabled = false;
        self
    }

    /// Check if monitoring is enabled.
    pub fn is_monitoring_enabled(&self) -> bool {
        self.config.monitoring_enabled
    }

    /// Get the current configuration.
    pub fn config(&self) -> &MonitorConfig {
        &self.config
    }

    /// Get a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut MonitorConfig {
        &mut self.config
    }

    /// Get a reference to the inner client.
    pub fn inner(&self) -> &C {
        &self.client
    }

    /// Get the inner client, consuming this wrapper.
    pub fn into_inner(self) -> C {
        self.client
    }

    fn apply_hook(&self, ctx: &mut QueryContext) -> Result<(), OrmError> {
        let Some(hook) = &self.hook else {
            return Ok(());
        };

        match hook.before_query(ctx) {
            HookAction::Continue => Ok(()),
            HookAction::ModifySql {
                exec_sql,
                canonical_sql,
            } => {
                ctx.exec_sql = exec_sql;
                if let Some(canonical_sql) = canonical_sql {
                    ctx.canonical_sql = canonical_sql;
                }
                ctx.query_type = QueryType::from_sql(&ctx.canonical_sql);
                Ok(())
            }
            HookAction::Abort(reason) => Err(OrmError::validation(format!(
                "Query aborted by hook: {reason}"
            ))),
        }
    }

    fn report_result(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        if !self.config.monitoring_enabled {
            return;
        }

        if let Some(hook) = &self.hook {
            hook.after_query(ctx, duration, result);
        }

        self.monitor.on_query_complete(ctx, duration, result);

        if let Some(threshold) = self.config.slow_query_threshold {
            if duration > threshold {
                self.monitor.on_slow_query(ctx, duration);
            }
        }
    }

    async fn execute_with_timeout<T, F>(&self, future: F) -> OrmResult<T>
    where
        F: std::future::Future<Output = OrmResult<T>> + Send,
    {
        match self.config.query_timeout {
            Some(timeout) => {
                tokio::pin!(future);
                tokio::select! {
                    result = &mut future => result,
                    _ = tokio::time::sleep(timeout) => {
                        if let Some(cancel_token) = self.client.cancel_token() {
                            tokio::spawn(async move {
                                let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                            });
                        }
                        Err(OrmError::Timeout(timeout))
                    }
                }
            }
            None => future.await,
        }
    }

    async fn query_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Vec<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn query_one_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Row> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound { .. }) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn query_opt_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Option<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(Some(_)) => QueryResult::OptionalRow(true),
            Ok(None) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    async fn execute_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<u64> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(n) => QueryResult::Affected(*n),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }
}

impl<C: GenericClient> GenericClient for InstrumentedClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        self.query_inner(sql, params, None).await
    }

    async fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query_inner(sql, params, Some(tag)).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one_inner(sql, params, None).await
    }

    async fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        self.query_one_inner(sql, params, Some(tag)).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.query_opt_inner(sql, params, None).await
    }

    async fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt_inner(sql, params, Some(tag)).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute_inner(sql, params, None).await
    }

    async fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        self.execute_inner(sql, params, Some(tag)).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.client.cancel_token()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_type_detection() {
        assert_eq!(
            QueryType::from_sql("SELECT * FROM users"),
            QueryType::Select
        );
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
                HookAction::ModifySql {
                    exec_sql: format!("/* instrumented */ {}", ctx.exec_sql),
                    canonical_sql: None,
                }
            }
        }

        let hook = CompositeHook::new().add(AddCommentHook);
        let ctx = QueryContext::new("SELECT 1", 0);

        match hook.before_query(&ctx) {
            HookAction::ModifySql {
                exec_sql,
                canonical_sql,
            } => {
                assert_eq!(exec_sql, "/* instrumented */ SELECT 1");
                assert!(canonical_sql.is_none());
            }
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

    #[tokio::test]
    async fn tagged_queries_propagate_to_monitor() {
        #[derive(Default)]
        struct TagCapture(std::sync::Mutex<Option<String>>);

        impl QueryMonitor for TagCapture {
            fn on_query_complete(&self, ctx: &QueryContext, _: Duration, _: &QueryResult) {
                *self.0.lock().unwrap() = ctx.tag.clone();
            }
        }

        struct DummyClient;
        impl GenericClient for DummyClient {
            async fn query(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
                Ok(vec![])
            }
            async fn query_one(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
                Err(OrmError::not_found("no rows"))
            }
            async fn query_opt(
                &self,
                _: &str,
                _: &[&(dyn ToSql + Sync)],
            ) -> OrmResult<Option<Row>> {
                Ok(None)
            }
            async fn execute(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
                Ok(0)
            }
        }

        let capture = Arc::new(TagCapture::default());
        let client = InstrumentedClient::new(DummyClient)
            .with_config(MonitorConfig::new().enable_monitoring())
            .with_monitor_arc(capture.clone());

        client
            .query_tagged("test-tag", "SELECT 1", &[])
            .await
            .unwrap();

        assert_eq!(capture.0.lock().unwrap().as_deref(), Some("test-tag"));
    }

    #[tokio::test]
    async fn timeout_returns_error_and_attempts_cancellation() {
        struct HangingClient;
        impl GenericClient for HangingClient {
            async fn query(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(vec![])
            }
            async fn query_one(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
                Err(OrmError::not_found("unused"))
            }
            async fn query_opt(
                &self,
                _: &str,
                _: &[&(dyn ToSql + Sync)],
            ) -> OrmResult<Option<Row>> {
                Ok(None)
            }
            async fn execute(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
                Ok(0)
            }
        }

        let client = InstrumentedClient::new(HangingClient).with_config(
            MonitorConfig::new()
                .with_query_timeout(Duration::from_millis(10))
                .enable_monitoring(),
        );

        let err = client.query("SELECT pg_sleep(60)", &[]).await.unwrap_err();
        assert!(matches!(err, OrmError::Timeout(_)));
    }
}
