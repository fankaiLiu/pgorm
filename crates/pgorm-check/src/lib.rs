//! pgorm-check
//!
//! Runtime helpers for checking whether SQL references match a live database schema.
//!
//! This crate can cache schema metadata into a local directory (default: `./.pgorm/`)
//! so subsequent runs can skip a full refresh when nothing has changed.

pub mod schema_cache;
pub mod schema_introspect;

#[cfg(feature = "sql")]
pub mod sql_check;

pub use schema_cache::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad};
pub use schema_introspect::{ColumnInfo, DbSchema, RelationKind, TableInfo};

#[cfg(feature = "sql")]
pub use sql_check::{
    SqlCheckIssue, SqlCheckIssueKind, SqlCheckLevel, check_sql, check_sql_cached,
};
