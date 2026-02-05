use super::truncate_sql_bytes;
use super::types::{HookAction, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryType};
use std::sync::Arc;
use std::time::Duration;

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

    pub(crate) fn truncate_sql(&self, sql: &str) -> String {
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
    stmt_cache_hits: std::sync::atomic::AtomicU64,
    stmt_cache_misses: std::sync::atomic::AtomicU64,
    stmt_prepare_count: std::sync::atomic::AtomicU64,
    stmt_prepare_duration_nanos: std::sync::atomic::AtomicU64,
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
    /// Prepared statement cache hits.
    pub stmt_cache_hits: u64,
    /// Prepared statement cache misses.
    pub stmt_cache_misses: u64,
    /// Number of statement prepares performed (misses + retries).
    pub stmt_prepare_count: u64,
    /// Total time spent preparing statements.
    pub stmt_prepare_duration: Duration,
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
            stmt_cache_hits: self.stmt_cache_hits.load(Ordering::Relaxed),
            stmt_cache_misses: self.stmt_cache_misses.load(Ordering::Relaxed),
            stmt_prepare_count: self.stmt_prepare_count.load(Ordering::Relaxed),
            stmt_prepare_duration: Duration::from_nanos(
                self.stmt_prepare_duration_nanos.load(Ordering::Relaxed),
            ),
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
        self.stmt_cache_hits.store(0, Ordering::Relaxed);
        self.stmt_cache_misses.store(0, Ordering::Relaxed);
        self.stmt_prepare_count.store(0, Ordering::Relaxed);
        self.stmt_prepare_duration_nanos.store(0, Ordering::Relaxed);
    }

    /// Record a prepared statement cache hit.
    pub fn on_stmt_cache_hit(&self) {
        use std::sync::atomic::Ordering;
        self.stmt_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a prepared statement cache miss.
    pub fn on_stmt_cache_miss(&self) {
        use std::sync::atomic::Ordering;
        self.stmt_cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a statement prepare operation and its duration.
    pub fn on_stmt_prepare(&self, duration: Duration) {
        use std::sync::atomic::Ordering;

        let nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        self.stmt_prepare_count.fetch_add(1, Ordering::Relaxed);

        let prev = self
            .stmt_prepare_duration_nanos
            .fetch_add(nanos, Ordering::Relaxed);
        if prev.checked_add(nanos).is_none() {
            self.stmt_prepare_duration_nanos
                .store(u64::MAX, Ordering::Relaxed);
        }
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
            stmt_cache_hits: std::sync::atomic::AtomicU64::new(0),
            stmt_cache_misses: std::sync::atomic::AtomicU64::new(0),
            stmt_prepare_count: std::sync::atomic::AtomicU64::new(0),
            stmt_prepare_duration_nanos: std::sync::atomic::AtomicU64::new(0),
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
