//! Trait definitions for query builders.

use crate::client::GenericClient;
use crate::error::OrmResult;
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Base trait for all query builders.
///
/// Provides methods for building SQL and executing queries.
pub trait SqlQb: Sync {
    /// Build the SQL string.
    fn build_sql(&self) -> String;

    /// Get parameters as references compatible with tokio-postgres.
    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)>;

    /// Debug helper to get the SQL string.
    fn to_sql(&self) -> String {
        self.build_sql()
    }

    /// Validate builder state before execution.
    fn validate(&self) -> OrmResult<()> {
        Ok(())
    }

    /// Get any build errors.
    fn build_error(&self) -> Option<&str> {
        None
    }

    /// Execute query and return all rows.
    fn query(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        async move {
            self.validate()?;
            let sql = self.build_sql();
            let params = self.params_ref();
            conn.query(&sql, &params).await
        }
    }

    /// Execute query and return at most one row.
    fn query_opt(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        async move {
            self.validate()?;
            let sql = self.build_sql();
            let params = self.params_ref();
            conn.query_opt(&sql, &params).await
        }
    }

    /// Execute query and return exactly one row.
    fn query_one(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            self.validate()?;
            let sql = self.build_sql();
            let params = self.params_ref();
            conn.query_one(&sql, &params).await
        }
    }

    /// Execute query and map all rows to `T`.
    fn fetch_all<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<T>>> + Send {
        async move {
            let rows = self.query(conn).await?;
            rows.iter().map(T::from_row).collect()
        }
    }

    /// Execute query and map at most one row to `T`.
    fn fetch_opt<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<T>>> + Send {
        async move {
            let row = self.query_opt(conn).await?;
            row.as_ref().map(T::from_row).transpose()
        }
    }

    /// Execute query and map exactly one row to `T`.
    fn fetch_one<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<T>> + Send {
        async move {
            let row = self.query_one(conn).await?;
            T::from_row(&row)
        }
    }

    // Aliases for backward compatibility

    /// Alias for `fetch_all`.
    fn query_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<T>>> + Send {
        self.fetch_all(conn)
    }

    /// Alias for `fetch_opt`.
    fn query_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<T>>> + Send {
        self.fetch_opt(conn)
    }

    /// Alias for `fetch_one`.
    fn query_one_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<T>> + Send {
        self.fetch_one(conn)
    }
}

/// Trait for mutation builders (INSERT/UPDATE/DELETE).
pub trait MutationQb: SqlQb {
    /// Execute and return affected row count.
    fn execute(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<u64>> + Send {
        async move {
            self.validate()?;
            let sql = self.build_sql();
            let params = self.params_ref();
            conn.execute(&sql, &params).await
        }
    }
}

/// The result of building a query.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BuiltQuery {
    pub sql: String,
    pub params: Vec<crate::qb::Param>,
}

#[allow(dead_code)]
impl BuiltQuery {
    /// Create a new built query.
    pub fn new(sql: String, params: Vec<crate::qb::Param>) -> Self {
        Self { sql, params }
    }

    /// Get parameters as references for tokio-postgres.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|p| p.as_ref()).collect()
    }
}
