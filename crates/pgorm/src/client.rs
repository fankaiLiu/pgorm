//! Generic client trait for unified database access

use crate::error::{OrmError, OrmResult};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// A trait that unifies `Client` and `Transaction` for database operations.
///
/// This allows repository methods to accept either a direct client connection
/// or a transaction, making it easy to compose operations within transactions.
pub trait GenericClient: Send + Sync {
    /// Execute a query and return all rows
    fn query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send;

    /// Execute a query and return exactly one row
    ///
    /// Returns `OrmError::NotFound` if no rows are returned
    fn query_one(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send;

    /// Execute a query and return at most one row
    fn query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send;

    /// Execute a statement and return the number of affected rows
    fn execute(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send;
}

impl GenericClient for tokio_postgres::Client {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        tokio_postgres::Client::query(self, sql, params)
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

/// Wrapper for deadpool_postgres::Client to implement GenericClient
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
        self.0
            .query(sql, params)
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
        self.0
            .execute(sql, params)
            .await
            .map_err(OrmError::from_db_error)
    }
}
