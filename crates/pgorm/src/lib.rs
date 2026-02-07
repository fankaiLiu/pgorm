//! # pgorm
//!
//! A model-definition-first, AI-friendly PostgreSQL ORM for Rust.
//!
//! ## Quick Start
//!
//! ```ignore
//! use pgorm::prelude::*;
//! ```
//!
//! ## Two-level API
//!
//! - **Recommended**: [`PgClient`] — monitoring + SQL checking + statement cache + policy
//! - **Low-level**: [`GenericClient`] / [`Sql`] — pluggable, minimal abstraction
//!
//! ## Modules
//!
//! - [`monitor`] — query monitoring, hooks, [`InstrumentedClient`]
//! - [`check`] — SQL schema checking, linting, [`SchemaRegistry`]
//! - [`prelude`] — convenient `use pgorm::prelude::*` for daily use
//! - [`qb`] — thin wrapper around `query()` for hand-written SQL
//!
//! > **Stability:** pgorm is pre-1.0. APIs may change between minor versions.
//! > MSRV: 1.88+

mod builder;
mod bulk;
pub mod changeset;
mod client;
mod condition;
mod cte;
pub mod eager;
mod error;
mod ident;
pub mod monitor;
pub mod prelude;
pub mod qb;
mod row;
mod sql;
mod transaction;
pub mod types;

#[cfg(feature = "validate")]
pub mod validate;

// SQL migrations (via refinery)
#[cfg(feature = "migrate")]
pub mod migrate;

// ─────────────────────────────────────────────────────────────────────────────
// Core stable exports (pgorm::*)
// ─────────────────────────────────────────────────────────────────────────────

// SQL building
pub use builder::{
    Cursor, Keyset1, Keyset2, NullsOrder, OrderBy, OrderItem, Pagination, SortDir, WhereExpr,
};
pub use condition::{Condition, Op};
pub use cte::WithBuilder;
pub use sql::{FromRowStream, Query, Sql, query, sql};

// Row mapping & types
pub use row::{FromRow, PgType, RowExt};
pub use tokio_postgres;
pub use tokio_postgres::types::Json;
pub use types::{Bound, Range};

// Client
pub use client::{GenericClient, RowStream, StreamingClient};

// Bulk operations
pub use bulk::{DeleteManyBuilder, SetExpr, UpdateManyBuilder};

// Identifiers
pub use ident::{Ident, IdentPart, IntoIdent};

// Eager loading
pub use eager::{BelongsToMap, HasManyMap, HasOneMap, Loaded};

// Transactions
pub use transaction::{
    __next_savepoint_name, Savepoint, TransactionBeginExt, TransactionExt, TransactionIsolation,
    TransactionOptions, begin_transaction, begin_transaction_with,
};

// Validation
pub use changeset::{ValidationCode, ValidationError, ValidationErrors};

// Errors
pub use error::{OrmError, OrmResult};

// Re-export refinery types for convenience
#[cfg(feature = "migrate")]
pub use migrate::{
    AppliedMigration, DiskMigration, Migration, MigrationStatus, Report, Runner, SchemaVersion,
    Target, embed_migrations,
};

// Connection pooling
#[cfg(feature = "pool")]
mod pool;

#[cfg(feature = "pool")]
pub use client::PoolClient;

#[cfg(feature = "pool")]
pub use pool::{create_pool, create_pool_with_config};

#[cfg(feature = "pool")]
pub use pool::{create_pool_with_manager_config, create_pool_with_tls};

// Derive macros
#[cfg(feature = "derive")]
pub use pgorm_derive::{
    FromRow, InsertModel, Model, PgComposite, PgEnum, QueryParams, UpdateModel, ViewModel,
};

// ─────────────────────────────────────────────────────────────────────────────
// SQL checking and linting (public module: pgorm::check)
// ─────────────────────────────────────────────────────────────────────────────

pub mod check;

// Checked client with auto-registration (lower-level)
mod checked_client;

// Unified PgClient (recommended)
#[cfg(feature = "check")]
mod pg_client;

// Core schema types stay at top level
pub use check::{ColumnMeta, SchemaRegistry, TableMeta, TableSchema};

// Re-export inventory for use by derive macros
#[doc(hidden)]
pub use inventory;

// Re-export serde for derive-generated input structs.
#[doc(hidden)]
pub use serde;

// Re-export CheckedClient and related types (lower-level API)
#[doc(hidden)]
pub use checked_client::ModelRegistration;

// Re-export PgClient (recommended API) — core stable
#[cfg(feature = "check")]
pub use pg_client::{
    CheckMode, DangerousDmlPolicy, ModelCheckResult, PgClient, PgClientConfig,
    SelectWithoutLimitPolicy, SqlPolicy, StatementCacheConfig, StmtCacheStats,
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
