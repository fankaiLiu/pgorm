//! Structured SQL builder.
//!
//! This module is ported from `noseclass`'s `query_builder_v2` and provides a
//! lightweight, Postgres-only, parameter-safe way to build dynamic SQL.
//!
//! ## Design
//!
//! - SQL is still explicit (strings), but common patterns are structured.
//! - Safe defaults: DELETE requires WHERE (unless explicitly allowed);
//!   UPDATE requires SET.
//! - Placeholders are managed automatically ($1, $2, ...).

pub mod delete;
pub mod insert;
pub mod select;
pub mod table;
pub mod traits;
pub mod update;

pub use delete::DeleteBuilder;
pub use insert::InsertBuilder;
pub use select::{BuiltQuery, QueryBuilder};
pub use table::Table;
pub use traits::{MutationBuilder, SqlBuilder};
pub use update::UpdateBuilder;

#[cfg(test)]
mod tests;
