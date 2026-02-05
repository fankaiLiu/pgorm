use super::parts::SqlPart;
use super::stream::FromRowStream;
use super::{starts_with_keyword, strip_sql_prefix};
use crate::bulk::{DeleteManyBuilder, SetExpr, UpdateManyBuilder};
use crate::client::{GenericClient, RowStream, StreamingClient};
use crate::condition::Condition;
use crate::cte::WithBuilder;
use crate::error::{OrmError, OrmResult};
use crate::ident::IntoIdent;
use crate::row::FromRow;
use std::sync::Arc;
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, ToSql};

/// A SQL-first, parameter-safe dynamic SQL builder.
///
/// `Sql` stores SQL pieces and parameters separately and generates `$1, $2, ...`
/// placeholders automatically in the final SQL string.
#[must_use]
pub struct Sql {
    parts: Vec<SqlPart>,
    params: Vec<Arc<dyn ToSql + Sync + Send>>,
    tag: Option<String>,
}

impl Sql {
    /// Create a new builder with an initial SQL fragment.
    pub fn new(initial_sql: impl Into<String>) -> Self {
        Self {
            parts: vec![SqlPart::Raw(initial_sql.into())],
            params: Vec::new(),
            tag: None,
        }
    }

    /// Create an empty builder.
    pub fn empty() -> Self {
        Self {
            parts: Vec::new(),
            params: Vec::new(),
            tag: None,
        }
    }

    /// Associate a tag for monitoring/observability.
    ///
    /// # Example
    /// ```ignore
    /// let users: Vec<User> = pgorm::sql("SELECT * FROM users WHERE username ILIKE ")
    ///     .tag("users.search")
    ///     .push_bind("%admin%")
    ///     .fetch_all_as(&pg)
    ///     .await?;
    /// ```
    pub fn tag(&mut self, tag: impl Into<String>) -> &mut Self {
        self.tag = Some(tag.into());
        self
    }

    /// Append raw SQL (no parameters).
    pub fn push(&mut self, sql: &str) -> &mut Self {
        if sql.is_empty() {
            return self;
        }

        match self.parts.last_mut() {
            Some(SqlPart::Raw(last)) => last.push_str(sql),
            _ => self.parts.push(SqlPart::Raw(sql.to_string())),
        }
        self
    }

    /// Append a parameter placeholder and bind its value.
    pub fn push_bind<T>(&mut self, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.parts.push(SqlPart::Param);
        self.params.push(Arc::new(value));
        self
    }

    pub(crate) fn push_bind_value(&mut self, value: Arc<dyn ToSql + Sync + Send>) -> &mut Self {
        self.parts.push(SqlPart::Param);
        self.params.push(value);
        self
    }

    /// Append a comma-separated list of placeholders and bind all values.
    ///
    /// If `values` is empty, this appends `NULL` (so `IN (NULL)` is valid SQL).
    pub fn push_bind_list<T>(&mut self, values: impl IntoIterator<Item = T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        let mut iter = values.into_iter();
        let Some(first) = iter.next() else {
            return self.push("NULL");
        };

        self.push_bind(first);
        for v in iter {
            self.push(", ");
            self.push_bind(v);
        }
        self
    }

    /// Append another `Sql` fragment, consuming it.
    pub fn push_sql(&mut self, mut other: Sql) -> &mut Self {
        self.parts.append(&mut other.parts);
        self.params.append(&mut other.params);
        if self.tag.is_none() {
            self.tag = other.tag;
        }
        self
    }

    /// Append a SQL identifier (schema/table/column) safely.
    ///
    /// This does **not** use parameters (Postgres doesn't allow parameterizing
    /// identifiers). To prevent SQL injection when identifiers are dynamic, this
    /// parses and validates identifiers via [`crate::Ident`].
    pub fn push_ident<I>(&mut self, ident: I) -> OrmResult<&mut Self>
    where
        I: IntoIdent,
    {
        let ident = ident.into_ident()?;
        Ok(self.push_ident_ref(&ident))
    }

    pub(crate) fn push_ident_ref(&mut self, ident: &crate::Ident) -> &mut Self {
        match self.parts.last_mut() {
            Some(SqlPart::Raw(last)) => ident.write_sql(last),
            _ => {
                let mut s = String::new();
                ident.write_sql(&mut s);
                self.parts.push(SqlPart::Raw(s));
            }
        }
        self
    }

    /// Render SQL with `$1, $2, ...` placeholders.
    pub fn to_sql(&self) -> String {
        fn decimal_digits(mut n: usize) -> usize {
            let mut digits = 1;
            while n >= 10 {
                n /= 10;
                digits += 1;
            }
            digits
        }

        // Pre-size to avoid repeated reallocations (hot path).
        let mut idx: usize = 0;
        let mut cap: usize = 0;
        for part in &self.parts {
            match part {
                SqlPart::Raw(s) => cap += s.len(),
                SqlPart::Param => {
                    idx += 1;
                    cap += 1 /* '$' */ + decimal_digits(idx);
                }
            }
        }

        let mut out = String::with_capacity(cap);
        idx = 0;
        for part in &self.parts {
            match part {
                SqlPart::Raw(s) => out.push_str(s),
                SqlPart::Param => {
                    idx += 1;
                    out.push('$');
                    use std::fmt::Write;
                    let _ = write!(&mut out, "{idx}");
                }
            }
        }
        out
    }

    /// Parameter refs compatible with `tokio-postgres`.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|p| p.as_ref() as &(dyn ToSql + Sync))
            .collect()
    }

    fn validate(&self) -> OrmResult<()> {
        let placeholder_count = self
            .parts
            .iter()
            .filter(|p| matches!(p, SqlPart::Param))
            .count();

        if placeholder_count != self.params.len() {
            let params_len = self.params.len();
            return Err(OrmError::Validation(format!(
                "Sql: placeholders({placeholder_count}) != params({params_len})"
            )));
        }
        Ok(())
    }

    /// Execute the built SQL and return all rows.
    pub async fn fetch_all(&self, conn: &impl GenericClient) -> OrmResult<Vec<Row>> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_tagged(tag, &sql, &params).await,
            None => conn.query(&sql, &params).await,
        }
    }

    // ==================== Streaming execution ====================

    /// Execute the built SQL and return a row stream.
    pub async fn stream(&self, conn: &impl StreamingClient) -> OrmResult<RowStream> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_stream_tagged(tag, &sql, &params).await,
            None => conn.query_stream(&sql, &params).await,
        }
    }

    /// Execute the built SQL and return a stream of `T`.
    pub async fn stream_as<T: FromRow>(
        &self,
        conn: &impl StreamingClient,
    ) -> OrmResult<FromRowStream<T>> {
        let stream = self.stream(conn).await?;
        Ok(FromRowStream::new(stream))
    }

    /// Execute the built SQL and return all rows mapped to `T`.
    pub async fn fetch_all_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all(conn).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute the built SQL and return the **first** row.
    ///
    /// Semantics match [`GenericClient::query_one`]. If you need strict row-count checking, use
    /// [`Sql::fetch_one_strict`].
    pub async fn fetch_one(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_one_tagged(tag, &sql, &params).await,
            None => conn.query_one(&sql, &params).await,
        }
    }

    /// Execute the built SQL and return the **first** row mapped to `T`.
    pub async fn fetch_one_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one(conn).await?;
        T::from_row(&row)
    }

    /// Execute the built SQL and return the first row, if any.
    pub async fn fetch_opt(&self, conn: &impl GenericClient) -> OrmResult<Option<Row>> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_opt_tagged(tag, &sql, &params).await,
            None => conn.query_opt(&sql, &params).await,
        }
    }

    /// Execute the built SQL and return at most one row mapped to `T`.
    pub async fn fetch_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt(conn).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute the built SQL and return affected row count.
    pub async fn execute(&self, conn: &impl GenericClient) -> OrmResult<u64> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.execute_tagged(tag, &sql, &params).await,
            None => conn.execute(&sql, &params).await,
        }
    }

    /// Append a [`Condition`] to this SQL builder.
    ///
    /// This uses `Sql`'s placeholder generation to keep parameter indices correct.
    pub fn push_condition(&mut self, condition: &Condition) -> &mut Self {
        condition.append_to_sql(self);
        self
    }

    /// Append multiple [`Condition`]s joined by `AND`.
    ///
    /// If `conditions` is empty, this is a no-op.
    pub fn push_conditions_and(&mut self, conditions: &[Condition]) -> &mut Self {
        for (i, cond) in conditions.iter().enumerate() {
            if i > 0 {
                self.push(" AND ");
            }
            self.push_condition(cond);
        }
        self
    }

    /// Append a `WHERE ...` clause composed of [`Condition`]s joined by `AND`.
    ///
    /// If `conditions` is empty, this is a no-op.
    pub fn push_where_and(&mut self, conditions: &[Condition]) -> &mut Self {
        if conditions.is_empty() {
            return self;
        }
        self.push(" WHERE ");
        self.push_conditions_and(conditions)
    }

    /// Execute the built SQL tagged (if the underlying client supports it) and return all rows.
    pub async fn fetch_all_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<Row>> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.query_tagged(tag, &sql, &params).await
    }

    /// Execute the built SQL tagged (if the underlying client supports it) and return all rows mapped to `T`.
    pub async fn fetch_all_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all_tagged(conn, tag).await?;
        rows.iter().map(T::from_row).collect()
    }

    // ==================== Strict execution ====================

    /// Execute the built SQL and require that it returns **exactly one** row.
    pub async fn fetch_one_strict(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        match self.tag.as_deref() {
            Some(tag) => conn.query_one_strict_tagged(tag, &sql, &params).await,
            None => conn.query_one_strict(&sql, &params).await,
        }
    }

    /// Execute the built SQL and require that it returns **exactly one** row mapped to `T`.
    pub async fn fetch_one_strict_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one_strict(conn).await?;
        T::from_row(&row)
    }

    /// Execute the built SQL and require that it returns **exactly one** row, associating a tag.
    pub async fn fetch_one_strict_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Row> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.query_one_strict_tagged(tag, &sql, &params).await
    }

    /// Execute the built SQL and require that it returns **exactly one** row mapped to `T`, associating a tag.
    pub async fn fetch_one_strict_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<T> {
        let row = self.fetch_one_strict_tagged(conn, tag).await?;
        T::from_row(&row)
    }

    /// Execute the built SQL tagged (if the underlying client supports it) and return affected row count.
    pub async fn execute_tagged(&self, conn: &impl GenericClient, tag: &str) -> OrmResult<u64> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.execute_tagged(tag, &sql, &params).await
    }

    // ==================== Convenience APIs (Phase 1) ====================

    /// Execute the built SQL and return exactly one scalar value.
    ///
    /// Expects exactly one row with at least one column. Returns `OrmError::NotFound`
    /// if no rows are returned.
    ///
    /// # Example
    /// ```ignore
    /// let count: i64 = sql("SELECT COUNT(*) FROM users WHERE status = ")
    ///     .push_bind("active")
    ///     .fetch_scalar_one(&client)
    ///     .await?;
    /// ```
    pub async fn fetch_scalar_one<'a, T>(&self, conn: &impl GenericClient) -> OrmResult<T>
    where
        T: for<'b> FromSql<'b> + Send + Sync,
    {
        let row = self.fetch_one(conn).await?;
        row.try_get(0)
            .map_err(|e| OrmError::decode("0", e.to_string()))
    }

    /// Execute the built SQL and return at most one scalar value.
    ///
    /// Returns `None` if no rows are returned, otherwise returns the first column
    /// of the first row.
    ///
    /// # Example
    /// ```ignore
    /// let max_id: Option<i64> = sql("SELECT MAX(id) FROM users")
    ///     .fetch_scalar_opt(&client)
    ///     .await?;
    /// ```
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

    /// Execute the built SQL and return all scalar values from the first column.
    ///
    /// # Example
    /// ```ignore
    /// let ids: Vec<i64> = sql("SELECT id FROM users WHERE status = ")
    ///     .push_bind("active")
    ///     .fetch_scalar_all(&client)
    ///     .await?;
    /// ```
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

    /// Check if any rows exist for this SELECT query.
    ///
    /// Wraps the query in `SELECT EXISTS(...)` for efficient existence checking.
    /// Only works with SELECT statements.
    ///
    /// # Example
    /// ```ignore
    /// let has_active: bool = sql("SELECT 1 FROM users WHERE status = ")
    ///     .push_bind("active")
    ///     .exists(&client)
    ///     .await?;
    /// ```
    pub async fn exists(&self, conn: &impl GenericClient) -> OrmResult<bool> {
        self.validate()?;
        let inner_sql = self.to_sql();
        let inner_sql = inner_sql.trim_end();
        let inner_sql = inner_sql.strip_suffix(';').unwrap_or(inner_sql).trim_end();

        // Validate that this is a SELECT-like statement.
        // Strip leading whitespace, SQL comments (-- and /* */), and parentheses
        // to handle: SELECT, WITH ... SELECT, (SELECT ...), /* comment */ SELECT, etc.
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

    /// Append `LIMIT $n` to the query with a bound parameter.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit(10)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn limit(&mut self, n: i64) -> &mut Self {
        self.push(" LIMIT ").push_bind(n)
    }

    /// Append `OFFSET $n` to the query with a bound parameter.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit(10)
    ///     .offset(20)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn offset(&mut self, n: i64) -> &mut Self {
        self.push(" OFFSET ").push_bind(n)
    }

    /// Append `LIMIT $n OFFSET $m` to the query with bound parameters.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit_offset(10, 20)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn limit_offset(&mut self, limit: i64, offset: i64) -> &mut Self {
        self.push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset)
    }

    /// Append pagination using page number and page size.
    ///
    /// Converts page-based pagination to LIMIT/OFFSET. Page numbers start at 1.
    /// Returns an error if `page < 1`.
    ///
    /// # Example
    /// ```ignore
    /// // Get page 3 with 25 items per page
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .page(3, 25)?
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn page(&mut self, page: i64, per_page: i64) -> OrmResult<&mut Self> {
        if page < 1 {
            return Err(OrmError::Validation(format!(
                "page must be >= 1, got {page}"
            )));
        }
        let offset = (page - 1) * per_page;
        Ok(self.limit_offset(per_page, offset))
    }

    // ==================== Consuming convenience APIs ====================

    /// Bind a parameter and return `self` (consuming version of [`push_bind`]).
    ///
    /// Useful for chaining in contexts where you need ownership, e.g. CTE sub-queries:
    ///
    /// ```ignore
    /// pgorm::sql("SELECT * FROM users WHERE status = ")
    ///     .bind("active")
    /// ```
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.push_bind(value);
        self
    }

    // ==================== Bulk operations ====================

    /// Create a bulk UPDATE builder.
    ///
    /// The initial SQL fragment is used as the table name.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("users")
    ///     .update_many([
    ///         SetExpr::set("status", "inactive")?,
    ///     ])?
    ///     .filter(Condition::lt("last_login", one_year_ago)?)
    ///     .execute(&client)
    ///     .await?;
    /// ```
    pub fn update_many(
        self,
        sets: impl IntoIterator<Item = SetExpr>,
    ) -> OrmResult<UpdateManyBuilder> {
        let table_name = self.to_sql();
        let table = table_name.trim().into_ident()?;
        let sets: Vec<SetExpr> = sets.into_iter().collect();
        if sets.is_empty() {
            return Err(OrmError::Validation(
                "update_many requires at least one SetExpr".to_string(),
            ));
        }
        Ok(UpdateManyBuilder {
            table,
            sets,
            where_clause: None,
            all_rows: false,
        })
    }

    /// Create a bulk DELETE builder.
    ///
    /// The initial SQL fragment is used as the table name.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("sessions")
    ///     .delete_many()?
    ///     .filter(Condition::lt("expires_at", now)?)
    ///     .execute(&client)
    ///     .await?;
    /// ```
    pub fn delete_many(self) -> OrmResult<DeleteManyBuilder> {
        let table_name = self.to_sql();
        let table = table_name.trim().into_ident()?;
        Ok(DeleteManyBuilder {
            table,
            where_clause: None,
            all_rows: false,
        })
    }

    // ==================== CTE (WITH clause) ====================

    /// Start building a CTE (WITH clause) query.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with("active_users", pgorm::sql("SELECT id FROM users WHERE status = ").bind("active"))?
    ///     .select(pgorm::sql("SELECT * FROM active_users"))
    ///     .fetch_all_as::<User>(&client)
    ///     .await?;
    /// ```
    pub fn with(self, name: impl IntoIdent, query: Sql) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new(name, query))
    }

    /// Start building a CTE with explicit column names.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with_columns(
    ///         "monthly_sales",
    ///         ["month", "total"],
    ///         pgorm::sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
    ///     )?
    ///     .select(pgorm::sql("SELECT * FROM monthly_sales"))
    /// ```
    pub fn with_columns(
        self,
        name: impl IntoIdent,
        columns: impl IntoIterator<Item = impl IntoIdent>,
        query: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        let cols: Vec<crate::Ident> = columns
            .into_iter()
            .map(|c| c.into_ident())
            .collect::<OrmResult<Vec<_>>>()?;
        Ok(WithBuilder::new_with_columns(name, cols, query))
    }

    /// Start building a recursive CTE (WITH RECURSIVE).
    ///
    /// Uses UNION ALL by default.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with_recursive(
    ///         "org_tree",
    ///         pgorm::sql("SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL"),
    ///         pgorm::sql("SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
    ///     )?
    ///     .select(pgorm::sql("SELECT * FROM org_tree ORDER BY level"))
    ///     .fetch_all_as::<OrgNode>(&client)
    ///     .await?;
    /// ```
    pub fn with_recursive(
        self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new_recursive(name, base, recursive, true))
    }

    /// Start building a recursive CTE using UNION (with deduplication).
    pub fn with_recursive_union(
        self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new_recursive(name, base, recursive, false))
    }
}
