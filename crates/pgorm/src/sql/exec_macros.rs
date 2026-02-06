/// Generate all query execution methods for a type that can provide
/// `(sql: String, params: Vec<&(dyn ToSql + Sync)>, tag: Option<&str>)`.
///
/// Usage:
/// ```ignore
/// impl_query_exec! {
///     prepare(self) {
///         self.validate()?;
///         let sql = self.to_sql();
///         let params = self.params_ref();
///         let tag = self.tag.as_deref();
///         (sql, params, tag)
///     }
/// }
/// ```
macro_rules! impl_query_exec {
    (prepare($this:ident) $prepare:block) => {

        /// Execute the built SQL and return all rows.
        pub async fn fetch_all(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Vec<tokio_postgres::Row>> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.query_tagged(tag, &sql, &params).await,
                None => conn.query(&sql, &params).await,
            }
        }

        /// Execute the built SQL and return all rows mapped to `T`.
        pub async fn fetch_all_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Vec<T>> {
            let rows = $this.fetch_all(conn).await?;
            rows.iter().map(T::from_row).collect()
        }

        /// Execute the built SQL and return the **first** row.
        pub async fn fetch_one(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<tokio_postgres::Row> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.query_one_tagged(tag, &sql, &params).await,
                None => conn.query_one(&sql, &params).await,
            }
        }

        /// Execute the built SQL and return the **first** row mapped to `T`.
        pub async fn fetch_one_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<T> {
            let row = $this.fetch_one(conn).await?;
            T::from_row(&row)
        }

        /// Execute the built SQL and return the first row, if any.
        pub async fn fetch_opt(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Option<tokio_postgres::Row>> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.query_opt_tagged(tag, &sql, &params).await,
                None => conn.query_opt(&sql, &params).await,
            }
        }

        /// Execute the built SQL and return at most one row mapped to `T`.
        pub async fn fetch_opt_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Option<T>> {
            let row = $this.fetch_opt(conn).await?;
            row.as_ref().map(T::from_row).transpose()
        }

        /// Execute the built SQL and return affected row count.
        pub async fn execute(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<u64> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.execute_tagged(tag, &sql, &params).await,
                None => conn.execute(&sql, &params).await,
            }
        }

        // ── Streaming ──

        /// Execute the built SQL and return a row stream.
        pub async fn stream(&$this, conn: &impl $crate::client::StreamingClient) -> $crate::error::OrmResult<$crate::client::RowStream> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.query_stream_tagged(tag, &sql, &params).await,
                None => conn.query_stream(&sql, &params).await,
            }
        }

        /// Execute the built SQL and return a stream of `T`.
        pub async fn stream_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::StreamingClient) -> $crate::error::OrmResult<super::stream::FromRowStream<T>> {
            let stream = $this.stream(conn).await?;
            Ok(super::stream::FromRowStream::new(stream))
        }

        // ── Tagged variants ──

        /// Execute and return all rows, associating a tag.
        pub async fn fetch_all_tagged(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<Vec<tokio_postgres::Row>> {
            let (sql, params, _) = $prepare;
            conn.query_tagged(tag, &sql, &params).await
        }

        /// Execute and return all rows mapped to `T`, associating a tag.
        pub async fn fetch_all_tagged_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<Vec<T>> {
            let rows = $this.fetch_all_tagged(conn, tag).await?;
            rows.iter().map(T::from_row).collect()
        }

        /// Execute and return the **first** row, associating a tag.
        pub async fn fetch_one_tagged(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<tokio_postgres::Row> {
            let (sql, params, _) = $prepare;
            conn.query_one_tagged(tag, &sql, &params).await
        }

        /// Execute and return the **first** row mapped to `T`, associating a tag.
        pub async fn fetch_one_tagged_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<T> {
            let row = $this.fetch_one_tagged(conn, tag).await?;
            T::from_row(&row)
        }

        /// Execute and return the first row if any, associating a tag.
        pub async fn fetch_opt_tagged(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<Option<tokio_postgres::Row>> {
            let (sql, params, _) = $prepare;
            conn.query_opt_tagged(tag, &sql, &params).await
        }

        /// Execute and return at most one row mapped to `T`, associating a tag.
        pub async fn fetch_opt_tagged_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<Option<T>> {
            let row = $this.fetch_opt_tagged(conn, tag).await?;
            row.as_ref().map(T::from_row).transpose()
        }

        /// Execute and return affected row count, associating a tag.
        pub async fn execute_tagged(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<u64> {
            let (sql, params, _) = $prepare;
            conn.execute_tagged(tag, &sql, &params).await
        }

        // ── Strict variants ──

        /// Execute and require **exactly one** row.
        pub async fn fetch_one_strict(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<tokio_postgres::Row> {
            let (sql, params, tag) = $prepare;
            match tag {
                Some(tag) => conn.query_one_strict_tagged(tag, &sql, &params).await,
                None => conn.query_one_strict(&sql, &params).await,
            }
        }

        /// Execute and require **exactly one** row mapped to `T`.
        pub async fn fetch_one_strict_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<T> {
            let row = $this.fetch_one_strict(conn).await?;
            T::from_row(&row)
        }

        /// Execute and require **exactly one** row, associating a tag.
        pub async fn fetch_one_strict_tagged(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<tokio_postgres::Row> {
            let (sql, params, _) = $prepare;
            conn.query_one_strict_tagged(tag, &sql, &params).await
        }

        /// Execute and require **exactly one** row mapped to `T`, associating a tag.
        pub async fn fetch_one_strict_tagged_as<T: $crate::row::FromRow>(&$this, conn: &impl $crate::client::GenericClient, tag: &str) -> $crate::error::OrmResult<T> {
            let row = $this.fetch_one_strict_tagged(conn, tag).await?;
            T::from_row(&row)
        }

        // ── Scalar convenience ──

        /// Execute and return exactly one scalar value from column 0.
        pub async fn fetch_scalar_one<'__a, T>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<T>
        where
            T: for<'__b> tokio_postgres::types::FromSql<'__b> + Send + Sync,
        {
            let row = $this.fetch_one(conn).await?;
            row.try_get(0)
                .map_err(|e| $crate::error::OrmError::decode("0", e.to_string()))
        }

        /// Execute and return at most one scalar value from column 0.
        pub async fn fetch_scalar_opt<'__a, T>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Option<T>>
        where
            T: for<'__b> tokio_postgres::types::FromSql<'__b> + Send + Sync,
        {
            let row = $this.fetch_opt(conn).await?;
            match row {
                Some(r) => r.try_get(0).map(Some)
                    .map_err(|e| $crate::error::OrmError::decode("0", e.to_string())),
                None => Ok(None),
            }
        }

        /// Execute and return all scalar values from column 0.
        pub async fn fetch_scalar_all<'__a, T>(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<Vec<T>>
        where
            T: for<'__b> tokio_postgres::types::FromSql<'__b> + Send + Sync,
        {
            let rows = $this.fetch_all(conn).await?;
            rows.iter()
                .map(|r| r.try_get(0)
                    .map_err(|e| $crate::error::OrmError::decode("0", e.to_string())))
                .collect()
        }

        /// Check if any rows exist by wrapping the query in `SELECT EXISTS(...)`.
        pub async fn exists(&$this, conn: &impl $crate::client::GenericClient) -> $crate::error::OrmResult<bool> {
            let (sql, params, tag) = $prepare;
            let inner_sql = sql.trim_end();
            let inner_sql = inner_sql.strip_suffix(';').unwrap_or(inner_sql).trim_end();

            let trimmed = super::strip_sql_prefix(inner_sql);
            if !super::starts_with_keyword(trimmed, "SELECT")
                && !super::starts_with_keyword(trimmed, "WITH")
            {
                return Err($crate::error::OrmError::Validation(
                    "exists() only works with SELECT statements (including WITH ... SELECT)".to_string(),
                ));
            }

            let wrapped_sql = format!("SELECT EXISTS({inner_sql})");
            let row = match tag {
                Some(tag) => conn.query_one_tagged(tag, &wrapped_sql, &params).await?,
                None => conn.query_one(&wrapped_sql, &params).await?,
            };
            row.try_get(0)
                .map_err(|e| $crate::error::OrmError::decode("0", e.to_string()))
        }
    };
}
