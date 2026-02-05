//! Convenient imports for typical `pgorm` usage.
//!
//! This module covers ~80% of common use cases. Start with:
//!
//! ```ignore
//! use pgorm::prelude::*;
//! ```
//!
//! For advanced features (monitoring, SQL linting), import from
//! [`pgorm::monitor`](crate::monitor) or [`pgorm::check`](crate::check) directly.

// ── Row mapping & core types ────────────────────────────────────────────────
pub use crate::sql::{FromRowStream, Query, Sql, query, sql};
pub use crate::types::{Bound, Range};
pub use crate::{FromRow, RowExt};
pub use tokio_postgres::types::Json;

// ── Client ──────────────────────────────────────────────────────────────────
pub use crate::client::{GenericClient, RowStream, StreamingClient};

// ── Query building ──────────────────────────────────────────────────────────
pub use crate::builder::{
    Cursor, Keyset1, Keyset2, NullsOrder, OrderBy, OrderItem, Pagination, SortDir, WhereExpr,
};
pub use crate::condition::{Condition, Op};
pub use crate::cte::WithBuilder;
pub use crate::ident::{Ident, IntoIdent};

// ── Bulk operations ─────────────────────────────────────────────────────────
pub use crate::bulk::{DeleteManyBuilder, SetExpr, UpdateManyBuilder};

// ── Eager loading ───────────────────────────────────────────────────────────
pub use crate::eager::{BelongsToMap, HasManyMap, HasOneMap, Loaded};

// ── Transactions ────────────────────────────────────────────────────────────
pub use crate::transaction::{Savepoint, TransactionExt};

// ── Validation ──────────────────────────────────────────────────────────────
pub use crate::changeset::{ValidationError, ValidationErrors};

// ── Errors ──────────────────────────────────────────────────────────────────
pub use crate::error::{OrmError, OrmResult};

// ── Write graphs ────────────────────────────────────────────────────────────
pub use crate::{ModelPk, WriteReport, WriteStepReport};

// ── Connection pooling (feature: pool) ──────────────────────────────────────
#[cfg(feature = "pool")]
pub use crate::pool::{create_pool, create_pool_with_config};

// ── Derive macros (feature: derive) ─────────────────────────────────────────
#[cfg(feature = "derive")]
pub use crate::{InsertModel, Model, PgComposite, PgEnum, QueryParams, UpdateModel, ViewModel};

// ── PgClient (feature: check) ───────────────────────────────────────────────
#[cfg(feature = "check")]
pub use crate::pg_client::{CheckMode, PgClient, PgClientConfig};
