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

use crate::{GenericClient, RowStream, StreamingClient};
use crate::check::{DbSchema, SchemaRegistry, TableMeta};
use crate::checked_client::ModelRegistration;
use crate::error::{OrmError, OrmResult};
#[cfg(feature = "tracing")]
use crate::monitor::TracingSqlHook;
use crate::monitor::{
    CompositeHook, LoggingMonitor, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryStats,
    QueryType, StatsMonitor,
};
use crate::row::FromRow;

// Re-export CheckMode from checked_client for public API
pub use crate::checked_client::CheckMode;

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use futures_core::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_postgres::Row;
use tokio_postgres::Statement;
use tokio_postgres::types::ToSql;

#[derive(Debug)]
struct StatementCache {
    inner: Mutex<StatementCacheInner>,
}

#[derive(Debug)]
struct StatementCacheInner {
    capacity: usize,
    map: HashMap<String, Statement>,
    order: VecDeque<String>,
}

impl StatementCache {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(StatementCacheInner {
                capacity,
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    fn get(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap();
        let stmt = inner.map.get(key).cloned()?;
        inner.touch(key);
        Some(stmt)
    }

    fn insert_if_absent(&self, key: String, stmt: Statement) -> Statement {
        let mut inner = self.inner.lock().unwrap();

        if let Some(existing) = inner.map.get(&key).cloned() {
            inner.touch(&key);
            return existing;
        }

        inner.map.insert(key.clone(), stmt.clone());
        inner.order.push_back(key);
        inner.evict_if_needed();
        stmt
    }

    fn remove(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap();
        let removed = inner.map.remove(key);
        if removed.is_some() {
            inner.remove_from_order(key);
        }
        removed
    }
}

impl StatementCacheInner {
    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k.as_str() == key) {
            if let Some(k) = self.order.remove(pos) {
                self.order.push_back(k);
            }
        }
    }

    fn remove_from_order(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k.as_str() == key) {
            let _ = self.order.remove(pos);
        }
    }

    fn evict_if_needed(&mut self) {
        if self.capacity == 0 {
            self.map.clear();
            self.order.clear();
            return;
        }

        while self.map.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            let _ = self.map.remove(&oldest);
        }
    }
}

#[derive(Debug)]
enum StmtCacheProbe {
    Disabled,
    Hit(Statement),
    Miss,
}

fn is_retryable_prepared_error(err: &OrmError) -> bool {
    let OrmError::Query(e) = err else {
        return false;
    };
    let Some(db_err) = e.as_db_error() else {
        return false;
    };

    match db_err.code().code() {
        // "cached plan must not change result type" (e.g. after schema change)
        "0A000" => db_err
            .message()
            .to_ascii_lowercase()
            .contains("cached plan must not change result type"),
        // invalid_sql_statement_name
        "26000" => true,
        _ => false,
    }
}

/// Result of checking a model against the database schema.
#[derive(Debug, Clone)]
pub struct ModelCheckResult {
    /// Model name
    pub model: &'static str,
    /// Table name the model maps to
    pub table: &'static str,
    /// Columns defined in the model
    pub model_columns: Vec<&'static str>,
    /// Columns found in the database (None if table not found)
    pub db_columns: Option<Vec<String>>,
    /// Missing columns (in model but not in DB)
    pub missing_in_db: Vec<&'static str>,
    /// Extra columns (in DB but not in model) - informational only
    pub extra_in_db: Vec<String>,
    /// Whether the table was found
    pub table_found: bool,
}

impl ModelCheckResult {
    /// Returns true if the model matches the database schema.
    pub fn is_valid(&self) -> bool {
        self.table_found && self.missing_in_db.is_empty()
    }

    /// Print a summary of the check result.
    pub fn print(&self) {
        if self.is_valid() {
            println!("  ✓ {} (table: {})", self.model, self.table);
        } else if !self.table_found {
            println!(
                "  ✗ {} - table '{}' not found in database",
                self.model, self.table
            );
        } else {
            println!(
                "  ✗ {} - missing columns: {:?}",
                self.model, self.missing_in_db
            );
        }
    }

    /// Check a model against a database schema.
    pub fn check<T: TableMeta>(db_schema: &DbSchema) -> Self {
        let table_name = T::table_name();
        let schema_name = T::schema_name();
        let model_columns: Vec<&'static str> = T::columns().to_vec();

        let db_table = db_schema.find_table(schema_name, table_name);

        match db_table {
            Some(table) => {
                let db_columns: Vec<String> =
                    table.columns.iter().map(|c| c.name.clone()).collect();

                let missing_in_db: Vec<&'static str> = model_columns
                    .iter()
                    .filter(|col| !db_columns.iter().any(|dc| dc == *col))
                    .copied()
                    .collect();

                let extra_in_db: Vec<String> = db_columns
                    .iter()
                    .filter(|col| !model_columns.contains(&col.as_str()))
                    .cloned()
                    .collect();

                ModelCheckResult {
                    model: std::any::type_name::<T>()
                        .rsplit("::")
                        .next()
                        .unwrap_or("Unknown"),
                    table: table_name,
                    model_columns,
                    db_columns: Some(db_columns),
                    missing_in_db,
                    extra_in_db,
                    table_found: true,
                }
            }
            None => ModelCheckResult {
                model: std::any::type_name::<T>()
                    .rsplit("::")
                    .next()
                    .unwrap_or("Unknown"),
                table: table_name,
                model_columns,
                db_columns: None,
                missing_in_db: vec![],
                extra_in_db: vec![],
                table_found: false,
            },
        }
    }
}

/// Configuration for `PgClient`.
#[derive(Debug, Clone)]
pub struct PgClientConfig {
    /// SQL check mode.
    pub check_mode: CheckMode,
    /// Runtime SQL safety policy (limit/where safeguards).
    pub sql_policy: SqlPolicy,
    /// Query timeout duration.
    pub query_timeout: Option<Duration>,
    /// Slow query threshold for alerting.
    pub slow_query_threshold: Option<Duration>,
    /// Prepared statement cache configuration (per-connection).
    pub statement_cache: StatementCacheConfig,
    /// Whether to collect query statistics.
    pub stats_enabled: bool,
    /// Whether to log queries.
    pub logging_enabled: bool,
    /// Minimum duration to log (filters out fast queries).
    pub log_min_duration: Option<Duration>,
}

/// Prepared statement cache configuration (per-connection).
#[derive(Debug, Clone)]
pub struct StatementCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
}

impl Default for StatementCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            capacity: 0,
        }
    }
}

impl Default for PgClientConfig {
    fn default() -> Self {
        Self {
            check_mode: CheckMode::WarnOnly,
            sql_policy: SqlPolicy::default(),
            query_timeout: None,
            slow_query_threshold: None,
            statement_cache: StatementCacheConfig::default(),
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

    /// Set the runtime SQL safety policy.
    pub fn sql_policy(mut self, policy: SqlPolicy) -> Self {
        self.sql_policy = policy;
        self
    }

    /// Configure how SELECT without LIMIT is handled.
    pub fn select_without_limit(mut self, policy: SelectWithoutLimitPolicy) -> Self {
        self.sql_policy.select_without_limit = policy;
        self
    }

    /// Configure how DELETE without WHERE is handled.
    pub fn delete_without_where(mut self, policy: DangerousDmlPolicy) -> Self {
        self.sql_policy.delete_without_where = policy;
        self
    }

    /// Configure how UPDATE without WHERE is handled.
    pub fn update_without_where(mut self, policy: DangerousDmlPolicy) -> Self {
        self.sql_policy.update_without_where = policy;
        self
    }

    /// Configure how TRUNCATE is handled.
    pub fn truncate_policy(mut self, policy: DangerousDmlPolicy) -> Self {
        self.sql_policy.truncate = policy;
        self
    }

    /// Configure how DROP TABLE is handled.
    pub fn drop_table_policy(mut self, policy: DangerousDmlPolicy) -> Self {
        self.sql_policy.drop_table = policy;
        self
    }

    /// Enable strict SQL checking.
    ///
    /// This only affects runtime SQL checking behavior (schema/lint/policy). It does **not**
    /// change `fetch_one/query_one` row-count semantics; use `*_strict` APIs if you need
    /// "exactly one row" enforcement.
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

    /// Enable prepared statement caching with a per-connection capacity.
    ///
    /// Note: prepared statements are per-connection; use a conservative capacity to avoid
    /// unbounded memory/state growth for highly dynamic SQL.
    pub fn statement_cache(mut self, cap: usize) -> Self {
        self.statement_cache = StatementCacheConfig {
            enabled: cap > 0,
            capacity: cap,
        };
        self
    }

    /// Disable prepared statement caching.
    pub fn no_statement_cache(mut self) -> Self {
        self.statement_cache.enabled = false;
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

/// Policy for runtime SQL safety rules.
#[derive(Debug, Clone)]
pub struct SqlPolicy {
    pub select_without_limit: SelectWithoutLimitPolicy,
    pub delete_without_where: DangerousDmlPolicy,
    pub update_without_where: DangerousDmlPolicy,
    pub truncate: DangerousDmlPolicy,
    pub drop_table: DangerousDmlPolicy,
}

impl Default for SqlPolicy {
    fn default() -> Self {
        Self {
            select_without_limit: SelectWithoutLimitPolicy::Allow,
            delete_without_where: DangerousDmlPolicy::Allow,
            update_without_where: DangerousDmlPolicy::Allow,
            truncate: DangerousDmlPolicy::Allow,
            drop_table: DangerousDmlPolicy::Allow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerousDmlPolicy {
    Allow,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectWithoutLimitPolicy {
    Allow,
    Warn,
    Error,
    /// Automatically add a LIMIT if the top-level SELECT has no LIMIT/OFFSET.
    AutoLimit(i32),
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

        let statement_cache = (config.statement_cache.enabled && config.statement_cache.capacity > 0)
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

impl<C: GenericClient> PgClient<C> {
    #[cfg(not(feature = "tracing"))]
    fn emit_tracing_sql(&self, _ctx: &QueryContext) {}

    #[cfg(feature = "tracing")]
    fn emit_tracing_sql(&self, ctx: &QueryContext) {
        if let Some(hook) = &self.tracing_sql_hook {
            let _ = hook.before_query(ctx);
        }
    }

    fn apply_sql_policy(&self, ctx: &mut QueryContext) -> OrmResult<()> {
        use crate::StatementKind;

        let policy = &self.config.sql_policy;
        // Fast path: default policy is "Allow" everywhere, so avoid parsing/analyzing SQL.
        if policy.select_without_limit == SelectWithoutLimitPolicy::Allow
            && policy.delete_without_where == DangerousDmlPolicy::Allow
            && policy.update_without_where == DangerousDmlPolicy::Allow
            && policy.truncate == DangerousDmlPolicy::Allow
            && policy.drop_table == DangerousDmlPolicy::Allow
        {
            return Ok(());
        }

        let analysis = self.registry.analyze_sql(&ctx.canonical_sql);

        if !analysis.parse_result.valid {
            // Leave parse errors to schema checks or database errors depending on configuration.
            return Ok(());
        }

        match analysis.statement_kind {
            Some(StatementKind::Select) => {
                if analysis.select_has_limit == Some(false) {
                    match policy.select_without_limit {
                        SelectWithoutLimitPolicy::Allow => {}
                        SelectWithoutLimitPolicy::Warn => {
                            eprintln!(
                                "[pgorm warn] SQL policy: SELECT without LIMIT/OFFSET: {}",
                                ctx.canonical_sql
                            );
                        }
                        SelectWithoutLimitPolicy::Error => {
                            return Err(OrmError::validation(format!(
                                "SQL policy violation: SELECT without LIMIT/OFFSET: {}",
                                ctx.canonical_sql
                            )));
                        }
                        SelectWithoutLimitPolicy::AutoLimit(limit) => {
                            let old_canonical = ctx.canonical_sql.clone();
                            match pgorm_check::ensure_select_limit(&old_canonical, limit) {
                                Ok(Some(new_sql)) => {
                                    ctx.canonical_sql = new_sql.clone();
                                    ctx.query_type = QueryType::from_sql(&ctx.canonical_sql);

                                    if ctx.exec_sql == old_canonical {
                                        ctx.exec_sql = new_sql;
                                    } else if let Some(pos) = ctx.exec_sql.rfind(&old_canonical) {
                                        let mut rewritten = String::with_capacity(
                                            ctx.exec_sql.len() - old_canonical.len()
                                                + ctx.canonical_sql.len(),
                                        );
                                        rewritten.push_str(&ctx.exec_sql[..pos]);
                                        rewritten.push_str(&ctx.canonical_sql);
                                        rewritten
                                            .push_str(&ctx.exec_sql[pos + old_canonical.len()..]);
                                        ctx.exec_sql = rewritten;
                                    } else {
                                        // Fallback: drop exec_sql modifications (e.g. comments) to ensure LIMIT is applied.
                                        ctx.exec_sql = ctx.canonical_sql.clone();
                                    }
                                }
                                Ok(None) => {
                                    // Shouldn't happen if analysis says no limit; treat as unsupported rewrite.
                                    return Err(OrmError::validation(format!(
                                        "SQL policy rewrite failed: unable to add LIMIT to: {}",
                                        ctx.canonical_sql
                                    )));
                                }
                                Err(e) => return Err(OrmError::validation(e.to_string())),
                            }
                        }
                    }
                }
            }
            Some(StatementKind::Delete) => {
                if analysis.delete_has_where == Some(false) {
                    handle_dangerous_dml(
                        policy.delete_without_where,
                        "DELETE without WHERE",
                        &ctx.canonical_sql,
                    )?;
                }
            }
            Some(StatementKind::Update) => {
                if analysis.update_has_where == Some(false) {
                    handle_dangerous_dml(
                        policy.update_without_where,
                        "UPDATE without WHERE",
                        &ctx.canonical_sql,
                    )?;
                }
            }
            Some(StatementKind::Truncate) => {
                handle_dangerous_dml(policy.truncate, "TRUNCATE", &ctx.canonical_sql)?;
            }
            Some(StatementKind::DropTable) => {
                handle_dangerous_dml(policy.drop_table, "DROP TABLE", &ctx.canonical_sql)?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Load the database schema from PostgreSQL.
    ///
    /// This queries the database catalog to get actual table and column information.
    /// By default, only the "public" schema is loaded.
    pub async fn load_db_schema(&self) -> OrmResult<DbSchema> {
        self.load_db_schema_for(&["public".to_string()]).await
    }

    /// Load the database schema for specific schemas.
    pub async fn load_db_schema_for(&self, schemas: &[String]) -> OrmResult<DbSchema> {
        // Query to get all tables and columns
        let rows = self
            .client
            .query(
                r#"
SELECT
  n.nspname AS schema_name,
  c.relname AS table_name,
  c.relkind AS relkind,
  a.attname AS column_name,
  a.attnum::integer AS ordinal,
  pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
  a.attnotnull AS not_null,
  pg_get_expr(ad.adbin, ad.adrelid) AS default_expr
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
  AND a.attnum > 0
  AND NOT a.attisdropped
  AND n.nspname = ANY($1::text[])
ORDER BY n.nspname, c.relname, a.attnum
"#,
                &[&schemas],
            )
            .await?;

        use crate::check::{ColumnInfo, RelationKind, TableInfo};
        use std::collections::BTreeMap;

        let mut tables: BTreeMap<(String, String), TableInfo> = BTreeMap::new();

        for row in rows {
            let schema_name: String = row.get("schema_name");
            let table_name: String = row.get("table_name");
            let relkind: i8 = row.get("relkind");

            let column_name: String = row.get("column_name");
            let ordinal: i32 = row.get("ordinal");
            let data_type: String = row.get("data_type");
            let not_null: bool = row.get("not_null");
            let default_expr: Option<String> = row.get("default_expr");

            let kind = match relkind as u8 as char {
                'r' => RelationKind::Table,
                'p' => RelationKind::PartitionedTable,
                'v' => RelationKind::View,
                'm' => RelationKind::MaterializedView,
                'f' => RelationKind::ForeignTable,
                _ => RelationKind::Other,
            };

            let key = (schema_name.clone(), table_name.clone());

            let table = tables.entry(key).or_insert_with(|| TableInfo {
                schema: schema_name,
                name: table_name,
                kind,
                columns: Vec::new(),
            });

            table.columns.push(ColumnInfo {
                name: column_name,
                data_type,
                not_null,
                default_expr,
                ordinal,
            });
        }

        let tables = tables.into_values().collect::<Vec<_>>();

        Ok(DbSchema {
            schemas: schemas.to_vec(),
            tables,
        })
    }

    /// Check a single model against the database schema.
    ///
    /// Compares the model's columns with the actual database table.
    pub async fn check_model<T: TableMeta>(&self) -> OrmResult<ModelCheckResult> {
        let db_schema = self.load_db_schema().await?;
        Ok(ModelCheckResult::check::<T>(&db_schema))
    }

    /// Check SQL against the registry.
    fn check_sql(&self, sql: &str) -> OrmResult<()> {
        match self.config.check_mode {
            CheckMode::Disabled => Ok(()),
            CheckMode::WarnOnly => {
                let issues = self.registry.check_sql(sql);
                for issue in issues {
                    eprintln!("[pgorm warn] SQL check: {issue}");
                }
                Ok(())
            }
            CheckMode::Strict => {
                let issues = self.registry.check_sql(sql);
                let errors: Vec<_> = issues
                    .iter()
                    .filter(|i| i.level == crate::SchemaIssueLevel::Error)
                    .collect();
                if errors.is_empty() {
                    Ok(())
                } else {
                    let messages: Vec<String> = errors.iter().map(|i| i.message.clone()).collect();
                    Err(OrmError::validation(format!(
                        "SQL check failed: {}",
                        messages.join("; ")
                    )))
                }
            }
        }
    }

    /// Process hook before query.
    fn apply_hook(&self, ctx: &mut QueryContext) -> Result<(), OrmError> {
        use crate::monitor::HookAction;

        if let Some(hook) = &self.hook {
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
        } else {
            Ok(())
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

fn handle_dangerous_dml(policy: DangerousDmlPolicy, rule: &str, sql: &str) -> Result<(), OrmError> {
    match policy {
        DangerousDmlPolicy::Allow => Ok(()),
        DangerousDmlPolicy::Warn => {
            eprintln!("[pgorm warn] SQL policy: {rule}: {sql}");
            Ok(())
        }
        DangerousDmlPolicy::Error => Err(OrmError::validation(format!(
            "SQL policy violation: {rule}: {sql}"
        ))),
    }
}

impl<C: GenericClient> PgClient<C> {
    /// Execute with timeout if configured.
    async fn execute_with_timeout<T, F>(&self, future: F) -> OrmResult<T>
    where
        F: std::future::Future<Output = OrmResult<T>> + Send,
    {
        match self.config.query_timeout {
            Some(timeout) => tokio::time::timeout(timeout, future).await.map_err(|_| {
                if let Some(cancel_token) = self.client.cancel_token() {
                    tokio::spawn(async move {
                        let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                    });
                }
                OrmError::Timeout(timeout)
            })?,
            None => future.await,
        }
    }
}

// ============================================================================
// Dynamic SQL execution methods
// ============================================================================

impl<C: GenericClient> PgClient<C> {
    /// Execute a dynamic SQL query and return all rows mapped to type T.
    ///
    /// This method is monitored and uses the same configuration as the `PgClient`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users: Vec<User> = pg.sql_query_as(
    ///     "SELECT * FROM users WHERE status = $1",
    ///     &[&"active"]
    /// ).await?;
    /// ```
    pub async fn sql_query_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<T>> {
        let rows = self.query(sql, params).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute a dynamic SQL query and return exactly one row mapped to type T.
    ///
    /// Returns an error if zero or more than one row is returned.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let user: User = pg.sql_query_one_as(
    ///     "SELECT * FROM users WHERE id = $1",
    ///     &[&user_id]
    /// ).await?;
    /// ```
    pub async fn sql_query_one_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<T> {
        let row = self.query_one(sql, params).await?;
        T::from_row(&row)
    }

    /// Execute a dynamic SQL query and return at most one row mapped to type T.
    ///
    /// Returns `Ok(None)` if no rows are found.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let user: Option<User> = pg.sql_query_opt_as(
    ///     "SELECT * FROM users WHERE email = $1",
    ///     &[&email]
    /// ).await?;
    /// ```
    pub async fn sql_query_opt_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<T>> {
        let row = self.query_opt(sql, params).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute a dynamic SQL statement and return the number of affected rows.
    ///
    /// Use this for INSERT, UPDATE, DELETE statements.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let count = pg.sql_execute(
    ///     "UPDATE users SET status = $1 WHERE last_login < $2",
    ///     &[&"inactive", &cutoff_date]
    /// ).await?;
    /// ```
    pub async fn sql_execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute(sql, params).await
    }

    /// Execute a dynamic SQL query and return all raw rows.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let rows = pg.sql_query(
    ///     "SELECT id, name FROM users WHERE status = $1",
    ///     &[&"active"]
    /// ).await?;
    /// ```
    pub async fn sql_query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query(sql, params).await
    }

    /// Execute a dynamic SQL query and return exactly one raw row.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let row = pg.sql_query_one(
    ///     "SELECT * FROM users WHERE id = $1",
    ///     &[&user_id]
    /// ).await?;
    /// ```
    pub async fn sql_query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one(sql, params).await
    }

    /// Execute a dynamic SQL query and return at most one raw row.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let row = pg.sql_query_opt(
    ///     "SELECT * FROM users WHERE email = $1",
    ///     &[&email]
    /// ).await?;
    /// ```
    pub async fn sql_query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt(sql, params).await
    }
}

impl<C: GenericClient> GenericClient for PgClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        self.query_impl(None, sql, params).await
    }

    async fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query_impl(Some(tag), sql, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one_impl(None, sql, params).await
    }

    async fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        self.query_one_impl(Some(tag), sql, params).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.query_opt_impl(None, sql, params).await
    }

    async fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt_impl(Some(tag), sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute_impl(None, sql, params).await
    }

    async fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        self.execute_impl(Some(tag), sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.client.cancel_token()
    }
}

impl<C: GenericClient> PgClient<C> {
    fn probe_stmt_cache(&self, ctx: &QueryContext) -> StmtCacheProbe {
        if !self.config.statement_cache.enabled {
            return StmtCacheProbe::Disabled;
        }
        let Some(cache) = &self.statement_cache else {
            return StmtCacheProbe::Disabled;
        };
        if !self.client.supports_prepared_statements() {
            return StmtCacheProbe::Disabled;
        }
        // Only use canonical_sql as cache key when it matches the executed SQL.
        if ctx.exec_sql != ctx.canonical_sql {
            return StmtCacheProbe::Disabled;
        }

        match cache.get(&ctx.canonical_sql) {
            Some(stmt) => StmtCacheProbe::Hit(stmt),
            None => StmtCacheProbe::Miss,
        }
    }

    async fn query_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        // Process hook first, then check the canonical SQL.
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;

        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields.insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields.insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.query_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_prepared(&stmt, params))
                            .await
                    }
                    None => self
                        .execute_with_timeout(self.client.query(&ctx.exec_sql, params))
                        .await,
                }
            }
        };
        let duration = start.elapsed();

        // Report
        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    async fn query_one_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields.insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields.insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                            .await
                    }
                    None => self
                        .execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
                        .await,
                }
            }
        };
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound(_)) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    async fn query_opt_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields.insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields.insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_opt_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(
                                    self.client.query_opt_prepared(&stmt, params),
                                )
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_opt_prepared(&stmt, params))
                            .await
                    }
                    None => self
                        .execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
                        .await,
                }
            }
        };
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

    async fn execute_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields.insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields.insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.execute_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.execute_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.execute_prepared(&stmt, params))
                            .await
                    }
                    None => self
                        .execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
                        .await,
                }
            }
        };
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

#[derive(Clone)]
struct PgClientStreamReporter {
    stats: Arc<StatsMonitor>,
    logging_monitor: Option<LoggingMonitor>,
    custom_monitor: Option<Arc<dyn QueryMonitor>>,
    hook: Option<Arc<dyn QueryHook>>,
    config: PgClientConfig,
}

impl PgClientStreamReporter {
    fn report(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
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

struct PgClientRowStream {
    inner: RowStream,
    reporter: PgClientStreamReporter,
    ctx: QueryContext,
    start: Instant,
    rows: usize,
    cancel_token: Option<tokio_postgres::CancelToken>,
    timeout_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    finished: bool,
    terminated: bool,
}

impl PgClientRowStream {
    fn new(
        inner: RowStream,
        reporter: PgClientStreamReporter,
        mut ctx: QueryContext,
        start: Instant,
        cancel_token: Option<tokio_postgres::CancelToken>,
        timeout_remaining: Option<Duration>,
    ) -> Self {
        ctx.fields.insert("stream".to_string(), "true".to_string());

        let timeout_sleep = timeout_remaining.map(|d| Box::pin(tokio::time::sleep(d)));

        Self {
            inner,
            reporter,
            ctx,
            start,
            rows: 0,
            cancel_token,
            timeout_sleep,
            finished: false,
            terminated: false,
        }
    }

    fn finalize(&mut self, dropped: bool, err: Option<&OrmError>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.ctx
            .fields
            .insert("stream_dropped".to_string(), dropped.to_string());

        let duration = self.start.elapsed();
        let query_result = match err {
            None => QueryResult::Rows(self.rows),
            Some(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Some(e) => QueryResult::Error(e.to_string()),
        };
        self.reporter.report(&self.ctx, duration, &query_result);
    }
}

impl Stream for PgClientRowStream {
    type Item = OrmResult<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }

        if let (Some(timeout), Some(sleep)) = (self.reporter.config.query_timeout, self.timeout_sleep.as_mut())
        {
            if Pin::new(sleep).poll(cx).is_ready() {
                self.timeout_sleep = None;
                self.terminated = true;

                if let Some(cancel_token) = self.cancel_token.take() {
                    tokio::spawn(async move {
                        let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                    });
                }

                let err = OrmError::Timeout(timeout);
                self.finalize(false, Some(&err));
                return Poll::Ready(Some(Err(err)));
            }
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(row))) => {
                self.rows += 1;
                Poll::Ready(Some(Ok(row)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.terminated = true;
                self.finalize(false, Some(&e));
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.terminated = true;
                self.finalize(false, None);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for PgClientRowStream {
    fn drop(&mut self) {
        if self.terminated {
            return;
        }
        self.finalize(true, None);
    }
}

impl<C: GenericClient> PgClient<C>
where
    C: StreamingClient,
{
    async fn query_stream_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        // Process hook first, then check the canonical SQL.
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_stream(&ctx.exec_sql, params))
            .await;

        match result {
            Ok(stream) => {
                let needs_wrap = self.config.query_timeout.is_some()
                    || self.config.stats_enabled
                    || self.logging_monitor.is_some()
                    || self.custom_monitor.is_some()
                    || self.hook.is_some()
                    || self.config.slow_query_threshold.is_some();

                if !needs_wrap {
                    return Ok(stream);
                }

                let timeout_remaining = self
                    .config
                    .query_timeout
                    .map(|t| t.saturating_sub(start.elapsed()));

                let reporter = PgClientStreamReporter {
                    stats: self.stats.clone(),
                    logging_monitor: self.logging_monitor.clone(),
                    custom_monitor: self.custom_monitor.clone(),
                    hook: self.hook.clone(),
                    config: self.config.clone(),
                };

                Ok(RowStream::new(PgClientRowStream::new(
                    stream,
                    reporter,
                    ctx,
                    start,
                    self.client.cancel_token(),
                    timeout_remaining,
                )))
            }
            Err(e) => {
                let mut ctx = ctx;
                ctx.fields.insert("stream".to_string(), "true".to_string());

                let duration = start.elapsed();
                let query_result = match &e {
                    OrmError::Timeout(d) => QueryResult::Error(format!("timeout after {d:?}")),
                    other => QueryResult::Error(other.to_string()),
                };
                self.report_result(&ctx, duration, &query_result);
                Err(e)
            }
        }
    }
}

impl<C: GenericClient + StreamingClient> StreamingClient for PgClient<C> {
    async fn query_stream(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<RowStream> {
        self.query_stream_impl(None, sql, params).await
    }

    async fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.query_stream_impl(Some(tag), sql, params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::Row;
    use tokio_postgres::types::ToSql;

    #[test]
    fn test_config_defaults() {
        let config = PgClientConfig::default();
        assert_eq!(config.check_mode, CheckMode::WarnOnly);
        assert_eq!(
            config.sql_policy.select_without_limit,
            SelectWithoutLimitPolicy::Allow
        );
        assert_eq!(
            config.sql_policy.delete_without_where,
            DangerousDmlPolicy::Allow
        );
        assert_eq!(
            config.sql_policy.update_without_where,
            DangerousDmlPolicy::Allow
        );
        assert!(config.stats_enabled);
        assert!(!config.logging_enabled);
        assert!(!config.statement_cache.enabled);
        assert_eq!(config.statement_cache.capacity, 0);
    }

    #[test]
    fn test_config_builder() {
        let config = PgClientConfig::new()
            .strict()
            .timeout(Duration::from_secs(30))
            .with_logging()
            .statement_cache(64);

        assert_eq!(config.check_mode, CheckMode::Strict);
        assert_eq!(config.query_timeout, Some(Duration::from_secs(30)));
        assert!(config.logging_enabled);
        assert!(config.statement_cache.enabled);
        assert_eq!(config.statement_cache.capacity, 64);
    }

    #[test]
    fn test_config_no_statement_cache() {
        let config = PgClientConfig::new().statement_cache(16).no_statement_cache();
        assert!(!config.statement_cache.enabled);
    }

    #[tokio::test]
    async fn sql_policy_select_without_limit_errors() {
        #[derive(Default)]
        struct Capture(std::sync::Mutex<Option<String>>);

        #[derive(Clone)]
        struct DummyClient(std::sync::Arc<Capture>);

        impl GenericClient for DummyClient {
            async fn query(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
                *self.0.0.lock().unwrap() = Some(sql.to_string());
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

        let capture = std::sync::Arc::new(Capture::default());
        let config = PgClientConfig::new()
            .no_check()
            .select_without_limit(SelectWithoutLimitPolicy::Error);
        let pg = PgClient::with_config(DummyClient(capture.clone()), config);

        let err = pg.query("SELECT * FROM users", &[]).await.unwrap_err();
        assert!(matches!(err, OrmError::Validation(_)));
        assert!(capture.0.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn sql_policy_select_without_limit_auto_limit_rewrites_exec_sql() {
        #[derive(Default)]
        struct Capture(std::sync::Mutex<Option<String>>);

        #[derive(Clone)]
        struct DummyClient(std::sync::Arc<Capture>);

        impl GenericClient for DummyClient {
            async fn query(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
                *self.0.0.lock().unwrap() = Some(sql.to_string());
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

        let capture = std::sync::Arc::new(Capture::default());
        let config = PgClientConfig::new()
            .no_check()
            .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(10));
        let pg = PgClient::with_config(DummyClient(capture.clone()), config);

        pg.query("SELECT * FROM users", &[]).await.unwrap();

        let executed = capture.0.lock().unwrap().clone().unwrap();
        assert!(executed.to_uppercase().contains("LIMIT 10"));
    }

    #[tokio::test]
    async fn sql_policy_delete_without_where_errors() {
        #[derive(Default)]
        struct Capture(std::sync::Mutex<Option<String>>);

        #[derive(Clone)]
        struct DummyClient(std::sync::Arc<Capture>);

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
            async fn execute(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
                *self.0.0.lock().unwrap() = Some(sql.to_string());
                Ok(0)
            }
        }

        let capture = std::sync::Arc::new(Capture::default());
        let config = PgClientConfig::new()
            .no_check()
            .delete_without_where(DangerousDmlPolicy::Error);
        let pg = PgClient::with_config(DummyClient(capture.clone()), config);

        let err = pg.execute("DELETE FROM users", &[]).await.unwrap_err();
        assert!(matches!(err, OrmError::Validation(_)));
        assert!(capture.0.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn tagged_queries_propagate_to_custom_monitor() {
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

        let capture = std::sync::Arc::new(TagCapture::default());
        let pg = PgClient::with_config(DummyClient, PgClientConfig::new().no_check())
            .with_monitor_arc(capture.clone());

        pg.query_tagged("test-tag", "SELECT 1", &[]).await.unwrap();

        assert_eq!(capture.0.lock().unwrap().as_deref(), Some("test-tag"));
    }
}
