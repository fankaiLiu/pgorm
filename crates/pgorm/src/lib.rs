//! # pgorm
//!
//! A lightweight Postgres-only ORM for Rust.
//!
//! ## Features
//!
//! - **SQL explicit**: SQL is a first-class citizen (use `query()` / `sql()`; `qb::query()` is an alias)
//! - **Type-safe mapping**: Row → Struct via `FromRow` trait
//! - **Minimal magic**: Traits and macros only for boilerplate reduction
//! - **Transaction-friendly**: pass a transaction anywhere a `GenericClient` is expected
//! - **Query monitoring**: Built-in support for timing, logging, and hooking SQL execution
//! - **SQL checking**: Validate SQL against registered schemas and lint for common issues
//! - **Migrations**: Optional SQL migrations via `refinery` (feature: `migrate`)
//!
//! ## SQL fallback (qb)
//!
//! The `qb` module is a thin wrapper around `query()` for running hand-written SQL:
//!
//! ```ignore
//! use pgorm::qb;
//!
//! let users: Vec<User> = qb::query("SELECT * FROM users WHERE status = $1")
//!     .bind("active")
//!     .fetch_all_as(&client)
//!     .await?;
//! ```

mod builder;
pub mod changeset;
mod client;
mod condition;
pub mod eager;
mod error;
mod ident;
mod monitor;
pub mod prelude;
pub mod qb;
mod row;
mod sql;
mod transaction;

#[cfg(feature = "validate")]
pub mod validate;

// SQL migrations (via refinery)
#[cfg(feature = "migrate")]
pub mod migrate;

pub use builder::{NullsOrder, OrderBy, OrderItem, Pagination, SortDir, WhereExpr};
pub use changeset::{ValidationCode, ValidationError, ValidationErrors};
pub use client::GenericClient;
pub use condition::{Condition, Op};
pub use eager::{BelongsToMap, HasManyMap, Loaded};
pub use error::{OrmError, OrmResult};
pub use ident::{Ident, IdentPart, IntoIdent};
pub use monitor::{
    CompositeHook, CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor, MonitorConfig,
    NoopMonitor, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryStats, QueryType,
    StatsMonitor,
};
pub use row::{FromRow, PgType, RowExt};
pub use sql::{Query, Sql, query, sql};
pub use tokio_postgres::types::Json;

// Re-export refinery types for convenience
#[cfg(feature = "migrate")]
pub use migrate::{Migration, Report, Runner, Target, embed_migrations};

#[cfg(feature = "pool")]
mod pool;

#[cfg(feature = "pool")]
pub use client::PoolClient;

#[cfg(feature = "pool")]
pub use pool::{create_pool, create_pool_with_config};

#[cfg(feature = "pool")]
pub use pool::{create_pool_with_manager_config, create_pool_with_tls};

#[cfg(feature = "derive")]
pub use pgorm_derive::{FromRow, InsertModel, Model, UpdateModel, ViewModel};

// SQL checking and linting
mod check;

// Checked client with auto-registration (lower-level)
mod checked_client;

// Unified PgClient (recommended)
#[cfg(feature = "check")]
mod pg_client;

pub use check::{
    ColumnMeta, SchemaIssue, SchemaIssueKind, SchemaIssueLevel, SchemaRegistry, TableMeta,
    TableSchema,
};

// Re-export inventory for use by derive macros
#[doc(hidden)]
pub use inventory;

// Re-export serde for derive-generated input structs.
#[doc(hidden)]
pub use serde;

// Re-export CheckedClient and related types (lower-level API)
#[doc(hidden)]
pub use checked_client::ModelRegistration;

#[cfg(feature = "check")]
pub use checked_client::CheckedClient;

// Re-export PgClient (recommended API)
#[cfg(feature = "check")]
pub use pg_client::{
    CheckMode, DangerousDmlPolicy, ModelCheckResult, PgClient, PgClientConfig,
    SelectWithoutLimitPolicy, SqlPolicy,
};

#[cfg(feature = "check")]
pub use check::{
    CheckClient,
    CheckError,
    CheckResult,
    ColumnInfo,
    // Lint types and functions
    ColumnRef,
    DbSchema,
    LintIssue,
    LintLevel,
    LintResult,
    ParseResult,
    RelationKind,
    SchemaCache,
    SchemaCacheConfig,
    SchemaCacheLoad,
    SqlCheckIssue,
    SqlCheckIssueKind,
    SqlCheckLevel,
    StatementKind,
    TableInfo,
    // Database schema check
    check_sql,
    check_sql_cached,
    delete_has_where,
    detect_statement_kind,
    get_column_refs,
    get_table_names,
    is_valid_sql,
    lint_select_many,
    lint_sql,
    // Schema introspection
    load_schema_from_db,
    select_has_limit,
    select_has_star,
    update_has_where,
};

// ─────────────────────────────────────────────────────────────────────────────
// Write Graph Types (for multi-table writes)
// ─────────────────────────────────────────────────────────────────────────────

/// Trait for models with a primary key.
///
/// This trait is automatically implemented by `#[derive(Model)]` for structs
/// with an `#[orm(id)]` field. It provides a way to access the primary key
/// value without requiring field visibility.
///
/// # Example
///
/// ```ignore
/// #[derive(Model)]
/// #[orm(table = "orders")]
/// struct Order {
///     #[orm(id)]
///     id: i64,
///     user_id: i64,
/// }
///
/// // ModelPk is automatically implemented:
/// let order: Order = /* ... */;
/// let pk: &i64 = order.pk();
/// ```
pub trait ModelPk {
    /// The type of the primary key.
    type Id: Clone + Send + Sync + 'static;

    /// Returns a reference to the primary key value.
    fn pk(&self) -> &Self::Id;
}

/// Report returned by `insert_graph_report` / `update_by_id_graph_report`.
///
/// Contains detailed information about each step in the write graph execution.
#[derive(Debug, Clone)]
pub struct WriteReport<R> {
    /// Sum of affected rows across all steps.
    pub affected: u64,
    /// Per-step statistics (in execution order).
    pub steps: ::std::vec::Vec<WriteStepReport>,
    /// The root table's returning value (if the `_returning` or `_report` variant was called).
    pub root: ::std::option::Option<R>,
}

/// Statistics for a single step in a write graph.
#[derive(Debug, Clone)]
pub struct WriteStepReport {
    /// A tag identifying this step, e.g. `"graph:belongs_to:categories"`,
    /// `"graph:root:orders"`, `"graph:has_many:order_items"`.
    pub tag: &'static str,
    /// Number of rows affected by this step.
    pub affected: u64,
}
