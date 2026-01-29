//! pgorm-check
//!
//! Runtime helpers for checking whether SQL references match a live database schema.
//!
//! This crate can cache schema metadata into a local directory (default: `./.pgorm/`)
//! so subsequent runs can skip a full refresh when nothing has changed.
//!
//! # Features
//!
//! - **Schema validation**: Check if SQL queries reference valid tables and columns
//! - **SQL linting**: Detect common issues like DELETE without WHERE, SELECT without LIMIT
//! - **Syntax validation**: Verify SQL syntax is correct
//! - **Statement analysis**: Detect statement types, extract table names, etc.
//!
//! # Example
//!
//! ```ignore
//! use pgorm_check::{is_valid_sql, lint_sql, lint_select_many, delete_has_where};
//!
//! // Check SQL syntax
//! assert!(is_valid_sql("SELECT * FROM users").valid);
//!
//! // Lint for dangerous operations
//! let result = lint_sql("DELETE FROM users");
//! assert!(result.has_errors()); // Missing WHERE clause
//!
//! // Check select_many patterns
//! let result = lint_select_many("SELECT * FROM users");
//! assert!(result.has_warnings()); // Missing LIMIT
//!
//! // Individual checks
//! assert_eq!(delete_has_where("DELETE FROM users WHERE id = 1"), Some(true));
//! ```

pub mod client;
pub mod error;
pub mod schema_cache;
pub mod schema_introspect;

#[cfg(feature = "sql")]
pub mod sql_analysis;

#[cfg(feature = "sql")]
pub mod sql_check;

#[cfg(feature = "sql")]
pub mod sql_lint;

pub use client::{CheckClient, RowExt};
pub use error::{CheckError, CheckResult};
pub use schema_cache::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad};
pub use schema_introspect::{ColumnInfo, DbSchema, RelationKind, TableInfo};

#[cfg(feature = "sql")]
pub use sql_analysis::{
    ColumnRefFull, InsertAnalysis, OnConflictAnalysis, RangeVarRef, SqlAnalysis, SqlParseCache,
    TargetColumn, UpdateAnalysis, analyze_sql,
};

#[cfg(feature = "sql")]
pub use sql_check::{
    SqlCheckIssue, SqlCheckIssueKind, SqlCheckLevel, check_sql, check_sql_analysis, check_sql_cached,
};

#[cfg(feature = "sql")]
pub use sql_lint::{
    ColumnRef, LintIssue, LintLevel, LintResult, ParseResult, StatementKind, delete_has_where,
    detect_statement_kind, get_column_refs, get_table_names, is_valid_sql, lint_select_many,
    lint_sql, select_has_limit, select_has_star, update_has_where,
};
