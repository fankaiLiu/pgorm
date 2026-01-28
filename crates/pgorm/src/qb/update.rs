//! UPDATE query builder using the unified expression layer.

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::qb::expr::{Expr, ExprGroup};
use crate::qb::param::{Param, ParamList};
use crate::qb::traits::{MutationQb, SqlQb};
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// SET field value type.
#[derive(Clone, Debug)]
enum SetField {
    /// Parameterized value
    Value(Param),
    /// Raw SQL expression
    Raw(String),
}

/// UPDATE query builder with unified expression-based WHERE.
#[derive(Clone, Debug)]
pub struct UpdateQb {
    /// Table name (empty for ON CONFLICT DO UPDATE)
    table: String,
    /// SET clauses
    set_fields: Vec<(String, SetField)>,
    /// WHERE conditions
    where_group: ExprGroup,
    /// RETURNING columns
    returning_cols: Vec<String>,
    /// Whether this is for ON CONFLICT DO UPDATE (no table prefix)
    #[allow(dead_code)]
    for_conflict: bool,
}

impl UpdateQb {
    /// Create a new UPDATE query builder.
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            set_fields: Vec::new(),
            where_group: ExprGroup::new(),
            returning_cols: Vec::new(),
            for_conflict: false,
        }
    }

    /// Create an UPDATE builder for ON CONFLICT DO UPDATE.
    pub(crate) fn new_for_conflict() -> Self {
        Self {
            table: String::new(),
            set_fields: Vec::new(),
            where_group: ExprGroup::new(),
            returning_cols: Vec::new(),
            for_conflict: true,
        }
    }

    /// Set a column value.
    pub fn set<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.set_fields.push((column.to_string(), SetField::Value(Param::new(value))));
        self
    }

    /// Set an optional column value (None => skip).
    pub fn set_opt<T: ToSql + Send + Sync + 'static>(self, column: &str, value: Option<T>) -> Self {
        if let Some(v) = value {
            self.set(column, v)
        } else {
            self
        }
    }

    /// Set a JSON column.
    pub fn set_json<T: serde::Serialize + Sync + Send>(
        self,
        column: &str,
        value: &T,
    ) -> serde_json::Result<Self> {
        let json_val = serde_json::to_value(value)?;
        Ok(self.set(column, json_val))
    }

    /// Set a raw SQL expression.
    pub fn set_raw(mut self, column: &str, expr: &str) -> Self {
        self.set_fields.push((column.to_string(), SetField::Raw(expr.to_string())));
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

    /// Build the UPDATE SQL and parameters.
    fn build_update(&self) -> (String, ParamList) {
        let mut params = ParamList::new();

        // Build SET clause
        let mut set_parts = Vec::new();
        for (col, field) in &self.set_fields {
            match field {
                SetField::Value(param) => {
                    let idx = params.push_param(param.clone());
                    set_parts.push(format!("{} = ${}", col, idx));
                }
                SetField::Raw(expr) => {
                    set_parts.push(format!("{} = {}", col, expr));
                }
            }
        }

        // If no SET fields, generate an error SQL
        if set_parts.is_empty() {
            return (
                format!("UPDATE {} SET _error_no_set_fields = 1 WHERE 1=0", self.table),
                params,
            );
        }

        let mut sql = format!("UPDATE {} SET {}", self.table, set_parts.join(", "));

        // Build WHERE clause with offset
        let (where_sql, where_params) = self.where_group.build_with_offset(params.len());
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

    /// Build for ON CONFLICT DO UPDATE (returns SET clause only with offset).
    pub(crate) fn build_for_conflict(&self, offset: usize) -> (String, ParamList) {
        let mut params = ParamList::new();
        let mut set_parts = Vec::new();

        for (col, field) in &self.set_fields {
            match field {
                SetField::Value(param) => {
                    let idx = params.push_param(param.clone()) + offset;
                    set_parts.push(format!("{} = ${}", col, idx));
                }
                SetField::Raw(expr) => {
                    set_parts.push(format!("{} = {}", col, expr));
                }
            }
        }

        if set_parts.is_empty() {
            return (String::new(), params);
        }

        let sql = format!(" SET {}", set_parts.join(", "));
        (sql, params)
    }

    /// Get the built SQL string (for debugging).
    pub fn to_sql(&self) -> String {
        self.build_update().0
    }
}

impl SqlQb for UpdateQb {
    fn build_sql(&self) -> String {
        self.build_update().0
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        vec![]
    }

    fn validate(&self) -> OrmResult<()> {
        if self.set_fields.is_empty() {
            return Err(OrmError::Validation(
                "UpdateQb: SET clause cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn query(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_update();
            let params_ref = params.as_refs();
            conn.query(&sql, &params_ref).await
        }
    }

    fn query_opt(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_update();
            let params_ref = params.as_refs();
            conn.query_opt(&sql, &params_ref).await
        }
    }

    fn query_one(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_update();
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

impl MutationQb for UpdateQb {
    fn execute(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_update();
            let params_ref = params.as_refs();
            conn.execute(&sql, &params_ref).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_update() {
        let qb = UpdateQb::new("users")
            .set("status", "inactive")
            .eq("id", 1i64);
        let sql = qb.to_sql();
        assert_eq!(sql, "UPDATE users SET status = $1 WHERE id = $2");
    }

    #[test]
    fn test_update_multiple_set() {
        let qb = UpdateQb::new("users")
            .set("name", "Alice")
            .set("email", "alice@example.com")
            .eq("id", 1i64);
        let sql = qb.to_sql();
        assert_eq!(sql, "UPDATE users SET name = $1, email = $2 WHERE id = $3");
    }

    #[test]
    fn test_update_with_raw() {
        let qb = UpdateQb::new("users")
            .set_raw("updated_at", "NOW()")
            .eq("id", 1i64);
        let sql = qb.to_sql();
        assert_eq!(sql, "UPDATE users SET updated_at = NOW() WHERE id = $1");
    }

    #[test]
    fn test_update_with_returning() {
        let qb = UpdateQb::new("users")
            .set("status", "inactive")
            .eq("id", 1i64)
            .returning("*");
        let sql = qb.to_sql();
        assert_eq!(sql, "UPDATE users SET status = $1 WHERE id = $2 RETURNING *");
    }

    #[test]
    fn test_update_complex_where() {
        let qb = UpdateQb::new("users")
            .set("status", "inactive")
            .eq("status", "active")
            .gt("age", 18i32)
            .in_list("role", vec!["user", "guest"]);
        let sql = qb.to_sql();
        assert!(sql.contains("status = $1"));
        assert!(sql.contains("status = $2"));
        assert!(sql.contains("age > $3"));
        assert!(sql.contains("role IN ($4, $5)"));
    }

    #[test]
    fn test_update_for_conflict() {
        let qb = UpdateQb::new_for_conflict()
            .set("email", "new@example.com")
            .set_raw("name", "EXCLUDED.name");
        let (sql, _) = qb.build_for_conflict(2);
        assert_eq!(sql, " SET email = $3, name = EXCLUDED.name");
    }
}
