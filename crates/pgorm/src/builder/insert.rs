use super::traits::{MutationBuilder, SqlBuilder};
use super::update::UpdateBuilder;
use tokio_postgres::types::ToSql;

/// INSERT builder.
pub struct InsertBuilder {
    /// Table name
    table: String,
    /// Column names
    columns: Vec<String>,
    /// VALUES expressions (e.g. "$1", "COALESCE($2, uuidv7())")
    value_exprs: Vec<String>,
    /// Params
    params: Vec<Box<dyn ToSql + Sync + Send>>,
    /// Current param counter
    param_count: usize,
    /// RETURNING columns
    returning_cols: Vec<String>,
    /// ON CONFLICT target (e.g. "(id)")
    conflict_target: Option<String>,
    /// ON CONFLICT action
    conflict_action: Option<ConflictAction>,
    /// Whether using UNNEST bulk insert mode
    unnest_mode: bool,
}

/// Conflict resolution action.
pub enum ConflictAction {
    DoNothing,
    DoUpdate(UpdateBuilder),
}

/// ON CONFLICT builder.
pub struct OnConflictBuilder<'a> {
    builder: &'a mut InsertBuilder,
    target: String,
}

impl<'a> OnConflictBuilder<'a> {
    /// DO NOTHING.
    pub fn do_nothing(self) -> &'a mut InsertBuilder {
        self.builder.conflict_target = Some(self.target);
        self.builder.conflict_action = Some(ConflictAction::DoNothing);
        self.builder
    }

    /// DO UPDATE.
    pub fn do_update(self) -> &'a mut UpdateBuilder {
        self.builder.conflict_target = Some(self.target);
        let update_builder = UpdateBuilder::new_for_conflict();
        self.builder.conflict_action = Some(ConflictAction::DoUpdate(update_builder));

        match self.builder.conflict_action.as_mut() {
            Some(ConflictAction::DoUpdate(ub)) => ub,
            _ => unreachable!(),
        }
    }
}

impl InsertBuilder {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            columns: Vec::new(),
            value_exprs: Vec::new(),
            params: Vec::new(),
            param_count: 0,
            returning_cols: Vec::new(),
            conflict_target: None,
            conflict_action: None,
            unnest_mode: false,
        }
    }

    /// Set a column value.
    pub fn set<T>(&mut self, column: &str, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        self.columns.push(column.to_string());
        self.value_exprs.push(format!("${}", self.param_count));
        self.params.push(Box::new(value));
        self
    }

    /// Set an optional column value (None => skip).
    pub fn set_opt<T>(&mut self, column: &str, value: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = value {
            self.set(column, v);
        }
        self
    }

    /// Set an optional value, using a default if None.
    pub fn set_default<T>(&mut self, column: &str, value: Option<T>, default: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.set(column, value.unwrap_or(default))
    }

    /// Set an optional value, using a closure for default if None.
    pub fn set_default_with<T, F>(&mut self, column: &str, value: Option<T>, default_fn: F) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
        F: FnOnce() -> T,
    {
        self.set(column, value.unwrap_or_else(default_fn))
    }

    /// Set a UUID column using `COALESCE($n, uuidv7())`.
    pub fn set_uuidv7<T>(&mut self, column: &str, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        self.columns.push(column.to_string());
        self.value_exprs
            .push(format!("COALESCE(${}, uuidv7())", self.param_count));
        self.params.push(Box::new(value));
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

    /// Set a raw SQL expression (no params).
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn set_raw(&mut self, column: &str, expr: &str) -> &mut Self {
        self.columns.push(column.to_string());
        self.value_exprs.push(expr.to_string());
        self
    }

    /// Set RETURNING columns (string form).
    pub fn returning(&mut self, cols: &str) -> &mut Self {
        self.returning_cols = vec![cols.to_string()];
        self
    }

    /// Set RETURNING columns (array form).
    pub fn returning_cols(&mut self, cols: &[&str]) -> &mut Self {
        self.returning_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Start ON CONFLICT clause.
    pub fn on_conflict<'a>(&'a mut self, target: &str) -> OnConflictBuilder<'a> {
        OnConflictBuilder {
            builder: self,
            target: target.to_string(),
        }
    }

    /// Add an array param and enable UNNEST bulk insert mode.
    ///
    /// Once enabled, all columns must be set via `unnest_list`.
    pub fn unnest_list<T>(&mut self, column: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.unnest_mode = true;
        self.param_count += 1;
        self.columns.push(column.to_string());
        self.params.push(Box::new(values));
        self
    }
}

impl SqlBuilder for InsertBuilder {
    fn build_sql(&self) -> String {
        let mut sql = if self.unnest_mode {
            let placeholders: Vec<String> = (1..=self.params.len()).map(|i| format!("${}", i)).collect();
            format!(
                "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                self.table,
                self.columns.join(", "),
                placeholders.join(", ")
            )
        } else if self.columns.is_empty() {
            format!("INSERT INTO {} DEFAULT VALUES", self.table)
        } else {
            format!(
                "INSERT INTO {} ({}) VALUES ({})",
                self.table,
                self.columns.join(", "),
                self.value_exprs.join(", ")
            )
        };

        if let Some(ref target) = self.conflict_target {
            sql.push_str(" ON CONFLICT ");
            sql.push_str(target);

            if let Some(ref action) = self.conflict_action {
                match action {
                    ConflictAction::DoNothing => {
                        sql.push_str(" DO NOTHING");
                    }
                    ConflictAction::DoUpdate(update_builder) => {
                        sql.push_str(" DO UPDATE");
                        let (update_sql, _) = update_builder.build_with_offset(self.params.len());
                        sql.push_str(&update_sql);
                    }
                }
            }
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        sql
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        let mut params: Vec<&(dyn ToSql + Sync)> = self
            .params
            .iter()
            .map(|v| &**v as &(dyn ToSql + Sync))
            .collect();

        if let Some(ConflictAction::DoUpdate(ref update_builder)) = self.conflict_action {
            params.extend(update_builder.params_ref());
        }

        params
    }

    fn validate(&self) -> crate::error::OrmResult<()> {
        if self.unnest_mode {
            if self.columns.is_empty() {
                return Err(crate::error::OrmError::Validation(
                    "InsertBuilder: unnest mode requires at least one column".to_string(),
                ));
            }
            if !self.value_exprs.is_empty() {
                return Err(crate::error::OrmError::Validation(
                    "InsertBuilder: cannot mix unnest_list and standard set methods".to_string(),
                ));
            }
            if self.columns.len() != self.params.len() {
                return Err(crate::error::OrmError::Validation(format!(
                    "InsertBuilder: unnest mode internal invariant violated: columns({}) != params({})",
                    self.columns.len(),
                    self.params.len()
                )));
            }
        }

        if let Some(ConflictAction::DoUpdate(ref ub)) = self.conflict_action {
            ub.validate()?;
        }

        Ok(())
    }
}

impl MutationBuilder for InsertBuilder {}
