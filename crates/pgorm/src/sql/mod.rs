//! Dynamic SQL builder.
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

mod builder;
mod parts;
mod query;
mod stream;

#[cfg(test)]
mod tests;

pub use builder::Sql;
pub use query::Query;
pub use stream::FromRowStream;

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
pub(crate) fn strip_sql_prefix(sql: &str) -> &str {
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

pub(crate) fn starts_with_keyword(s: &str, keyword: &str) -> bool {
    match s.get(0..keyword.len()) {
        Some(prefix) => prefix.eq_ignore_ascii_case(keyword),
        None => false,
    }
}
