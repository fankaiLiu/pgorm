use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

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
        use crate::sql::{starts_with_keyword, strip_sql_prefix};

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
