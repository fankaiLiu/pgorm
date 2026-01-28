//! DELETE query builder using the unified expression layer.

use crate::client::GenericClient;
use crate::error::OrmResult;
use crate::qb::expr::{Expr, ExprGroup};
use crate::qb::param::ParamList;
use crate::qb::traits::{MutationQb, SqlQb};
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// DELETE query builder with unified expression-based WHERE.
#[derive(Clone, Debug)]
pub struct DeleteQb {
    /// Table name
    table: String,
    /// WHERE conditions
    where_group: ExprGroup,
    /// RETURNING columns
    returning_cols: Vec<String>,
    /// Whether to allow DELETE without WHERE (dangerous!)
    allow_delete_all: bool,
}

impl DeleteQb {
    /// Create a new DELETE query builder.
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            where_group: ExprGroup::new(),
            returning_cols: Vec::new(),
            allow_delete_all: false,
        }
    }

    /// Allow DELETE without WHERE conditions (dangerous!).
    ///
    /// By default, DELETE without WHERE generates `WHERE 1=0` (no-op).
    /// Call this with `true` to allow deleting all rows.
    pub fn allow_delete_all(mut self, allow: bool) -> Self {
        self.allow_delete_all = allow;
        self
    }

    // ==================== WHERE conditions ====================

    /// Add WHERE: column = value
    pub fn eq<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.eq(column, value);
        self
    }

    /// Add WHERE: column != value
    pub fn ne<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.ne(column, value);
        self
    }

    /// Add WHERE: column > value
    pub fn gt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.gt(column, value);
        self
    }

    /// Add WHERE: column >= value
    pub fn gte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.gte(column, value);
        self
    }

    /// Add WHERE: column < value
    pub fn lt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.lt(column, value);
        self
    }

    /// Add WHERE: column <= value
    pub fn lte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.lte(column, value);
        self
    }

    /// Add WHERE: column IN (values...)
    pub fn in_list<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Vec<T>) -> Self {
        self.where_group.in_list(column, values);
        self
    }

    /// Add WHERE: column NOT IN (values...)
    pub fn not_in<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Vec<T>) -> Self {
        self.where_group.not_in(column, values);
        self
    }

    /// Add WHERE: column IS NULL
    pub fn is_null(mut self, column: &str) -> Self {
        self.where_group.is_null(column);
        self
    }

    /// Add WHERE: column IS NOT NULL
    pub fn is_not_null(mut self, column: &str) -> Self {
        self.where_group.is_not_null(column);
        self
    }

    /// Add a raw WHERE condition.
    pub fn raw(mut self, sql: &str) -> Self {
        self.where_group.raw(sql);
        self
    }

    /// Add a custom expression.
    pub fn and_expr(mut self, expr: Expr) -> Self {
        self.where_group.and_expr(expr);
        self
    }

    // ==================== RETURNING ====================

    /// Set RETURNING columns (string form).
    pub fn returning(mut self, cols: &str) -> Self {
        self.returning_cols = vec![cols.to_string()];
        self
    }

    /// Set RETURNING columns (array form).
    pub fn returning_cols(mut self, cols: &[&str]) -> Self {
        self.returning_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    // ==================== Build ====================

    /// Build the DELETE SQL and parameters.
    fn build_delete(&self) -> (String, ParamList) {
        let mut params = ParamList::new();

        // Safety check: no WHERE and not allowed to delete all
        if self.where_group.is_empty() && !self.allow_delete_all {
            return (format!("DELETE FROM {} WHERE 1=0", self.table), params);
        }

        let mut sql = format!("DELETE FROM {}", self.table);

        // Build WHERE clause
        let (where_sql, where_params) = self.where_group.build();
        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }
        params.extend(&where_params);

        // RETURNING
        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        (sql, params)
    }

    /// Get the built SQL string (for debugging).
    pub fn to_sql(&self) -> String {
        self.build_delete().0
    }
}

impl SqlQb for DeleteQb {
    fn build_sql(&self) -> String {
        self.build_delete().0
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        vec![]
    }

    fn query(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        async move {
            let (sql, params) = self.build_delete();
            let params_ref = params.as_refs();
            conn.query(&sql, &params_ref).await
        }
    }

    fn query_opt(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        async move {
            let (sql, params) = self.build_delete();
            let params_ref = params.as_refs();
            conn.query_opt(&sql, &params_ref).await
        }
    }

    fn query_one(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            let (sql, params) = self.build_delete();
            let params_ref = params.as_refs();
            conn.query_one(&sql, &params_ref).await
        }
    }

    fn fetch_all<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<T>>> + Send {
        async move {
            let rows = self.query(conn).await?;
            rows.iter().map(T::from_row).collect()
        }
    }

    fn fetch_opt<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<T>>> + Send {
        async move {
            let row = self.query_opt(conn).await?;
            row.as_ref().map(T::from_row).transpose()
        }
    }

    fn fetch_one<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<T>> + Send {
        async move {
            let row = self.query_one(conn).await?;
            T::from_row(&row)
        }
    }
}

impl MutationQb for DeleteQb {
    fn execute(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        async move {
            let (sql, params) = self.build_delete();
            let params_ref = params.as_refs();
            conn.execute(&sql, &params_ref).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_delete() {
        let qb = DeleteQb::new("users").eq("id", 1i64);
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users WHERE id = $1");
    }

    #[test]
    fn test_delete_without_where_safe() {
        let qb = DeleteQb::new("users");
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users WHERE 1=0");
    }

    #[test]
    fn test_delete_allow_all() {
        let qb = DeleteQb::new("users").allow_delete_all(true);
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users");
    }

    #[test]
    fn test_delete_with_returning() {
        let qb = DeleteQb::new("users")
            .eq("id", 1i64)
            .returning("*");
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users WHERE id = $1 RETURNING *");
    }

    #[test]
    fn test_delete_complex_where() {
        let qb = DeleteQb::new("users")
            .eq("status", "inactive")
            .lt("last_login", "2024-01-01");
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users WHERE status = $1 AND last_login < $2");
    }

    #[test]
    fn test_delete_with_in() {
        let qb = DeleteQb::new("users").in_list("id", vec![1i64, 2, 3]);
        let sql = qb.to_sql();
        assert_eq!(sql, "DELETE FROM users WHERE id IN ($1, $2, $3)");
    }
}
