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
    ///
    /// For CTEs (`WITH ...`), looks past the CTE definitions to find the
    /// actual DML keyword (INSERT/UPDATE/DELETE/SELECT).
    pub fn from_sql(sql: &str) -> Self {
        use crate::sql::{starts_with_keyword, strip_sql_prefix};

        let trimmed = strip_sql_prefix(sql);
        if starts_with_keyword(trimmed, "SELECT") {
            QueryType::Select
        } else if starts_with_keyword(trimmed, "INSERT") {
            QueryType::Insert
        } else if starts_with_keyword(trimmed, "UPDATE") {
            QueryType::Update
        } else if starts_with_keyword(trimmed, "DELETE") {
            QueryType::Delete
        } else if starts_with_keyword(trimmed, "WITH") {
            // CTE: find the final DML keyword after the last top-level AS (...)
            Self::detect_cte_dml(trimmed)
        } else {
            QueryType::Other
        }
    }

    /// Detect the DML type inside a CTE (WITH ... SELECT/INSERT/UPDATE/DELETE).
    fn detect_cte_dml(sql: &str) -> Self {
        use crate::sql::starts_with_keyword;

        // Simple approach: scan for the last top-level DML keyword by tracking
        // parenthesis depth. The final statement follows the last closing paren
        // of the CTE definitions.
        let mut depth: i32 = 0;
        let mut last_top_level = 0;
        let bytes = sql.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        last_top_level = i + 1;
                    }
                }
                b'\'' => {
                    // Skip string literal
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\'' {
                            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                                i += 1; // escaped quote
                            } else {
                                break;
                            }
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        let remainder = sql[last_top_level..].trim_start();
        // Skip optional comma and CTE name after last paren (e.g. ", cte2 AS (...)")
        // The remainder should start with the DML keyword
        if starts_with_keyword(remainder, "INSERT") {
            QueryType::Insert
        } else if starts_with_keyword(remainder, "UPDATE") {
            QueryType::Update
        } else if starts_with_keyword(remainder, "DELETE") {
            QueryType::Delete
        } else {
            // Default: SELECT (covers plain WITH ... SELECT and edge cases)
            QueryType::Select
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

/// Maximum length for error messages in `QueryResult::Error`.
const MAX_ERROR_LEN: usize = 512;

/// Result of a query execution for monitoring purposes.
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// Query returned rows.
    Rows(usize),
    /// Query affected rows (for mutations).
    Affected(u64),
    /// Query returned a single optional row.
    OptionalRow(bool),
    /// Query failed with an error (truncated to 512 characters).
    Error(String),
}

impl QueryResult {
    /// Create an error result, truncating the message to avoid monitoring data explosion.
    pub fn error(msg: String) -> Self {
        if msg.len() > MAX_ERROR_LEN {
            let truncated = match msg.get(..MAX_ERROR_LEN) {
                Some(s) => s,
                None => {
                    // Find a valid UTF-8 boundary
                    let mut end = MAX_ERROR_LEN;
                    while !msg.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    &msg[..end]
                }
            };
            Self::Error(format!("{truncated}..."))
        } else {
            Self::Error(msg)
        }
    }
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
