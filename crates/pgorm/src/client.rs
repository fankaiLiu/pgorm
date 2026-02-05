//! Generic client trait for unified database access.

use crate::error::{OrmError, OrmResult};
use futures_core::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_postgres::Row;
use tokio_postgres::Statement;
use tokio_postgres::types::ToSql;

/// A trait that unifies database clients and transactions.
///
/// This allows repository methods to accept either a direct client connection
/// or a transaction, making it easy to compose operations within transactions.
pub trait GenericClient: Send + Sync {
    /// Execute a query and return all rows.
    fn query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send;

    /// Execute a query and return all rows, associating a tag for monitoring/observability.
    ///
    /// The default implementation ignores `tag` and calls [`GenericClient::query`].
    fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        let _ = tag;
        self.query(sql, params)
    }

    /// Execute a query and return the **first** row.
    ///
    /// Semantics:
    /// - 0 rows: returns [`OrmError::NotFound`]
    /// - 1 row: returns that row
    /// - multiple rows: returns the first row (does **not** error)
    ///
    /// If you need strict row-count checking (i.e. error on multiple rows), use
    /// [`GenericClient::query_one_strict`].
    ///
    /// Returns `OrmError::NotFound` if no rows are returned.
    fn query_one(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send;

    /// Execute a query and return the **first** row, associating a tag for monitoring/observability.
    ///
    /// Semantics match [`GenericClient::query_one`]. The default implementation ignores `tag` and
    /// calls [`GenericClient::query_one`].
    fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        let _ = tag;
        self.query_one(sql, params)
    }

    /// Execute a query and require that it returns **exactly one** row.
    ///
    /// Semantics:
    /// - 0 rows: returns [`OrmError::NotFound`]
    /// - 1 row: returns that row
    /// - multiple rows: returns [`OrmError::TooManyRows`]
    fn query_one_strict(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            let rows = self.query(sql, params).await?;
            match rows.len() {
                0 => Err(OrmError::not_found("Expected 1 row, got 0")),
                1 => Ok(rows.into_iter().next().expect("len == 1")),
                got => Err(OrmError::too_many_rows(1, got)),
            }
        }
    }

    /// Execute a query and require that it returns **exactly one** row, associating a tag.
    ///
    /// The default implementation uses [`GenericClient::query_tagged`] and applies the same
    /// row-count semantics as [`GenericClient::query_one_strict`].
    fn query_one_strict_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            let rows = self.query_tagged(tag, sql, params).await?;
            match rows.len() {
                0 => Err(OrmError::not_found("Expected 1 row, got 0")),
                1 => Ok(rows.into_iter().next().expect("len == 1")),
                got => Err(OrmError::too_many_rows(1, got)),
            }
        }
    }

    /// Execute a query and return the first row, if any.
    ///
    /// Semantics:
    /// - 0 rows: returns `Ok(None)`
    /// - 1 row: returns `Ok(Some(row))`
    /// - multiple rows: returns `Ok(Some(first_row))` (does **not** error)
    fn query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send;

    /// Execute a query and return the first row, if any, associating a tag for monitoring/observability.
    ///
    /// Semantics match [`GenericClient::query_opt`]. The default implementation ignores `tag` and
    /// calls [`GenericClient::query_opt`].
    fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        let _ = tag;
        self.query_opt(sql, params)
    }

    /// Execute a statement and return the number of affected rows.
    fn execute(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send;

    /// Execute a statement and return the number of affected rows, associating a tag for monitoring/observability.
    ///
    /// The default implementation ignores `tag` and calls [`GenericClient::execute`].
    fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        let _ = tag;
        self.execute(sql, params)
    }

    /// Return a cancellation token for the underlying connection, if supported.
    ///
    /// This enables best-effort server-side query cancellation in higher-level wrappers when a timeout triggers.
    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        None
    }

    /// Whether this client supports prepared statement APIs.
    ///
    /// The default implementation returns `false`, and prepared APIs will error if called.
    fn supports_prepared_statements(&self) -> bool {
        false
    }

    /// Prepare a statement on this connection.
    ///
    /// Prepared statements are **per-connection** and must not be used across connections.
    fn prepare_statement(
        &self,
        sql: &str,
    ) -> impl std::future::Future<Output = OrmResult<Statement>> + Send {
        let _ = sql;
        async {
            Err(OrmError::Other(
                "prepared statements are not supported by this client".to_string(),
            ))
        }
    }

    /// Execute a prepared statement and return all rows.
    fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        let _ = stmt;
        let _ = params;
        async {
            Err(OrmError::Other(
                "prepared statements are not supported by this client".to_string(),
            ))
        }
    }

    /// Execute a prepared statement and return the **first** row.
    ///
    /// Semantics match [`GenericClient::query_one`].
    fn query_one_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            let rows = self.query_prepared(stmt, params).await?;
            rows.into_iter()
                .next()
                .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
        }
    }

    /// Execute a prepared statement and return the first row, if any.
    ///
    /// Semantics match [`GenericClient::query_opt`].
    fn query_opt_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        async move {
            let rows = self.query_prepared(stmt, params).await?;
            Ok(rows.into_iter().next())
        }
    }

    /// Execute a prepared statement and return affected row count.
    fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        let _ = stmt;
        let _ = params;
        async {
            Err(OrmError::Other(
                "prepared statements are not supported by this client".to_string(),
            ))
        }
    }
}

impl GenericClient for tokio_postgres::Client {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        tokio_postgres::Client::query(self, sql, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        Some(tokio_postgres::Client::cancel_token(self))
    }

    fn supports_prepared_statements(&self) -> bool {
        true
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        tokio_postgres::Client::prepare(self, sql)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        tokio_postgres::Client::query(self, stmt, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        tokio_postgres::Client::execute(self, stmt, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let rows = GenericClient::query(self, sql, params).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let rows = GenericClient::query(self, sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        tokio_postgres::Client::execute(self, sql, params)
            .await
            .map_err(OrmError::from_db_error)
    }
}

impl GenericClient for tokio_postgres::Transaction<'_> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        tokio_postgres::Transaction::query(self, sql, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        Some(tokio_postgres::Transaction::cancel_token(self))
    }

    fn supports_prepared_statements(&self) -> bool {
        true
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        tokio_postgres::Transaction::prepare(self, sql)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        tokio_postgres::Transaction::query(self, stmt, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        tokio_postgres::Transaction::execute(self, stmt, params)
            .await
            .map_err(OrmError::from_db_error)
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let rows = GenericClient::query(self, sql, params).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let rows = GenericClient::query(self, sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        tokio_postgres::Transaction::execute(self, sql, params)
            .await
            .map_err(OrmError::from_db_error)
    }
}

/// A stream of database rows.
///
/// This is a type-erased wrapper around a `Stream<Item = OrmResult<Row>>` so that different
/// client implementations can return a uniform streaming type.
#[must_use]
pub struct RowStream {
    inner: Pin<Box<dyn Stream<Item = OrmResult<Row>> + Send>>,
}

impl RowStream {
    /// Create a new `RowStream` from any compatible stream.
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = OrmResult<Row>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
        }
    }
}

impl Stream for RowStream {
    type Item = OrmResult<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// Streaming query support (RowStream).
///
/// This trait is intentionally separate from [`GenericClient`] so that only clients that can
/// efficiently stream rows (e.g. via `tokio-postgres`'s `query_raw`) need to implement it.
pub trait StreamingClient: GenericClient {
    /// Execute a query and return a `RowStream` for incremental consumption.
    fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<RowStream>> + Send;

    /// Execute a query and return a `RowStream`, associating a tag for monitoring/observability.
    ///
    /// The default implementation ignores `tag` and calls [`StreamingClient::query_stream`].
    fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<RowStream>> + Send {
        let _ = tag;
        self.query_stream(sql, params)
    }
}

struct MapDbRowStream<S> {
    inner: Pin<Box<S>>,
}

impl<S> MapDbRowStream<S> {
    fn new(stream: S) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }
}

impl<S> Stream for MapDbRowStream<S>
where
    S: Stream<Item = Result<Row, tokio_postgres::Error>> + Send + 'static,
{
    type Item = OrmResult<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(row))) => Poll::Ready(Some(Ok(row))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(OrmError::from_db_error(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl StreamingClient for tokio_postgres::Client {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        let stream = tokio_postgres::Client::query_raw(self, sql, params.iter().copied())
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(RowStream::new(MapDbRowStream::new(stream)))
    }
}

impl StreamingClient for tokio_postgres::Transaction<'_> {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        let stream = tokio_postgres::Transaction::query_raw(self, sql, params.iter().copied())
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(RowStream::new(MapDbRowStream::new(stream)))
    }
}

// ===== deadpool-postgres support =====

#[cfg(feature = "pool")]
impl GenericClient for deadpool_postgres::Client {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        // Delegate to the deref target (ClientWrapper / tokio_postgres::Client).
        GenericClient::query(&**self, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        GenericClient::cancel_token(&**self)
    }

    fn supports_prepared_statements(&self) -> bool {
        GenericClient::supports_prepared_statements(&**self)
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        GenericClient::prepare_statement(&**self, sql).await
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        GenericClient::query_prepared(&**self, stmt, params).await
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        GenericClient::execute_prepared(&**self, stmt, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let rows = GenericClient::query(self, sql, params).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let rows = GenericClient::query(self, sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        GenericClient::execute(&**self, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl GenericClient for deadpool_postgres::ClientWrapper {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        GenericClient::query(&**self, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        GenericClient::cancel_token(&**self)
    }

    fn supports_prepared_statements(&self) -> bool {
        GenericClient::supports_prepared_statements(&**self)
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        GenericClient::prepare_statement(&**self, sql).await
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        GenericClient::query_prepared(&**self, stmt, params).await
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        GenericClient::execute_prepared(&**self, stmt, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let rows = GenericClient::query(self, sql, params).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let rows = GenericClient::query(self, sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        GenericClient::execute(&**self, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl GenericClient for deadpool_postgres::Transaction<'_> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        GenericClient::query(&**self, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        GenericClient::cancel_token(&**self)
    }

    fn supports_prepared_statements(&self) -> bool {
        GenericClient::supports_prepared_statements(&**self)
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        GenericClient::prepare_statement(&**self, sql).await
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        GenericClient::query_prepared(&**self, stmt, params).await
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        GenericClient::execute_prepared(&**self, stmt, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let rows = GenericClient::query(self, sql, params).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| OrmError::not_found("Expected one row, got none"))
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        let rows = GenericClient::query(self, sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        GenericClient::execute(&**self, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl StreamingClient for deadpool_postgres::Client {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        StreamingClient::query_stream(&**self, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl StreamingClient for deadpool_postgres::ClientWrapper {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        StreamingClient::query_stream(&**self, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl StreamingClient for deadpool_postgres::Transaction<'_> {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        StreamingClient::query_stream(&**self, sql, params).await
    }
}

/// Wrapper for `deadpool_postgres::Client`.
///
/// You can use this if you want to make pooled clients explicit in your API,
/// but `deadpool_postgres::Client` itself also implements `GenericClient`.
#[cfg(feature = "pool")]
pub struct PoolClient(deadpool_postgres::Client);

#[cfg(feature = "pool")]
impl PoolClient {
    pub fn new(client: deadpool_postgres::Client) -> Self {
        Self(client)
    }

    pub fn inner(&self) -> &deadpool_postgres::Client {
        &self.0
    }

    pub fn into_inner(self) -> deadpool_postgres::Client {
        self.0
    }
}

#[cfg(feature = "pool")]
impl std::ops::Deref for PoolClient {
    type Target = deadpool_postgres::Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "pool")]
impl GenericClient for PoolClient {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        GenericClient::query(&self.0, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.0.cancel_token()
    }

    fn supports_prepared_statements(&self) -> bool {
        GenericClient::supports_prepared_statements(&self.0)
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        GenericClient::prepare_statement(&self.0, sql).await
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        GenericClient::query_prepared(&self.0, stmt, params).await
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        GenericClient::execute_prepared(&self.0, stmt, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        GenericClient::query_one(&self.0, sql, params).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        GenericClient::query_opt(&self.0, sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        GenericClient::execute(&self.0, sql, params).await
    }
}

#[cfg(feature = "pool")]
impl StreamingClient for PoolClient {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        StreamingClient::query_stream(&self.0, sql, params).await
    }
}

// ===== Reference implementations =====
// These allow InstrumentedClient to wrap &Client instead of owned Client

impl<C: GenericClient> GenericClient for &C {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        (*self).query(sql, params).await
    }

    fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        (*self).query_tagged(tag, sql, params)
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        (*self).query_one(sql, params).await
    }

    fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        (*self).query_one_tagged(tag, sql, params)
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        (*self).query_opt(sql, params).await
    }

    fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        (*self).query_opt_tagged(tag, sql, params)
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        (*self).execute(sql, params).await
    }

    fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        (*self).execute_tagged(tag, sql, params)
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        (*self).cancel_token()
    }

    fn supports_prepared_statements(&self) -> bool {
        (*self).supports_prepared_statements()
    }

    fn prepare_statement(
        &self,
        sql: &str,
    ) -> impl std::future::Future<Output = OrmResult<Statement>> + Send {
        (*self).prepare_statement(sql)
    }

    fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        (*self).query_prepared(stmt, params)
    }

    fn query_one_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        (*self).query_one_prepared(stmt, params)
    }

    fn query_opt_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        (*self).query_opt_prepared(stmt, params)
    }

    fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        (*self).execute_prepared(stmt, params)
    }
}

impl<C: StreamingClient> StreamingClient for &C {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        (*self).query_stream(sql, params).await
    }

    fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<RowStream>> + Send {
        (*self).query_stream_tagged(tag, sql, params)
    }
}
