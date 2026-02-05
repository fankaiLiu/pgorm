// Re-export CheckMode from checked_client for public API
pub use crate::checked_client::CheckMode;

use crate::error::OrmError;
use std::time::Duration;

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
#[derive(Debug, Clone, Default)]
pub struct StatementCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
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

pub(crate) fn handle_dangerous_dml(
    policy: DangerousDmlPolicy,
    rule: &str,
    sql: &str,
) -> Result<(), OrmError> {
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
