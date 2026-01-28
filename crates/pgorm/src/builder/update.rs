use super::traits::{MutationBuilder, SqlBuilder};
use super::where_builder::WhereBuilder;
use tokio_postgres::types::ToSql;

/// SET field value type.
pub enum SetField {
    /// Parameterized value
    Value(Box<dyn ToSql + Sync + Send>),
    /// Raw SQL expression
    Raw(String),
}

/// UPDATE builder.
pub struct UpdateBuilder {
    /// Table name
    table: String,
    /// SET clauses (column, value)
    set_fields: Vec<(String, SetField)>,
    /// WHERE conditions
    where_builder: WhereBuilder,
    /// RETURNING columns
    returning_cols: Vec<String>,
}

impl UpdateBuilder {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            set_fields: Vec::new(),
            where_builder: WhereBuilder::new(),
            returning_cols: Vec::new(),
        }
    }

    /// Builder used for ON CONFLICT DO UPDATE (no table prefix).
    pub fn new_for_conflict() -> Self {
        Self::new("")
    }

    /// Set a column.
    pub fn set<T>(&mut self, column: &str, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.set_fields
            .push((column.to_string(), SetField::Value(Box::new(value))));
        self
    }

    /// Set an optional column (None => skip).
    pub fn set_opt<T>(&mut self, column: &str, value: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = value {
            self.set(column, v);
        }
        self
    }

    /// Set a JSON column.
    pub fn set_json<T>(&mut self, column: &str, value: &T) -> serde_json::Result<&mut Self>
    where
        T: serde::Serialize + Sync + Send,
    {
        let json_val = serde_json::to_value(value)?;
        Ok(self.set(column, json_val))
    }

    /// Set a raw SQL expression.
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn set_raw(&mut self, column: &str, expr: &str) -> &mut Self {
        self.set_fields
            .push((column.to_string(), SetField::Raw(expr.to_string())));
        self
    }

    // ==================== Conditions (delegated to WhereBuilder) ====================

    /// Add AND equality condition.
    pub fn and_eq<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_eq(col, val);
        self
    }

    /// Add AND not-equal condition.
    pub fn and_ne<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_ne(col, val);
        self
    }

    /// Add AND IN (...) condition.
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_in(col, values);
        self
    }

    /// Add AND < condition.
    pub fn and_lt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_lt(col, val);
        self
    }

    /// Add AND <= condition.
    pub fn and_lte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_lte(col, val);
        self
    }

    /// Add AND > condition.
    pub fn and_gt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_gt(col, val);
        self
    }

    /// Add AND >= condition.
    pub fn and_gte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_gte(col, val);
        self
    }

    /// Add raw WHERE condition.
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn and_raw(&mut self, sql: &str) -> &mut Self {
        self.where_builder.and_raw(sql);
        self
    }

    pub fn and_is_null(&mut self, col: &str) -> &mut Self {
        self.where_builder.and_is_null(col);
        self
    }

    pub fn and_is_not_null(&mut self, col: &str) -> &mut Self {
        self.where_builder.and_is_not_null(col);
        self
    }

    /// Set RETURNING (string form).
    pub fn returning(&mut self, cols: &str) -> &mut Self {
        self.returning_cols = vec![cols.to_string()];
        self
    }

    /// Set RETURNING (array form).
    pub fn returning_cols(&mut self, cols: &[&str]) -> &mut Self {
        self.returning_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Count SET params (params that contribute to placeholders).
    fn count_set_params(&self) -> usize {
        self.set_fields
            .iter()
            .filter(|(_, field)| matches!(field, SetField::Value(_)))
            .count()
    }

    /// Build SQL and params with an index offset.
    pub fn build_with_offset(&self, offset: usize) -> (String, Vec<&(dyn ToSql + Sync)>) {
        let mut sql = String::new();
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
        let mut current_idx = offset;

        let mut set_parts = Vec::new();
        for (col, field) in &self.set_fields {
            match field {
                SetField::Value(val) => {
                    current_idx += 1;
                    set_parts.push(format!("{} = ${}", col, current_idx));
                    params.push(&**val);
                }
                SetField::Raw(expr) => {
                    set_parts.push(format!("{} = {}", col, expr));
                }
            }
        }

        if set_parts.is_empty() {
            return (String::new(), Vec::new());
        }

        sql.push_str(" SET ");
        sql.push_str(&set_parts.join(", "));

        // Build WHERE clause with correct offset
        if !self.where_builder.is_empty() {
            // We need to rebuild the where clause with the correct offset
            // Since WhereBuilder stores conditions with placeholders already,
            // we need to recalculate them
            sql.push_str(" WHERE ");

            let where_clause = self.build_where_with_offset(current_idx);
            sql.push_str(&where_clause);

            // Add where params
            params.extend(self.where_builder.params_ref());
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        (sql, params)
    }

    /// Build WHERE clause with correct param offset.
    fn build_where_with_offset(&self, offset: usize) -> String {
        // The WhereBuilder already built the clause with $1, $2, etc.
        // We need to replace these with the correct offset
        let base_clause = self.where_builder.build_clause();
        let mut result = base_clause;

        // Replace placeholders in reverse order to avoid $1 matching $10
        for i in (1..=self.where_builder.param_count()).rev() {
            let old = format!("${}", i);
            let new = format!("${}", i + offset);
            result = result.replace(&old, &new);
        }

        result
    }
}

impl SqlBuilder for UpdateBuilder {
    fn build_sql(&self) -> String {
        let set_param_count = self.count_set_params();
        let (parts, _) = self.build_with_offset(0);

        if parts.is_empty() {
            return format!("UPDATE {} SET _error_no_set_fields = 1 WHERE 1=0", self.table);
        }

        // Rebuild properly for standalone UPDATE
        let mut sql = format!("UPDATE {}", self.table);

        let mut current_idx = 0;
        let mut set_parts = Vec::new();
        for (col, field) in &self.set_fields {
            match field {
                SetField::Value(_) => {
                    current_idx += 1;
                    set_parts.push(format!("{} = ${}", col, current_idx));
                }
                SetField::Raw(expr) => {
                    set_parts.push(format!("{} = {}", col, expr));
                }
            }
        }

        sql.push_str(" SET ");
        sql.push_str(&set_parts.join(", "));

        if !self.where_builder.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.build_where_with_offset(set_param_count));
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        sql
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();

        // SET params first
        for (_, field) in &self.set_fields {
            if let SetField::Value(val) = field {
                params.push(&**val);
            }
        }

        // WHERE params second
        params.extend(self.where_builder.params_ref());

        params
    }

    fn validate(&self) -> crate::error::OrmResult<()> {
        if self.set_fields.is_empty() {
            return Err(crate::error::OrmError::Validation(
                "UpdateBuilder: SET clause cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

impl MutationBuilder for UpdateBuilder {}
