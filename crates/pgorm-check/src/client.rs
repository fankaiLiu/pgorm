//! Database client trait for pgorm-check
//!
//! This module defines a minimal trait for database operations,
//! allowing pgorm-check to work independently of pgorm.

use crate::error::{CheckError, CheckResult};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

/// A trait for types that can execute PostgreSQL queries.
///
/// This is implemented for `tokio_postgres::Client`, `tokio_postgres::Transaction`,
/// and connection pool clients.
#[async_trait::async_trait]
pub trait CheckClient: Sync {
    /// Execute a query and return all rows.
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Vec<Row>>;

    /// Execute a query and return exactly one row.
    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Row>;

    /// Execute a statement and return the number of affected rows.
    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<u64>;
}

#[async_trait::async_trait]
impl CheckClient for tokio_postgres::Client {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Vec<Row>> {
        self.query(sql, params).await.map_err(CheckError::from)
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Row> {
        self.query_one(sql, params).await.map_err(CheckError::from)
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<u64> {
        self.execute(sql, params).await.map_err(CheckError::from)
    }
}

#[async_trait::async_trait]
impl<'a> CheckClient for tokio_postgres::Transaction<'a> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Vec<Row>> {
        self.query(sql, params).await.map_err(CheckError::from)
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<Row> {
        self.query_one(sql, params).await.map_err(CheckError::from)
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> CheckResult<u64> {
        self.execute(sql, params).await.map_err(CheckError::from)
    }
}

/// Extension trait for accessing row columns with better error handling.
pub trait RowExt {
    /// Get a column value by name, returning a CheckError on failure.
    fn try_get_column<'a, T>(&'a self, column: &str) -> CheckResult<T>
    where
        T: tokio_postgres::types::FromSql<'a>;
}

impl RowExt for Row {
    fn try_get_column<'a, T>(&'a self, column: &str) -> CheckResult<T>
    where
        T: tokio_postgres::types::FromSql<'a>,
    {
        self.try_get(column)
            .map_err(|e| CheckError::decode(column, e.to_string()))
    }
}
