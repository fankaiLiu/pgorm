//! SQL-first dynamic builder.
//!
//! This module complements `query()`:
//! - `query()` is great when you already have a full SQL string with `$1, $2...`.
//! - `Sql` is great when you want to *compose* SQL dynamically without manually
//!   tracking placeholder indices.
//!
//! # Example
//!
//! ```ignore
//! use pgorm::sql;
//!
//! let mut q = sql("SELECT id, username FROM users WHERE 1=1");
//! if let Some(status) = status {
//!     q.push(" AND status = ").push_bind(status);
//! }
//! q.push(" ORDER BY created_at DESC");
//!
//! let users: Vec<User> = q.fetch_all_as(&conn).await?;
//! ```

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

#[derive(Debug)]
enum SqlPart {
    Raw(String),
    Param,
}

/// A SQL-first, parameter-safe dynamic SQL builder.
///
/// `Sql` stores SQL pieces and parameters separately and generates `$1, $2, ...`
/// placeholders automatically in the final SQL string.
pub struct Sql {
    parts: Vec<SqlPart>,
    params: Vec<Box<dyn ToSql + Sync + Send>>,
}

/// Start building a SQL statement.
pub fn sql(initial_sql: impl Into<String>) -> Sql {
    Sql::new(initial_sql)
}

impl Sql {
    /// Create a new builder with an initial SQL fragment.
    pub fn new(initial_sql: impl Into<String>) -> Self {
        Self {
            parts: vec![SqlPart::Raw(initial_sql.into())],
            params: Vec::new(),
        }
    }

    /// Create an empty builder.
    pub fn empty() -> Self {
        Self {
            parts: Vec::new(),
            params: Vec::new(),
        }
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
        self.params.push(Box::new(value));
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
        self
    }

    /// Append a SQL identifier (schema/table/column) safely.
    ///
    /// This does **not** use parameters (Postgres doesn't allow parameterizing
    /// identifiers). To prevent SQL injection when identifiers are dynamic,
    /// this validates that each `.`-separated segment matches:
    /// `[A-Za-z_][A-Za-z0-9_]*`.
    pub fn push_ident(&mut self, ident: &str) -> OrmResult<&mut Self> {
        if ident.is_empty() {
            return Err(OrmError::Validation("Sql::push_ident: empty identifier".to_string()));
        }

        for seg in ident.split('.') {
            if seg.is_empty() {
                return Err(OrmError::Validation(format!(
                    "Sql::push_ident: invalid identifier '{}'",
                    ident
                )));
            }

            let mut chars = seg.chars();
            let Some(first) = chars.next() else {
                return Err(OrmError::Validation(format!(
                    "Sql::push_ident: invalid identifier '{}'",
                    ident
                )));
            };
            let first_ok = first == '_' || first.is_ascii_alphabetic();
            if !first_ok {
                return Err(OrmError::Validation(format!(
                    "Sql::push_ident: invalid identifier '{}'",
                    ident
                )));
            }

            if !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
                return Err(OrmError::Validation(format!(
                    "Sql::push_ident: invalid identifier '{}'",
                    ident
                )));
            }
        }

        Ok(self.push(ident))
    }

    /// Render SQL with `$1, $2, ...` placeholders.
    pub fn to_sql(&self) -> String {
        let mut out = String::new();
        let mut idx: usize = 0;

        for part in &self.parts {
            match part {
                SqlPart::Raw(s) => out.push_str(s),
                SqlPart::Param => {
                    idx += 1;
                    use std::fmt::Write;
                    let _ = write!(&mut out, "${}", idx);
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
            return Err(OrmError::Validation(format!(
                "Sql internal invariant violated: placeholders({}) != params({})",
                placeholder_count,
                self.params.len()
            )));
        }
        Ok(())
    }

    /// Execute the built SQL and return all rows.
    pub async fn fetch_all(&self, conn: &impl GenericClient) -> OrmResult<Vec<Row>> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.query(&sql, &params).await
    }

    /// Execute the built SQL and return all rows mapped to `T`.
    pub async fn fetch_all_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all(conn).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute the built SQL and return exactly one row.
    pub async fn fetch_one(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.query_one(&sql, &params).await
    }

    /// Execute the built SQL and return exactly one row mapped to `T`.
    pub async fn fetch_one_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one(conn).await?;
        T::from_row(&row)
    }

    /// Execute the built SQL and return at most one row.
    pub async fn fetch_opt(&self, conn: &impl GenericClient) -> OrmResult<Option<Row>> {
        self.validate()?;
        let sql = self.to_sql();
        let params = self.params_ref();
        conn.query_opt(&sql, &params).await
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
        conn.execute(&sql, &params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_placeholders_in_order() {
        let mut q = sql("SELECT * FROM users WHERE a = ");
        q.push_bind(1).push(" AND b = ").push_bind("x");

        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE a = $1 AND b = $2");
        assert_eq!(q.params_ref().len(), 2);
    }

    #[test]
    fn can_compose_fragments() {
        let mut w = Sql::empty();
        w.push(" WHERE id = ").push_bind(42);

        let mut q = sql("SELECT * FROM users");
        q.push_sql(w);

        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id = $1");
        assert_eq!(q.params_ref().len(), 1);
    }

    #[test]
    fn bind_list_renders_commas() {
        let mut q = sql("SELECT * FROM users WHERE id IN (");
        q.push_bind_list(vec![1, 2, 3]).push(")");
        assert_eq!(
            q.to_sql(),
            "SELECT * FROM users WHERE id IN ($1, $2, $3)"
        );
        assert_eq!(q.params_ref().len(), 3);
    }

    #[test]
    fn bind_list_empty_is_valid_sql() {
        let mut q = sql("SELECT * FROM users WHERE id IN (");
        q.push_bind_list(Vec::<i32>::new()).push(")");
        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id IN (NULL)");
        assert_eq!(q.params_ref().len(), 0);
    }

    #[test]
    fn push_ident_accepts_simple_and_dotted() {
        let mut q = Sql::empty();
        q.push_ident("users").unwrap();
        q.push(", ");
        q.push_ident("public.users").unwrap();
        assert_eq!(q.to_sql(), "users, public.users");
    }

    #[test]
    fn push_ident_rejects_unsafe() {
        let mut q = Sql::empty();
        assert!(q.push_ident("users; drop table users; --").is_err());
        assert!(q.push_ident("1users").is_err());
        assert!(q.push_ident("users..name").is_err());
        assert!(q.push_ident("users name").is_err());
    }
}

