//! Convenient imports for typical `pgorm` usage.
//!
//! This module is intentionally small and focused on the most common APIs so
//! examples can start with:
//!
//! ```ignore
//! use pgorm::prelude::*;
//! ```

pub use crate::{BelongsToMap, HasManyMap, HasOneMap, Loaded};
pub use crate::{
    FromRow, GenericClient, OrmError, OrmResult, Query, RowExt, RowStream, Sql, StreamingClient,
    query, sql,
};

#[cfg(feature = "pool")]
pub use crate::{create_pool, create_pool_with_config};

#[cfg(feature = "derive")]
pub use crate::{InsertModel, Model, QueryParams, UpdateModel, ViewModel};

#[cfg(feature = "check")]
pub use crate::{CheckMode, CheckedClient, PgClient, PgClientConfig};

pub use crate::Json;
pub use crate::{Condition, Cursor, Ident, IntoIdent, Keyset1, Keyset2, Op};
