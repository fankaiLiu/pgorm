//! INSERT query builder using the unified expression layer.

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::qb::param::{Param, ParamList};
use crate::qb::traits::{MutationQb, SqlQb};
use crate::qb::update::UpdateQb;
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Conflict resolution action.
#[derive(Clone, Debug)]
pub enum ConflictAction {
    /// DO NOTHING
    DoNothing,
    /// DO UPDATE with SET clauses
    DoUpdate(UpdateQb),
}

/// INSERT query builder with unified parameter handling.
#[derive(Clone, Debug)]
pub struct InsertQb {
    /// Table name
    table: String,
    /// Column names
    columns: Vec<String>,
    /// Value expressions (e.g., "$1", "COALESCE($2, uuidv7())")
    value_exprs: Vec<ValueExpr>,
    /// RETURNING columns
    returning_cols: Vec<String>,
    /// ON CONFLICT target
    conflict_target: Option<String>,
    /// ON CONFLICT action
    conflict_action: Option<ConflictAction>,
    /// Whether using UNNEST bulk insert mode
    unnest_mode: bool,
    /// UNNEST arrays for bulk insert
    unnest_arrays: Vec<Param>,
}

/// Value expression for INSERT.
#[derive(Clone, Debug)]
enum ValueExpr {
    /// Parameterized value
    Param(Param),
    /// Raw SQL expression with a parameter (e.g., "COALESCE($n, uuidv7())")
    ParamWithExpr { param: Param, expr_template: String },
    /// Raw SQL expression without parameter
    Raw(String),
}

impl InsertQb {
    /// Create a new INSERT query builder.
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            columns: Vec::new(),
            value_exprs: Vec::new(),
            returning_cols: Vec::new(),
            conflict_target: None,
            conflict_action: None,
            unnest_mode: false,
            unnest_arrays: Vec::new(),
        }
    }

    /// Set a column value.
    pub fn set<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.columns.push(column.to_string());
        self.value_exprs.push(ValueExpr::Param(Param::new(value)));
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

    /// Set an optional value, using a default if None.
    pub fn set_default<T: ToSql + Send + Sync + 'static>(
        self,
        column: &str,
        value: Option<T>,
        default: T,
    ) -> Self {
        self.set(column, value.unwrap_or(default))
    }

    /// Set an optional value, using a closure for default if None.
    pub fn set_default_with<T: ToSql + Send + Sync + 'static, F: FnOnce() -> T>(
        self,
        column: &str,
        value: Option<T>,
        default_fn: F,
    ) -> Self {
        self.set(column, value.unwrap_or_else(default_fn))
    }

    /// Set a UUID column using `COALESCE($n, uuidv7())`.
    pub fn set_uuidv7<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.columns.push(column.to_string());
        self.value_exprs.push(ValueExpr::ParamWithExpr {
            param: Param::new(value),
            expr_template: "COALESCE(${}, uuidv7())".to_string(),
        });
        self
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

    /// Set a raw SQL expression (no params).
    pub fn set_raw(mut self, column: &str, expr: &str) -> Self {
        self.columns.push(column.to_string());
        self.value_exprs.push(ValueExpr::Raw(expr.to_string()));
        self
    }

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

    /// Start ON CONFLICT clause.
    pub fn on_conflict(self, target: &str) -> OnConflictQb {
        OnConflictQb {
            builder: self,
            target: target.to_string(),
        }
    }

    /// Add an array param and enable UNNEST bulk insert mode.
    pub fn unnest_list<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Vec<T>) -> Self {
        self.unnest_mode = true;
        self.columns.push(column.to_string());
        self.unnest_arrays.push(Param::new(values));
        self
    }

    /// Build the SQL and parameters.
    fn build_insert(&self) -> (String, ParamList) {
        let mut params = ParamList::new();

        let sql = if self.unnest_mode {
            // UNNEST bulk insert
            let placeholders: Vec<String> = (1..=self.unnest_arrays.len())
                .map(|_| {
                    let idx = params.len() + 1;
                    format!("${}", idx)
                })
                .collect();

            // Add unnest array params
            for arr in &self.unnest_arrays {
                params.push_param(arr.clone());
            }

            format!(
                "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                self.table,
                self.columns.join(", "),
                placeholders.join(", ")
            )
        } else if self.columns.is_empty() {
            format!("INSERT INTO {} DEFAULT VALUES", self.table)
        } else {
            // Build value expressions
            let mut value_parts = Vec::new();
            for expr in &self.value_exprs {
                match expr {
                    ValueExpr::Param(p) => {
                        let idx = params.push_param(p.clone());
                        value_parts.push(format!("${}", idx));
                    }
                    ValueExpr::ParamWithExpr { param, expr_template } => {
                        let idx = params.push_param(param.clone());
                        value_parts.push(expr_template.replace("{}", &idx.to_string()));
                    }
                    ValueExpr::Raw(raw) => {
                        value_parts.push(raw.clone());
                    }
                }
            }

            format!(
                "INSERT INTO {} ({}) VALUES ({})",
                self.table,
                self.columns.join(", "),
                value_parts.join(", ")
            )
        };

        let mut full_sql = sql;

        // ON CONFLICT
        if let Some(ref target) = self.conflict_target {
            full_sql.push_str(" ON CONFLICT ");
            full_sql.push_str(target);

            if let Some(ref action) = self.conflict_action {
                match action {
                    ConflictAction::DoNothing => {
                        full_sql.push_str(" DO NOTHING");
                    }
                    ConflictAction::DoUpdate(update_qb) => {
                        full_sql.push_str(" DO UPDATE");
                        let (update_sql, update_params) = update_qb.build_for_conflict(params.len());
                        full_sql.push_str(&update_sql);
                        params.extend(&update_params);
                    }
                }
            }
        }

        // RETURNING
        if !self.returning_cols.is_empty() {
            full_sql.push_str(" RETURNING ");
            full_sql.push_str(&self.returning_cols.join(", "));
        }

        (full_sql, params)
    }

    /// Get the built SQL string (for debugging).
    pub fn to_sql(&self) -> String {
        self.build_insert().0
    }
}

impl SqlQb for InsertQb {
    fn build_sql(&self) -> String {
        self.build_insert().0
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        vec![]
    }

    fn validate(&self) -> OrmResult<()> {
        if self.unnest_mode {
            if self.columns.is_empty() {
                return Err(OrmError::Validation(
                    "InsertQb: unnest mode requires at least one column".to_string(),
                ));
            }
            if !self.value_exprs.is_empty() {
                return Err(OrmError::Validation(
                    "InsertQb: cannot mix unnest_list and standard set methods".to_string(),
                ));
            }
            if self.columns.len() != self.unnest_arrays.len() {
                return Err(OrmError::Validation(format!(
                    "InsertQb: unnest mode internal invariant violated: columns({}) != arrays({})",
                    self.columns.len(),
                    self.unnest_arrays.len()
                )));
            }
        }

        if let Some(ConflictAction::DoUpdate(ref ub)) = self.conflict_action {
            ub.validate()?;
        }

        Ok(())
    }

    fn query(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_insert();
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
            let (sql, params) = self.build_insert();
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
            let (sql, params) = self.build_insert();
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

impl MutationQb for InsertQb {
    fn execute(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_insert();
            let params_ref = params.as_refs();
            conn.execute(&sql, &params_ref).await
        }
    }
}

/// ON CONFLICT builder.
pub struct OnConflictQb {
    builder: InsertQb,
    target: String,
}

impl OnConflictQb {
    /// DO NOTHING.
    pub fn do_nothing(mut self) -> InsertQb {
        self.builder.conflict_target = Some(self.target);
        self.builder.conflict_action = Some(ConflictAction::DoNothing);
        self.builder
    }

    /// DO UPDATE - returns an UpdateQb that can be configured and then finalized.
    pub fn do_update(mut self) -> OnConflictUpdateQb {
        self.builder.conflict_target = Some(self.target);
        OnConflictUpdateQb {
            insert_builder: self.builder,
            update_builder: UpdateQb::new_for_conflict(),
        }
    }
}

/// Builder for ON CONFLICT DO UPDATE.
pub struct OnConflictUpdateQb {
    insert_builder: InsertQb,
    update_builder: UpdateQb,
}

impl OnConflictUpdateQb {
    /// Set a column in the UPDATE.
    pub fn set<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.update_builder = self.update_builder.set(column, value);
        self
    }

    /// Set an optional column in the UPDATE.
    pub fn set_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.update_builder = self.update_builder.set_opt(column, value);
        self
    }

    /// Set a raw SQL expression in the UPDATE.
    pub fn set_raw(mut self, column: &str, expr: &str) -> Self {
        self.update_builder = self.update_builder.set_raw(column, expr);
        self
    }

    /// Set column to EXCLUDED value.
    pub fn set_excluded(mut self, column: &str) -> Self {
        self.update_builder = self.update_builder.set_raw(column, &format!("EXCLUDED.{}", column));
        self
    }

    /// Finish and return the InsertQb.
    pub fn finish(mut self) -> InsertQb {
        self.insert_builder.conflict_action = Some(ConflictAction::DoUpdate(self.update_builder));
        self.insert_builder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_insert() {
        let qb = InsertQb::new("users")
            .set("username", "alice")
            .set("email", "alice@example.com");
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO users (username, email) VALUES ($1, $2)");
    }

    #[test]
    fn test_insert_with_returning() {
        let qb = InsertQb::new("users")
            .set("username", "alice")
            .returning("id");
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO users (username) VALUES ($1) RETURNING id");
    }

    #[test]
    fn test_insert_uuidv7() {
        let qb = InsertQb::new("users")
            .set_uuidv7("id", Option::<String>::None)
            .set("username", "alice");
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO users (id, username) VALUES (COALESCE($1, uuidv7()), $2)");
    }

    #[test]
    fn test_insert_with_raw() {
        let qb = InsertQb::new("users")
            .set("username", "alice")
            .set_raw("created_at", "NOW()");
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO users (username, created_at) VALUES ($1, NOW())");
    }

    #[test]
    fn test_insert_on_conflict_do_nothing() {
        let qb = InsertQb::new("users")
            .set("username", "alice")
            .on_conflict("(username)")
            .do_nothing();
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO users (username) VALUES ($1) ON CONFLICT (username) DO NOTHING");
    }

    #[test]
    fn test_insert_on_conflict_do_update() {
        let qb = InsertQb::new("users")
            .set("username", "alice")
            .set("email", "alice@example.com")
            .on_conflict("(username)")
            .do_update()
            .set_excluded("email")
            .finish();
        let sql = qb.to_sql();
        assert!(sql.contains("ON CONFLICT (username) DO UPDATE SET email = EXCLUDED.email"));
    }

    #[test]
    fn test_insert_default_values() {
        let qb = InsertQb::new("audit_log");
        let sql = qb.to_sql();
        assert_eq!(sql, "INSERT INTO audit_log DEFAULT VALUES");
    }
}
