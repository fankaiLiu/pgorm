use super::truncate_sql_bytes;
use super::types::{HookAction, QueryContext, QueryHook};
use tracing::Level;

/// A `tracing`-based debug hook that emits the SQL that will actually be executed.
///
/// This logs **before** the query is executed (via [`QueryHook::before_query`]), so it works even
/// when monitoring is disabled (e.g. `InstrumentedClient` without `enable_monitoring()`).
///
/// Enable via the crate feature: `pgorm = { features = ["tracing"] }`.
#[derive(Debug, Clone)]
pub struct TracingSqlHook {
    /// Tracing event level to emit at.
    pub level: Level,
    /// Truncate long SQL strings (in chars). `None` means no truncation.
    pub max_sql_length: Option<usize>,
}

impl Default for TracingSqlHook {
    fn default() -> Self {
        Self {
            level: Level::DEBUG,
            max_sql_length: Some(200),
        }
    }
}

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
        /// Dispatch a tracing event at a runtime-determined level.
        macro_rules! emit_at_level {
            ($level:expr, $($field:tt)*) => {
                match $level {
                    Level::ERROR => tracing::error!($($field)*),
                    Level::WARN  => tracing::warn!($($field)*),
                    Level::INFO  => tracing::info!($($field)*),
                    Level::DEBUG => tracing::debug!($($field)*),
                    Level::TRACE => tracing::trace!($($field)*),
                }
            };
        }

        let tag = ctx.tag.as_deref().unwrap_or("-");
        let fields = tracing::field::debug(&ctx.fields);
        match canonical_sql {
            Some(canonical_sql) => emit_at_level!(
                self.level,
                target: "pgorm.sql",
                query_type = ?ctx.query_type,
                tag,
                param_count = ctx.param_count,
                sql = %exec_sql,
                canonical_sql = %canonical_sql,
                fields = fields,
            ),
            None => emit_at_level!(
                self.level,
                target: "pgorm.sql",
                query_type = ?ctx.query_type,
                tag,
                param_count = ctx.param_count,
                sql = %exec_sql,
                fields = fields,
            ),
        }
    }
}

impl QueryHook for TracingSqlHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        let exec_sql = self.truncate_sql(&ctx.exec_sql);
        let canonical_sql =
            (ctx.exec_sql != ctx.canonical_sql).then(|| self.truncate_sql(&ctx.canonical_sql));
        self.emit(ctx, &exec_sql, canonical_sql.as_deref());
        HookAction::Continue
    }
}
