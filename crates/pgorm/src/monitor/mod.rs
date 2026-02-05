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

mod config;
mod instrumented;
mod monitors;
mod stream;
mod types;

#[cfg(feature = "tracing")]
mod tracing_hook;

#[cfg(test)]
mod tests;

pub use config::MonitorConfig;
pub use instrumented::InstrumentedClient;
pub use monitors::{
    CompositeHook, CompositeMonitor, LoggingMonitor, NoopMonitor, QueryStats, StatsMonitor,
};
pub use types::{HookAction, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryType};

#[cfg(feature = "tracing")]
pub use tracing_hook::TracingSqlHook;

pub(crate) fn truncate_sql_bytes(sql: &str, max_bytes: usize) -> &str {
    if sql.len() <= max_bytes {
        return sql;
    }
    let mut end = max_bytes;
    while end > 0 && !sql.is_char_boundary(end) {
        end -= 1;
    }
    &sql[..end]
}
