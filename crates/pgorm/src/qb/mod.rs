//! Unified Query Builder (QB) system for pgorm.
//!
//! This module provides a unified query builder that supports both generic SQL
//! queries and Model-based queries with the same underlying core.
//!
//! # Features
//!
//! - **Unified expression layer**: AND/OR/NOT groups with automatic placeholder numbering
//! - **No string replacement**: Parameter indices are computed at build time, not via string manipulation
//! - **Arc-based parameters**: Clone-friendly, suitable for derive macros
//! - **Consistent API**: Same methods for generic QB and Model QB
//!
//! # Usage
//!
//! ```ignore
//! use pgorm::qb;
//!
//! // Generic query builder
//! let users = qb::select("users")
//!     .select("*")
//!     .eq("status", "active")
//!     .order_by("created_at DESC")
//!     .limit(20)
//!     .fetch_all_as::<User>(&client)
//!     .await?;
//!
//! // INSERT
//! let id = qb::insert("users")
//!     .set("username", "alice")
//!     .set("email", "alice@example.com")
//!     .returning("id")
//!     .fetch_one_as::<i64>(&client)
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

mod delete;
mod expr;
mod insert;
mod param;
mod select;
mod traits;
mod update;

pub use delete::DeleteQb;
pub use expr::{Expr, ExprGroup};
pub use insert::{ConflictAction, InsertQb, OnConflictQb};
pub use param::Param;
pub use select::SelectQb;
pub use traits::{MutationQb, SqlQb};
pub use update::UpdateQb;

/// Create a SELECT query builder for the given table.
///
/// # Example
/// ```ignore
/// let qb = pgorm::qb::select("users").eq("id", 1);
/// ```
pub fn select(table: &str) -> SelectQb {
    SelectQb::new(table)
}

/// Create a SELECT query builder with a custom FROM expression.
///
/// Use this for complex FROM clauses like aliases or subqueries.
///
/// # Example
/// ```ignore
/// let qb = pgorm::qb::select_from("users u").inner_join("orders o", "u.id = o.user_id");
/// ```
pub fn select_from(from_expr: &str) -> SelectQb {
    SelectQb::from(from_expr)
}

/// Create an INSERT query builder for the given table.
///
/// # Example
/// ```ignore
/// let qb = pgorm::qb::insert("users")
///     .set("username", "alice")
///     .set("email", "alice@example.com");
/// ```
pub fn insert(table: &str) -> InsertQb {
    InsertQb::new(table)
}

/// Alias for `insert`.
pub fn insert_into(table: &str) -> InsertQb {
    InsertQb::new(table)
}

/// Create an UPDATE query builder for the given table.
///
/// # Example
/// ```ignore
/// let qb = pgorm::qb::update("users")
///     .set("status", "inactive")
///     .eq("id", user_id);
/// ```
pub fn update(table: &str) -> UpdateQb {
    UpdateQb::new(table)
}

/// Create a DELETE query builder for the given table.
///
/// # Safety
/// By default, DELETE without WHERE conditions will generate `WHERE 1=0` (no-op).
/// Use `allow_delete_all(true)` to allow deleting all rows.
///
/// # Example
/// ```ignore
/// let qb = pgorm::qb::delete("users").eq("id", user_id);
/// ```
pub fn delete(table: &str) -> DeleteQb {
    DeleteQb::new(table)
}

/// Alias for `delete`.
pub fn delete_from(table: &str) -> DeleteQb {
    DeleteQb::new(table)
}

#[cfg(test)]
mod tests;
