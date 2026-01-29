//! Generic client trait for unified database access.

use crate::error::{OrmError, OrmResult};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

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

    /// Execute a query and return exactly one row.
    ///
    /// Returns `OrmError::NotFound` if no rows are returned.
    fn query_one(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send;

    /// Execute a query and return exactly one row, associating a tag for monitoring/observability.
    ///
    /// The default implementation ignores `tag` and calls [`GenericClient::query_one`].
    fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        let _ = tag;
        self.query_one(sql, params)
    }

    /// Execute a query and return at most one row.
    fn query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send;

    /// Execute a query and return at most one row, associating a tag for monitoring/observability.
    ///
    /// The default implementation ignores `tag` and calls [`GenericClient::query_opt`].
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
}
