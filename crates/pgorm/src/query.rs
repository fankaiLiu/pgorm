//! Lightweight query builder for hand-written SQL

use crate::client::GenericClient;
use crate::error::OrmResult;
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// A lightweight query builder for executing hand-written SQL with type-safe parameter binding.
///
/// # Example
///
/// ```ignore
/// use pgorm::query;
///
/// let user: User = query("SELECT * FROM users WHERE id = $1")
///     .bind(&user_id)
///     .fetch_one_as(&conn)
///     .await?;
/// ```
pub struct Query {
    sql: String,
    params: Vec<Box<dyn ToSql + Sync + Send>>,
}

/// Create a new query with the given SQL
pub fn query(sql: impl Into<String>) -> Query {
    Query {
        sql: sql.into(),
        params: Vec::new(),
    }
}

impl Query {
    /// Bind a parameter to the query
    pub fn bind<T: ToSql + Sync + Send + 'static>(mut self, value: T) -> Self {
        self.params.push(Box::new(value));
        self
    }

    /// Execute the query and return all rows
    pub async fn fetch_all(&self, conn: &impl GenericClient) -> OrmResult<Vec<Row>> {
        let params: Vec<&(dyn ToSql + Sync)> =
            self.params.iter().map(|p| p.as_ref() as _).collect();
        conn.query(&self.sql, &params).await
    }

    /// Execute the query and return all rows mapped to type T
    pub async fn fetch_all_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all(conn).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute the query and return exactly one row
    pub async fn fetch_one(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        let params: Vec<&(dyn ToSql + Sync)> =
            self.params.iter().map(|p| p.as_ref() as _).collect();
        conn.query_one(&self.sql, &params).await
    }

    /// Execute the query and return exactly one row mapped to type T
    pub async fn fetch_one_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one(conn).await?;
        T::from_row(&row)
    }

    /// Execute the query and return at most one row
    pub async fn fetch_opt(&self, conn: &impl GenericClient) -> OrmResult<Option<Row>> {
        let params: Vec<&(dyn ToSql + Sync)> =
            self.params.iter().map(|p| p.as_ref() as _).collect();
        conn.query_opt(&self.sql, &params).await
    }

    /// Execute the query and return at most one row mapped to type T
    pub async fn fetch_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt(conn).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute the query and return the number of affected rows
    pub async fn execute(&self, conn: &impl GenericClient) -> OrmResult<u64> {
        let params: Vec<&(dyn ToSql + Sync)> =
            self.params.iter().map(|p| p.as_ref() as _).collect();
        conn.execute(&self.sql, &params).await
    }
}
