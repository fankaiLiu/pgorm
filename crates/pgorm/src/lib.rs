//! # pgorm
//!
//! A lightweight Postgres-only ORM for Rust.
//!
//! ## Features
//!
//! - **SQL explicit**: SQL is a first-class citizen (use `query()` / `sql()` or the optional builders)
//! - **Type-safe mapping**: Row → Struct via `FromRow` trait
//! - **Minimal magic**: Traits and macros only for boilerplate reduction
//! - **Safe defaults**: DELETE requires WHERE, UPDATE requires SET

pub mod client;
pub mod condition;
pub mod error;
pub mod query;
pub mod row;
pub mod sql;

pub use client::GenericClient;
pub use condition::{Condition, Op};
pub use error::{OrmError, OrmResult};
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
