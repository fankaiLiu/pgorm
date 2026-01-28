//! # pgorm
//!
//! A lightweight Postgres-only ORM for Rust.
//!
//! ## Features
//!
//! - **SQL explicit**: SQL is a first-class citizen (use `query()` / `sql()` or the optional builders)
//! - **Type-safe mapping**: Row → Struct via `FromRow` trait
//! - **Minimal magic**: Traits and macros only for boilerplate reduction
//! - **Transaction-friendly**: pass a transaction anywhere a `GenericClient` is expected
//! - **Safe defaults**: DELETE requires WHERE, UPDATE requires SET
//! - **Query monitoring**: Built-in support for timing, logging, and hooking SQL execution
//! - **SQL checking**: Validate SQL against registered schemas and lint for common issues

pub mod client;
pub mod condition;
pub mod error;
pub mod monitor;
pub mod query;
pub mod row;
pub mod sql;
pub mod transaction;

pub use client::GenericClient;
pub use condition::{Condition, Op};
pub use error::{OrmError, OrmResult};
pub use monitor::{
    CompositeHook, CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor, MonitorConfig,
    NoopMonitor, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryStats, QueryType,
    StatsMonitor,
};
pub use query::query;
pub use row::{FromRow, PgType, RowExt};
pub use sql::{Sql, sql};

#[cfg(feature = "pool")]
pub mod pool;

#[cfg(feature = "pool")]
pub use client::PoolClient;

#[cfg(feature = "pool")]
pub use pool::{create_pool, create_pool_with_config};

#[cfg(feature = "builder")]
pub mod builder;

#[cfg(feature = "builder")]
pub use builder::{
    BuiltQuery, DeleteBuilder, InsertBuilder, MutationBuilder, QueryBuilder, SqlBuilder, Table,
    UpdateBuilder,
};

#[cfg(feature = "derive")]
pub use pgorm_derive::{FromRow, InsertModel, Model, UpdateModel, ViewModel};

// SQL checking and linting
pub mod check;

// Checked client with auto-registration
pub mod checked_client;

pub use check::{
    ColumnMeta, SchemaIssue, SchemaIssueKind, SchemaIssueLevel, SchemaRegistry, TableMeta,
    TableSchema,
};

// Re-export inventory for use by derive macros
pub use inventory;

// Re-export CheckedClient and related types
pub use checked_client::{CheckMode, ModelRegistration};

#[cfg(feature = "check")]
pub use checked_client::CheckedClient;

#[cfg(feature = "check")]
pub use check::{
    // Lint types and functions
    delete_has_where, detect_statement_kind, get_table_names, is_valid_sql, lint_select_many,
    lint_sql, select_has_limit, select_has_star, update_has_where, LintIssue, LintLevel,
    LintResult, ParseResult, StatementKind,
    // Database schema check
    check_sql, check_sql_cached, CheckClient, CheckError, CheckResult,
    DbSchema, SchemaCache, SchemaCacheConfig, SchemaCacheLoad,
    SqlCheckIssue, SqlCheckIssueKind, SqlCheckLevel,
    ColumnInfo, RelationKind, TableInfo,
};
