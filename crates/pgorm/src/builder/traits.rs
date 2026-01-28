use crate::client::GenericClient;
use crate::error::OrmResult;
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Base trait for SQL builders.
pub trait SqlBuilder: Sync {
    /// Build the SQL string.
    fn build_sql(&self) -> String;

    /// Get parameters as references compatible with tokio-postgres.
    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)>;

    /// Debug helper.
    fn to_sql(&self) -> String {
        self.build_sql()
    }

    /// Validate builder state.
    fn validate(&self) -> OrmResult<()> {
        Ok(())
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
    fn query_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<T>>> + Send {
        async move {
            let rows = self.query(conn).await?;
            rows.iter().map(T::from_row).collect()
        }
    }

    /// Execute query and map at most one row to `T`.
    fn query_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<T>>> + Send {
        async move {
            let row = self.query_opt(conn).await?;
            row.as_ref().map(T::from_row).transpose()
        }
    }

    /// Execute query and map exactly one row to `T`.
    fn query_one_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<T>> + Send {
        async move {
            let row = self.query_one(conn).await?;
            T::from_row(&row)
        }
    }
}

/// Trait for mutation builders (INSERT/UPDATE/DELETE).
pub trait MutationBuilder: SqlBuilder {
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
