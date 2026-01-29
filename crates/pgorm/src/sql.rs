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
use crate::condition::Condition;
use crate::error::{OrmError, OrmResult};
use crate::ident::IntoIdent;
use crate::row::FromRow;
use std::sync::Arc;
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, ToSql};

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
    params: Vec<Arc<dyn ToSql + Sync + Send>>,
}

/// A SQL string with pre-numbered placeholders (`$1, $2, ...`) plus bound parameters.
///
/// Use this when you already have a complete SQL string and just want to bind values.
pub struct Query {
    sql: String,
    params: Vec<Arc<dyn ToSql + Sync + Send>>,
}

/// Build a SQL query from a pre-numbered SQL string (`$1, $2, ...`).
pub fn query(initial_sql: impl Into<String>) -> Query {
    Query::new(initial_sql)
}

/// Start building a SQL statement.
pub fn sql(initial_sql: impl Into<String>) -> Sql {
    Sql::new(initial_sql)
}

/// Strip leading whitespace, SQL comments (`--` and `/* */`), and parentheses
/// from a SQL string to find the first meaningful keyword.
fn strip_sql_prefix(sql: &str) -> &str {
    let mut s = sql;
    loop {
        let before = s;
        // Trim whitespace
        s = s.trim_start();
        // Skip line comments
        if s.starts_with("--") {
            if let Some(pos) = s.find('\n') {
                s = &s[pos + 1..];
                continue;
            }
            return ""; // comment is the whole remaining string
        }
        // Skip block comments
        if s.starts_with("/*") {
            if let Some(pos) = s.find("*/") {
                s = &s[pos + 2..];
                continue;
            }
            return ""; // unclosed block comment
        }
        // Skip leading parentheses
        if s.starts_with('(') {
            s = &s[1..];
            continue;
        }
        if s == before {
            break;
        }
    }
    s
}

fn starts_with_keyword(s: &str, keyword: &str) -> bool {
    match s.get(0..keyword.len()) {
        Some(prefix) => prefix.eq_ignore_ascii_case(keyword),
        None => false,
    }
}

impl Query {
    /// Create a new pre-numbered query.
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }

    /// Bind a parameter value.
    ///
    /// This does not modify the SQL string; it only appends the value to the
    /// parameter list. The SQL string must already contain `$1, $2, ...`.
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.params.push(Arc::new(value));
        self
    }

    /// Access the SQL string.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Parameter refs compatible with `tokio-postgres`.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|p| p.as_ref() as &(dyn ToSql + Sync))
            .collect()
    }

    // ==================== Execution ====================

    /// Execute the query and return all rows.
    pub async fn fetch_all(&self, conn: &impl GenericClient) -> OrmResult<Vec<Row>> {
        let params = self.params_ref();
        conn.query(&self.sql, &params).await
    }

    /// Execute the query and return all rows mapped to `T`.
    pub async fn fetch_all_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all(conn).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute the query and return exactly one row.
    pub async fn fetch_one(&self, conn: &impl GenericClient) -> OrmResult<Row> {
        let params = self.params_ref();
        conn.query_one(&self.sql, &params).await
    }

    /// Execute the query and return exactly one row mapped to `T`.
    pub async fn fetch_one_as<T: FromRow>(&self, conn: &impl GenericClient) -> OrmResult<T> {
        let row = self.fetch_one(conn).await?;
        T::from_row(&row)
    }

    /// Execute the query and return at most one row.
    pub async fn fetch_opt(&self, conn: &impl GenericClient) -> OrmResult<Option<Row>> {
        let params = self.params_ref();
        conn.query_opt(&self.sql, &params).await
    }

    /// Execute the query and return at most one row mapped to `T`.
    pub async fn fetch_opt_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt(conn).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute the query and return affected row count.
    pub async fn execute(&self, conn: &impl GenericClient) -> OrmResult<u64> {
        let params = self.params_ref();
        conn.execute(&self.sql, &params).await
    }

    // ==================== Tagged execution ====================

    pub async fn fetch_all_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<Row>> {
        let params = self.params_ref();
        conn.query_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_all_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Vec<T>> {
        let rows = self.fetch_all_tagged(conn, tag).await?;
        rows.iter().map(T::from_row).collect()
    }

    pub async fn fetch_one_tagged(&self, conn: &impl GenericClient, tag: &str) -> OrmResult<Row> {
        let params = self.params_ref();
        conn.query_one_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_one_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<T> {
        let row = self.fetch_one_tagged(conn, tag).await?;
        T::from_row(&row)
    }

    pub async fn fetch_opt_tagged(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Option<Row>> {
        let params = self.params_ref();
        conn.query_opt_tagged(tag, &self.sql, &params).await
    }

    pub async fn fetch_opt_tagged_as<T: FromRow>(
        &self,
        conn: &impl GenericClient,
        tag: &str,
    ) -> OrmResult<Option<T>> {
        let row = self.fetch_opt_tagged(conn, tag).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    pub async fn execute_tagged(&self, conn: &impl GenericClient, tag: &str) -> OrmResult<u64> {
        let params = self.params_ref();
        conn.execute_tagged(tag, &self.sql, &params).await
    }

    // ==================== Convenience APIs ====================

    pub async fn fetch_scalar_one<'a, T>(&self, conn: &impl GenericClient) -> OrmResult<T>
    where
        T: for<'b> FromSql<'b> + Send + Sync,
    {
        let row = self.fetch_one(conn).await?;
        row.try_get(0)
            .map_err(|e| OrmError::decode("0", e.to_string()))
    }

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

    pub async fn exists(&self, conn: &impl GenericClient) -> OrmResult<bool> {
        let inner_sql = self.sql.trim_end();
        let inner_sql = inner_sql.strip_suffix(';').unwrap_or(inner_sql).trim_end();

        let trimmed = strip_sql_prefix(inner_sql);
        if !starts_with_keyword(trimmed, "SELECT") && !starts_with_keyword(trimmed, "WITH") {
            return Err(OrmError::Validation(
                "exists() only works with SELECT statements (including WITH ... SELECT)"
                    .to_string(),
            ));
        }

        let wrapped_sql = format!("SELECT EXISTS({})", inner_sql);
        let params = self.params_ref();
        let row = conn.query_one(&wrapped_sql, &params).await?;
        row.try_get(0)
            .map_err(|e| OrmError::decode("0", e.to_string()))
    }
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
        Ok(self.push(&ident.to_sql()))
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
                "Sql: placeholders({}) != params({})",
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

        let wrapped_sql = format!("SELECT EXISTS({})", inner_sql);
        let params = self.params_ref();
        let row = conn.query_one(&wrapped_sql, &params).await?;
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
                "page must be >= 1, got {}",
                page
            )));
        }
        let offset = (page - 1) * per_page;
        Ok(self.limit_offset(per_page, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::Condition;

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
        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id IN ($1, $2, $3)");
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

    #[test]
    fn can_append_condition_as_placeholders() {
        let mut q = sql("SELECT * FROM users WHERE ");
        q.push_condition(&Condition::eq("id", 42_i64).unwrap());

        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id = $1");
        assert_eq!(q.params_ref().len(), 1);
    }

    #[test]
    fn condition_placeholders_compose_with_push_bind() {
        let mut q = sql("SELECT * FROM users WHERE a = ");
        q.push_bind(1_i64);
        q.push(" AND ");
        q.push_condition(&Condition::eq("b", "x").unwrap());

        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE a = $1 AND b = $2");
        assert_eq!(q.params_ref().len(), 2);
    }

    #[test]
    fn empty_in_list_condition_is_valid_sql() {
        let mut q = sql("SELECT * FROM users WHERE ");
        q.push_condition(&Condition::in_list("id", Vec::<i32>::new()).unwrap());

        assert_eq!(q.to_sql(), "SELECT * FROM users WHERE 1=0");
        assert_eq!(q.params_ref().len(), 0);
    }

    // ==================== Phase 1: Convenience API tests ====================

    #[test]
    fn limit_appends_with_param() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        q.limit(10);
        assert_eq!(q.to_sql(), "SELECT * FROM users ORDER BY id LIMIT $1");
        assert_eq!(q.params_ref().len(), 1);
    }

    #[test]
    fn offset_appends_with_param() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        q.offset(20);
        assert_eq!(q.to_sql(), "SELECT * FROM users ORDER BY id OFFSET $1");
        assert_eq!(q.params_ref().len(), 1);
    }

    #[test]
    fn limit_offset_appends_both_params() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        q.limit_offset(10, 20);
        assert_eq!(
            q.to_sql(),
            "SELECT * FROM users ORDER BY id LIMIT $1 OFFSET $2"
        );
        assert_eq!(q.params_ref().len(), 2);
    }

    #[test]
    fn page_converts_to_limit_offset() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        q.page(3, 25).unwrap();
        // page 3 with 25 per page = OFFSET 50
        assert_eq!(
            q.to_sql(),
            "SELECT * FROM users ORDER BY id LIMIT $1 OFFSET $2"
        );
        assert_eq!(q.params_ref().len(), 2);
    }

    #[test]
    fn page_rejects_zero() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        assert!(q.page(0, 25).is_err());
    }

    #[test]
    fn page_rejects_negative() {
        let mut q = sql("SELECT * FROM users ORDER BY id");
        assert!(q.page(-1, 25).is_err());
    }

    #[test]
    fn pagination_composes_with_conditions() {
        let mut q = sql("SELECT * FROM users WHERE ");
        q.push_condition(&Condition::eq("status", "active").unwrap());
        q.push(" ORDER BY id");
        q.limit_offset(10, 0);
        assert_eq!(
            q.to_sql(),
            "SELECT * FROM users WHERE status = $1 ORDER BY id LIMIT $2 OFFSET $3"
        );
        assert_eq!(q.params_ref().len(), 3);
    }
}
