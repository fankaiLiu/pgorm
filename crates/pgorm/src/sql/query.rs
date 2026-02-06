use std::sync::Arc;
use tokio_postgres::types::ToSql;

/// A SQL string with pre-numbered placeholders (`$1, $2, ...`) plus bound parameters.
///
/// Use this when you already have a complete SQL string and just want to bind values.
#[must_use]
pub struct Query {
    sql: String,
    params: Vec<Arc<dyn ToSql + Sync + Send>>,
    tag: Option<String>,
}

impl Query {
    /// Create a new pre-numbered query.
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
            tag: None,
        }
    }

    /// Associate a tag for monitoring/observability.
    ///
    /// # Example
    /// ```ignore
    /// let user: User = pgorm::query("SELECT id, username FROM users WHERE id = $1")
    ///     .tag("users.by_id")
    ///     .bind(1_i64)
    ///     .fetch_one_as(&pg)
    ///     .await?;
    /// ```
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Bind a parameter value.
    ///
    /// This does not modify the SQL string; it only appends the value to the
    /// parameter list. The SQL string must already contain `$1, $2, ...`.
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.params.push(Arc::new(value));
        self
    }

    /// Access the SQL string.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Parameter refs compatible with `tokio-postgres`.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|p| p.as_ref() as &(dyn ToSql + Sync))
            .collect()
    }

    impl_query_exec! {
        prepare(self) {
            let sql = &self.sql;
            let params = self.params_ref();
            let tag = self.tag.as_deref();
            (sql, params, tag)
        }
    }
}
