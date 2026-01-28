//! # pgorm
//!
//! A lightweight Postgres-only ORM for Rust.
//!
//! ## Features
//!
//! - **SQL explicit**: SQL is a first-class citizen (use `query()` / `sql()` or the query builders)
//! - **Type-safe mapping**: Row → Struct via `FromRow` trait
//! - **Minimal magic**: Traits and macros only for boilerplate reduction
//! - **Transaction-friendly**: pass a transaction anywhere a `GenericClient` is expected
//! - **Safe defaults**: DELETE requires WHERE, UPDATE requires SET
//! - **Query monitoring**: Built-in support for timing, logging, and hooking SQL execution
//! - **SQL checking**: Validate SQL against registered schemas and lint for common issues
//!
//! ## Query Builder (qb)
//!
//! The `qb` module provides a unified query builder system:
//!
//! ```ignore
//! use pgorm::qb;
//!
//! // SELECT
//! let users = qb::select("users")
//!     .eq("status", "active")
//!     .order_by("created_at DESC")
//!     .limit(10)
//!     .fetch_all::<User>(&client)
//!     .await?;
//!
//! // INSERT
//! qb::insert("users")
//!     .set("username", "alice")
//!     .set("email", "alice@example.com")
//!     .execute(&client)
//!     .await?;
//!
//! // UPDATE
//! qb::update("users")
//!     .set("status", "inactive")
//!     .eq("id", user_id)
//!     .execute(&client)
//!     .await?;
//!
//! // DELETE
//! qb::delete("users")
//!     .eq("id", user_id)
//!     .execute(&client)
//!     .await?;
//! ```

pub mod client;
pub mod condition;
pub mod error;
pub mod monitor;
pub mod qb;
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

// Re-export qb module for easy access
pub use qb::{
    delete, delete_from, insert, insert_into, select, select_from, update,
    DeleteQb, Expr, ExprGroup, InsertQb, MutationQb, SelectQb, SqlQb, UpdateQb,
};

#[cfg(feature = "pool")]
pub mod pool;

#[cfg(feature = "pool")]
pub use client::PoolClient;

#[cfg(feature = "pool")]
pub use pool::{create_pool, create_pool_with_config};

#[cfg(feature = "derive")]
pub use pgorm_derive::{FromRow, InsertModel, Model, UpdateModel, ViewModel};

// SQL checking and linting
pub mod check;

// Checked client with auto-registration (lower-level)
pub mod checked_client;

// Unified PgClient (recommended)
#[cfg(feature = "check")]
pub mod pg_client;

pub use check::{
    ColumnMeta, SchemaIssue, SchemaIssueKind, SchemaIssueLevel, SchemaRegistry, TableMeta,
    TableSchema,
};

// Re-export inventory for use by derive macros
pub use inventory;

// Re-export CheckedClient and related types (lower-level API)
pub use checked_client::ModelRegistration;

#[cfg(feature = "check")]
pub use checked_client::CheckedClient;

// Re-export PgClient (recommended API)
#[cfg(feature = "check")]
pub use pg_client::{CheckMode, ModelCheckResult, PgClient, PgClientConfig};

#[cfg(feature = "check")]
pub use check::{
    // Lint types and functions
    ColumnRef, delete_has_where, detect_statement_kind, get_column_refs, get_table_names,
    is_valid_sql, lint_select_many, lint_sql, select_has_limit, select_has_star, update_has_where,
    LintIssue, LintLevel, LintResult, ParseResult, StatementKind,
    // Database schema check
    check_sql, check_sql_cached, CheckClient, CheckError, CheckResult, ColumnInfo, DbSchema,
    RelationKind, SchemaCache, SchemaCacheConfig, SchemaCacheLoad, SqlCheckIssue, SqlCheckIssueKind,
    SqlCheckLevel, TableInfo,
    // Schema introspection
    load_schema_from_db,
};
