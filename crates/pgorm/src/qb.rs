//! Minimal facade for running hand-written SQL.
//!
//! This module is intentionally thin: it primarily exists to provide a familiar
//! `qb::query("...").bind(...).fetch_*` style API.

use crate::Query;

/// Build a SQL query from a pre-numbered SQL string (`$1, $2, ...`).
pub fn query(initial_sql: impl Into<String>) -> Query {
    crate::query(initial_sql)
}

