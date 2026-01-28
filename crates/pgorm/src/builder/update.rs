use super::traits::{MutationBuilder, SqlBuilder};
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
    /// WHERE clauses
    where_clauses: Vec<WhereClause>,
    /// RETURNING columns
    returning_cols: Vec<String>,
}

enum WhereClause {
    Raw(String),
    Template {
        sql_template: String,
        params: Vec<Box<dyn ToSql + Sync + Send>>,
    },
}

impl UpdateBuilder {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            set_fields: Vec::new(),
            where_clauses: Vec::new(),
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

    /// Add AND equality condition.
    pub fn and_eq<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_clauses.push(WhereClause::Template {
            sql_template: format!("{} = ?", col),
            params: vec![Box::new(val)],
        });
        self
    }

    /// Add AND IN (...) condition.
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            self.where_clauses.push(WhereClause::Raw("1=0".to_string()));
            return self;
        }

        let placeholders = vec!["?"; values.len()].join(", ");
        let params = values
            .into_iter()
            .map(|value| Box::new(value) as Box<dyn ToSql + Sync + Send>)
            .collect();

        self.where_clauses.push(WhereClause::Template {
            sql_template: format!("{} IN ({})", col, placeholders),
            params,
        });
        self
    }

    /// Add raw WHERE condition.
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn and_raw(&mut self, sql: &str) -> &mut Self {
        self.where_clauses.push(WhereClause::Raw(sql.to_string()));
        self
    }

    pub fn and_is_null(&mut self, col: &str) -> &mut Self {
        self.where_clauses
            .push(WhereClause::Raw(format!("{} IS NULL", col)));
        self
    }

    pub fn and_is_not_null(&mut self, col: &str) -> &mut Self {
        self.where_clauses
            .push(WhereClause::Raw(format!("{} IS NOT NULL", col)));
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

        let mut where_parts = Vec::new();
        for clause in &self.where_clauses {
            match clause {
                WhereClause::Raw(sql) => {
                    where_parts.push(sql.clone());
                }
                WhereClause::Template {
                    sql_template,
                    params: clause_params,
                } => {
                    let mut final_sql = sql_template.clone();
                    for _ in clause_params {
                        current_idx += 1;
                        let placeholder = format!("${}", current_idx);
                        final_sql = final_sql.replacen('?', &placeholder, 1);
                    }
                    where_parts.push(final_sql);
                    for param in clause_params {
                        params.push(&**param);
                    }
                }
            }
        }

        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        (sql, params)
    }
}

impl SqlBuilder for UpdateBuilder {
    fn build_sql(&self) -> String {
        let (parts, _) = self.build_with_offset(0);
        if parts.is_empty() {
            return format!("UPDATE {} SET _error_no_set_fields = 1 WHERE 1=0", self.table);
        }
        format!("UPDATE {}{}", self.table, parts)
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
        for (_, field) in &self.set_fields {
            if let SetField::Value(val) = field {
                params.push(&**val);
            }
        }
        for clause in &self.where_clauses {
            if let WhereClause::Template {
                params: clause_params,
                ..
            } = clause
            {
                for param in clause_params {
                    params.push(&**param);
                }
            }
        }
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
