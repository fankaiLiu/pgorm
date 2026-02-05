//! Bulk update and delete operations.
//!
//! This module provides [`SetExpr`], [`UpdateManyBuilder`], and [`DeleteManyBuilder`]
//! for performing batch UPDATE and DELETE operations with type-safe conditions.
//!
//! # Example
//! ```ignore
//! use pgorm::prelude::*;
//! use pgorm::SetExpr;
//!
//! // Bulk update
//! let affected = pgorm::sql("users")
//!     .update_many([
//!         SetExpr::set("status", "inactive")?,
//!     ])
//!     .filter(Condition::lt("last_login", one_year_ago)?)
//!     .execute(&client)
//!     .await?;
//!
//! // Bulk delete
//! let deleted = pgorm::sql("sessions")
//!     .delete_many()
//!     .filter(Condition::lt("expires_at", now)?)
//!     .execute(&client)
//!     .await?;
//! ```

use crate::builder::WhereExpr;
use crate::client::{GenericClient, StreamingClient};
use crate::error::{OrmError, OrmResult};
use crate::ident::{Ident, IntoIdent};
use crate::row::FromRow;
use crate::sql::{FromRowStream, Sql};
use std::sync::Arc;
use tokio_postgres::types::ToSql;

// ==================== SetExpr ====================

/// A SET clause expression for bulk updates.
///
/// # Example
/// ```ignore
/// use pgorm::SetExpr;
///
/// // Simple value assignment: SET status = $1
/// SetExpr::set("status", "inactive")?;
///
/// // Increment: SET view_count = view_count + 1
/// SetExpr::increment("view_count", 1)?;
///
/// // Raw SQL expression: SET updated_at = NOW()
/// SetExpr::raw("updated_at = NOW()");
/// ```
pub enum SetExpr {
    /// `column = $n` (parameterized value)
    Value {
        column: Ident,
        value: Arc<dyn ToSql + Send + Sync>,
    },
    /// `column = column + amount` (increment/decrement)
    Increment { column: Ident, amount: i64 },
    /// Raw SQL expression (escape hatch), e.g. `"updated_at = NOW()"`
    Raw(String),
}

impl SetExpr {
    /// Create a SET clause that assigns a parameterized value: `col = $n`
    pub fn set<T: ToSql + Send + Sync + 'static>(
        column: impl IntoIdent,
        value: T,
    ) -> OrmResult<Self> {
        Ok(SetExpr::Value {
            column: column.into_ident()?,
            value: Arc::new(value),
        })
    }

    /// Create a SET clause that increments a column: `col = col + amount`
    ///
    /// Supports negative values for decrement.
    pub fn increment(column: impl IntoIdent, amount: i64) -> OrmResult<Self> {
        Ok(SetExpr::Increment {
            column: column.into_ident()?,
            amount,
        })
    }

    /// Create a SET clause with a raw SQL expression.
    ///
    /// The string should be a complete assignment expression, e.g. `"updated_at = NOW()"`.
    ///
    /// **Warning**: This bypasses SQL injection protection. Only use with trusted SQL.
    pub fn raw(expr: impl Into<String>) -> Self {
        SetExpr::Raw(expr.into())
    }

    fn append_to_sql(&self, sql: &mut Sql) {
        match self {
            SetExpr::Value { column, value } => {
                sql.push_ident_ref(column);
                sql.push(" = ");
                sql.push_bind_value(value.clone());
            }
            SetExpr::Increment { column, amount } => {
                sql.push_ident_ref(column);
                sql.push(" = ");
                sql.push_ident_ref(column);
                if *amount >= 0 {
                    let s = format!(" + {amount}");
                    sql.push(&s);
                } else {
                    let s = format!(" - {}", amount.abs());
                    sql.push(&s);
                }
            }
            SetExpr::Raw(expr) => {
                sql.push(expr);
            }
        }
    }
}

// ==================== UpdateManyBuilder ====================

/// Builder for bulk UPDATE operations.
///
/// Created via [`Sql::update_many`].
///
/// # Example
/// ```ignore
/// pgorm::sql("users")
///     .update_many([
///         SetExpr::set("status", "inactive")?,
///     ])
///     .filter(Condition::lt("last_login", one_year_ago)?)
///     .execute(&client)
///     .await?;
/// ```
#[must_use]
pub struct UpdateManyBuilder {
    pub(crate) table: Ident,
    pub(crate) sets: Vec<SetExpr>,
    pub(crate) where_clause: Option<WhereExpr>,
    pub(crate) all_rows: bool,
}

impl UpdateManyBuilder {
    /// Add a WHERE condition.
    pub fn filter(mut self, condition: impl Into<WhereExpr>) -> Self {
        let new_where = condition.into();
        self.where_clause = Some(match self.where_clause.take() {
            Some(existing) => existing.and_with(new_where),
            None => new_where,
        });
        self
    }

    /// Explicitly allow updating all rows without a WHERE clause.
    ///
    /// Without this, executing without a `.filter()` returns an error.
    pub fn all_rows(mut self) -> Self {
        self.all_rows = true;
        self
    }

    /// Build the SQL statement without executing it.
    ///
    /// Useful for inspecting the generated SQL.
    pub fn build_sql(&self) -> OrmResult<Sql> {
        if self.where_clause.is_none() && !self.all_rows {
            return Err(OrmError::Validation(
                "update_many requires a .filter() condition or .all_rows() to proceed. \
                 This prevents accidental full-table updates."
                    .to_string(),
            ));
        }

        let mut sql = Sql::new("UPDATE ");
        sql.push_ident_ref(&self.table);
        sql.push(" SET ");

        for (i, set) in self.sets.iter().enumerate() {
            if i > 0 {
                sql.push(", ");
            }
            set.append_to_sql(&mut sql);
        }

        if let Some(ref where_clause) = self.where_clause {
            sql.push(" WHERE ");
            where_clause.append_to_sql(&mut sql);
        }

        Ok(sql)
    }

    /// Execute the update, returning the number of affected rows.
    pub async fn execute(self, conn: &impl GenericClient) -> OrmResult<u64> {
        let sql = self.build_sql()?;
        sql.execute(conn).await
    }

    /// Execute the update and return the affected rows.
    ///
    /// Appends `RETURNING *` to the query.
    pub async fn returning<T: FromRow>(self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let mut sql = self.build_sql()?;
        sql.push(" RETURNING *");
        sql.fetch_all_as(conn).await
    }

    /// Execute the update and return a stream of affected rows.
    ///
    /// Appends `RETURNING *` to the query.
    pub async fn returning_stream<T: FromRow>(
        self,
        conn: &impl StreamingClient,
    ) -> OrmResult<FromRowStream<T>> {
        let mut sql = self.build_sql()?;
        sql.push(" RETURNING *");
        sql.stream_as(conn).await
    }
}

// ==================== DeleteManyBuilder ====================

/// Builder for bulk DELETE operations.
///
/// Created via [`Sql::delete_many`].
///
/// # Example
/// ```ignore
/// pgorm::sql("sessions")
///     .delete_many()
///     .filter(Condition::lt("expires_at", now)?)
///     .execute(&client)
///     .await?;
/// ```
#[must_use]
pub struct DeleteManyBuilder {
    pub(crate) table: Ident,
    pub(crate) where_clause: Option<WhereExpr>,
    pub(crate) all_rows: bool,
}

impl DeleteManyBuilder {
    /// Add a WHERE condition.
    pub fn filter(mut self, condition: impl Into<WhereExpr>) -> Self {
        let new_where = condition.into();
        self.where_clause = Some(match self.where_clause.take() {
            Some(existing) => existing.and_with(new_where),
            None => new_where,
        });
        self
    }

    /// Explicitly allow deleting all rows without a WHERE clause.
    ///
    /// Without this, executing without a `.filter()` returns an error.
    pub fn all_rows(mut self) -> Self {
        self.all_rows = true;
        self
    }

    /// Build the SQL statement without executing it.
    ///
    /// Useful for inspecting the generated SQL.
    pub fn build_sql(&self) -> OrmResult<Sql> {
        if self.where_clause.is_none() && !self.all_rows {
            return Err(OrmError::Validation(
                "delete_many requires a .filter() condition or .all_rows() to proceed. \
                 This prevents accidental full-table deletes."
                    .to_string(),
            ));
        }

        let mut sql = Sql::new("DELETE FROM ");
        sql.push_ident_ref(&self.table);

        if let Some(ref where_clause) = self.where_clause {
            sql.push(" WHERE ");
            where_clause.append_to_sql(&mut sql);
        }

        Ok(sql)
    }

    /// Execute the delete, returning the number of affected rows.
    pub async fn execute(self, conn: &impl GenericClient) -> OrmResult<u64> {
        let sql = self.build_sql()?;
        sql.execute(conn).await
    }

    /// Execute the delete and return the deleted rows.
    ///
    /// Appends `RETURNING *` to the query.
    pub async fn returning<T: FromRow>(self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let mut sql = self.build_sql()?;
        sql.push(" RETURNING *");
        sql.fetch_all_as(conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::Condition;

    #[test]
    fn update_many_basic_sql() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("users").unwrap(),
            sets: vec![SetExpr::set("status", "inactive").unwrap()],
            where_clause: Some(WhereExpr::Atom(Condition::eq("active", true).unwrap())),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE users SET status = $1 WHERE active = $2"
        );
        assert_eq!(sql.params_ref().len(), 2);
    }

    #[test]
    fn update_many_multiple_sets() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("orders").unwrap(),
            sets: vec![
                SetExpr::set("status", "shipped").unwrap(),
                SetExpr::raw("shipped_at = NOW()"),
            ],
            where_clause: Some(WhereExpr::Atom(Condition::eq("id", 1_i64).unwrap())),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE orders SET status = $1, shipped_at = NOW() WHERE id = $2"
        );
        assert_eq!(sql.params_ref().len(), 2);
    }

    #[test]
    fn update_many_increment() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("products").unwrap(),
            sets: vec![SetExpr::increment("view_count", 1).unwrap()],
            where_clause: Some(WhereExpr::Atom(Condition::eq("id", 42_i64).unwrap())),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE products SET view_count = view_count + 1 WHERE id = $1"
        );
        assert_eq!(sql.params_ref().len(), 1);
    }

    #[test]
    fn update_many_decrement() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("products").unwrap(),
            sets: vec![SetExpr::increment("stock", -5).unwrap()],
            where_clause: Some(WhereExpr::Atom(Condition::eq("id", 1_i64).unwrap())),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE products SET stock = stock - 5 WHERE id = $1"
        );
    }

    #[test]
    fn update_many_all_rows() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("temp_data").unwrap(),
            sets: vec![SetExpr::set("status", "archived").unwrap()],
            where_clause: None,
            all_rows: true,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(sql.to_sql(), "UPDATE temp_data SET status = $1");
    }

    #[test]
    fn update_many_rejects_no_where() {
        let builder = UpdateManyBuilder {
            table: Ident::parse("users").unwrap(),
            sets: vec![SetExpr::set("status", "x").unwrap()],
            where_clause: None,
            all_rows: false,
        };
        assert!(builder.build_sql().is_err());
    }

    #[test]
    fn delete_many_basic_sql() {
        let builder = DeleteManyBuilder {
            table: Ident::parse("sessions").unwrap(),
            where_clause: Some(WhereExpr::raw("expires_at < NOW()")),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "DELETE FROM sessions WHERE expires_at < NOW()"
        );
    }

    #[test]
    fn delete_many_with_condition() {
        let builder = DeleteManyBuilder {
            table: Ident::parse("audit_logs").unwrap(),
            where_clause: Some(WhereExpr::And(vec![
                WhereExpr::Atom(Condition::eq("level", "debug").unwrap()),
                WhereExpr::Atom(Condition::eq("archived", true).unwrap()),
            ])),
            all_rows: false,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "DELETE FROM audit_logs WHERE (level = $1 AND archived = $2)"
        );
        assert_eq!(sql.params_ref().len(), 2);
    }

    #[test]
    fn delete_many_all_rows() {
        let builder = DeleteManyBuilder {
            table: Ident::parse("temp_data").unwrap(),
            where_clause: None,
            all_rows: true,
        };
        let sql = builder.build_sql().unwrap();
        assert_eq!(sql.to_sql(), "DELETE FROM temp_data");
    }

    #[test]
    fn delete_many_rejects_no_where() {
        let builder = DeleteManyBuilder {
            table: Ident::parse("users").unwrap(),
            where_clause: None,
            all_rows: false,
        };
        assert!(builder.build_sql().is_err());
    }

    #[test]
    fn update_many_via_sql_builder() {
        let builder = crate::sql("users")
            .update_many([SetExpr::set("status", "inactive").unwrap()])
            .unwrap()
            .filter(Condition::eq("active", true).unwrap());
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE users SET status = $1 WHERE active = $2"
        );
    }

    #[test]
    fn delete_many_via_sql_builder() {
        let builder = crate::sql("sessions")
            .delete_many()
            .unwrap()
            .filter(WhereExpr::raw("expires_at < NOW()"));
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "DELETE FROM sessions WHERE expires_at < NOW()"
        );
    }

    #[test]
    fn update_many_filter_combines_with_and() {
        let builder = crate::sql("orders")
            .update_many([SetExpr::set("status", "archived").unwrap()])
            .unwrap()
            .filter(Condition::eq("status", "cancelled").unwrap())
            .filter(Condition::eq("archived", false).unwrap());
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "UPDATE orders SET status = $1 WHERE (status = $2 AND archived = $3)"
        );
    }

    #[test]
    fn delete_many_filter_combines_with_and() {
        let builder = crate::sql("logs")
            .delete_many()
            .unwrap()
            .filter(Condition::eq("level", "debug").unwrap())
            .filter(Condition::eq("archived", true).unwrap());
        let sql = builder.build_sql().unwrap();
        assert_eq!(
            sql.to_sql(),
            "DELETE FROM logs WHERE (level = $1 AND archived = $2)"
        );
    }

    #[test]
    fn set_expr_validates_column_name() {
        assert!(SetExpr::set("valid_column", "value").is_ok());
        assert!(SetExpr::set("1invalid", "value").is_err());
        assert!(SetExpr::set("has space", "value").is_err());
        assert!(SetExpr::increment("valid_col", 1).is_ok());
        assert!(SetExpr::increment("bad;col", 1).is_err());
    }
}
