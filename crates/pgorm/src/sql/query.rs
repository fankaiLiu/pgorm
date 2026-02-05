use super::stream::FromRowStream;
use super::{starts_with_keyword, strip_sql_prefix};
use crate::client::{GenericClient, RowStream, StreamingClient};
use crate::error::{OrmError, OrmResult};
use crate::row::FromRow;
use std::sync::Arc;
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, ToSql};

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

    // ==================== Execution ====================

    /// Execute the query and return all rows.
    pub async fn fetch_all(&self, conn: &impl GenericClient) -> OrmResult<Vec<Row>> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_tagged(tag, &self.sql, &params).await,
            None => conn.query(&self.sql, &params).await,
        }
    }

    // ==================== Streaming execution ====================

    /// Execute the query and return a row stream.
    pub async fn stream(&self, conn: &impl StreamingClient) -> OrmResult<RowStream> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_stream_tagged(tag, &self.sql, &params).await,
            None => conn.query_stream(&self.sql, &params).await,
        }
    }

    /// Execute the query and return a stream of `T`.
    pub async fn stream_as<T: FromRow>(
        &self,
        conn: &impl StreamingClient,
    ) -> OrmResult<FromRowStream<T>> {
        let stream = self.stream(conn).await?;
        Ok(FromRowStream::new(stream))
    }

    /// Execute the query and return all rows mapped to `T`.
    pub async fn fetch_all_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all(conn).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute the query and return the **first** row.
    ///
    /// Semantics:
    /// - 0 rows: returns [`OrmError::NotFound`]
    /// - 1 row: returns that row
    /// - multiple rows: returns the first row (does **not** error)
    ///
    /// If you need strict row-count checking (i.e. error on multiple rows), use
    /// [`Query::fetch_one_strict`].
    pub async fn fetch_one(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_one_tagged(tag, &self.sql, &params).await,
            None => conn.query_one(&self.sql, &params).await,
        }
    }

    /// Execute the query and return the **first** row mapped to `T`.
    pub async fn fetch_one_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one(conn).await?;
        T::from_row(&row)
    }

    /// Execute the query and return the first row, if any.
    pub async fn fetch_opt(&self, conn: &impl GenericClient) -> OrmResult<Option<Row>> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_opt_tagged(tag, &self.sql, &params).await,
            None => conn.query_opt(&self.sql, &params).await,
        }
    }

    /// Execute the query and return at most one row mapped to `T`.
    pub async fn fetch_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt(conn).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute the query and return affected row count.
    pub async fn execute(&self, conn: &impl GenericClient) -> OrmResult<u64> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.execute_tagged(tag, &self.sql, &params).await,
            None => conn.execute(&self.sql, &params).await,
        }
    }

    // ==================== Tagged execution ====================

    pub async fn fetch_all_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<Row>> {
        let params = self.params_ref();
        conn.query_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_all_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all_tagged(conn, tag).await?;
        rows.iter().map(T::from_row).collect()
    }

    pub async fn fetch_one_tagged(&self, conn: &impl GenericClient, tag: &str) -> OrmResult<Row> {
        let params = self.params_ref();
        conn.query_one_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_one_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<T> {
        let row = self.fetch_one_tagged(conn, tag).await?;
        T::from_row(&row)
    }

    // ==================== Strict execution ====================

    /// Execute the query and require that it returns **exactly one** row.
    pub async fn fetch_one_strict(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_one_strict_tagged(tag, &self.sql, &params).await,
            None => conn.query_one_strict(&self.sql, &params).await,
        }
    }

    /// Execute the query and require that it returns **exactly one** row mapped to `T`.
    pub async fn fetch_one_strict_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one_strict(conn).await?;
        T::from_row(&row)
    }

    /// Execute the query and require that it returns **exactly one** row, associating a tag.
    pub async fn fetch_one_strict_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Row> {
        let params = self.params_ref();
        conn.query_one_strict_tagged(tag, &self.sql, &params).await
    }

    /// Execute the query and require that it returns **exactly one** row mapped to `T`, associating a tag.
    pub async fn fetch_one_strict_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<T> {
        let row = self.fetch_one_strict_tagged(conn, tag).await?;
        T::from_row(&row)
    }

    pub async fn fetch_opt_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Option<Row>> {
        let params = self.params_ref();
        conn.query_opt_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_opt_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt_tagged(conn, tag).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    pub async fn execute_tagged(&self, conn: &impl GenericClient, tag: &str) -> OrmResult<u64> {
        let params = self.params_ref();
        conn.execute_tagged(tag, &self.sql, &params).await
    }

    // ==================== Convenience APIs ====================

    pub async fn fetch_scalar_one<'a, T>(&self, conn: &impl GenericClient) -> OrmResult<T>
    where
        T: for<'b> FromSql<'b> + Send + Sync,
    {
        let row = self.fetch_one(conn).await?;
        row.try_get(0)
            .map_err(|e| OrmError::decode("0", e.to_string()))
    }

    pub async fn fetch_scalar_opt<'a, T>(&self, conn: &impl GenericClient) -> OrmResult<Option<T>>
    where
        T: for<'b> FromSql<'b> + Send + Sync,
    {
        let row = self.fetch_opt(conn).await?;
        match row {
            Some(r) => r
                .try_get(0)
                .map(Some)
                .map_err(|e| OrmError::decode("0", e.to_string())),
            None => Ok(None),
        }
    }

    pub async fn fetch_scalar_all<'a, T>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>>
    where
        T: for<'b> FromSql<'b> + Send + Sync,
    {
        let rows = self.fetch_all(conn).await?;
        rows.iter()
            .map(|r| {
                r.try_get(0)
                    .map_err(|e| OrmError::decode("0", e.to_string()))
            })
            .collect()
    }

    pub async fn exists(&self, conn: &impl GenericClient) -> OrmResult<bool> {
        let inner_sql = self.sql.trim_end();
        let inner_sql = inner_sql.strip_suffix(';').unwrap_or(inner_sql).trim_end();

        let trimmed = strip_sql_prefix(inner_sql);
        if !starts_with_keyword(trimmed, "SELECT") && !starts_with_keyword(trimmed, "WITH") {
            return Err(OrmError::Validation(
                "exists() only works with SELECT statements (including WITH ... SELECT)"
                    .to_string(),
            ));
        }

        let wrapped_sql = format!("SELECT EXISTS({inner_sql})");
        let params = self.params_ref();
        let row = match self.tag.as_deref() {
            Some(tag) => conn.query_one_tagged(tag, &wrapped_sql, &params).await?,
            None => conn.query_one(&wrapped_sql, &params).await?,
        };
        row.try_get(0)
            .map_err(|e| OrmError::decode("0", e.to_string()))
    }
}
