//! SQL linting utilities for common safety and correctness checks.
//!
//! This module provides functions to validate SQL queries for common issues:
//! - SELECT queries missing LIMIT (potential memory issues)
//! - DELETE/UPDATE queries missing WHERE (dangerous operations)
//! - SQL syntax validation
//! - Statement type detection

use serde::{Deserialize, Serialize};

use crate::sql_analysis::{ColumnRefFull, analyze_sql};

// ── Lint codes ──────────────────────────────────────────────────────
// Centralised constants to avoid magic strings scattered across the codebase.

/// SQL syntax error.
pub const LINT_E001: &str = "E001";
/// DELETE without WHERE clause.
pub const LINT_E002: &str = "E002";
/// UPDATE without WHERE clause.
pub const LINT_E003: &str = "E003";
/// Expected a SELECT statement.
pub const LINT_E004: &str = "E004";
/// SELECT * usage (informational).
pub const LINT_I001: &str = "I001";
/// TRUNCATE warning.
pub const LINT_W001: &str = "W001";
/// DROP TABLE warning.
pub const LINT_W002: &str = "W002";
/// SELECT without LIMIT.
pub const LINT_W003: &str = "W003";

/// Result of SQL parsing/validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResult {
    /// Whether the SQL is syntactically valid.
    pub valid: bool,
    /// Error message if invalid.
    pub error: Option<String>,
    /// Error location (byte offset) if available.
    pub error_location: Option<usize>,
}

/// Type of SQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
    CreateTable,
    AlterTable,
    DropTable,
    CreateIndex,
    DropIndex,
    Truncate,
    Begin,
    Commit,
    Rollback,
    With,
    Other,
}

/// Lint level for issues found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintLevel {
    /// Informational only.
    Info,
    /// Potential issue, but may be intentional.
    Warning,
    /// Likely a bug or dangerous operation.
    Error,
}

/// A lint issue found in SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintIssue {
    pub level: LintLevel,
    pub code: &'static str,
    pub message: String,
}

/// Result of linting a SQL query.
#[derive(Debug, Clone, Default)]
pub struct LintResult {
    pub issues: Vec<LintIssue>,
}

impl LintResult {
    /// Returns true if there are no issues.
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns true if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.level == LintLevel::Error)
    }

    /// Returns true if there are any warnings or errors.
    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.level == LintLevel::Warning || i.level == LintLevel::Error)
    }
}

/// Check if a SQL string is syntactically valid.
///
/// # Example
/// ```
/// use pgorm_check::is_valid_sql;
///
/// assert!(is_valid_sql("SELECT * FROM users").valid);
/// assert!(!is_valid_sql("SELEC * FROM users").valid);
/// ```
pub fn is_valid_sql(sql: &str) -> ParseResult {
    analyze_sql(sql).parse_result
}

/// Detect the type of SQL statement.
///
/// # Example
/// ```
/// use pgorm_check::{detect_statement_kind, StatementKind};
///
/// assert_eq!(detect_statement_kind("SELECT * FROM users"), Some(StatementKind::Select));
/// assert_eq!(detect_statement_kind("DELETE FROM users WHERE id = 1"), Some(StatementKind::Delete));
/// ```
pub fn detect_statement_kind(sql: &str) -> Option<StatementKind> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.statement_kind
    } else {
        None
    }
}

/// Check if a SELECT query has a LIMIT clause.
///
/// Returns `true` if the query has LIMIT, `false` otherwise.
/// Returns `None` if the SQL is invalid or not a SELECT statement.
///
/// # Example
/// ```
/// use pgorm_check::select_has_limit;
///
/// assert_eq!(select_has_limit("SELECT * FROM users LIMIT 10"), Some(true));
/// assert_eq!(select_has_limit("SELECT * FROM users"), Some(false));
/// assert_eq!(select_has_limit("DELETE FROM users"), None); // Not a SELECT
/// ```
pub fn select_has_limit(sql: &str) -> Option<bool> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.select_has_limit
    } else {
        None
    }
}

/// Check if a DELETE query has a WHERE clause.
///
/// Returns `true` if the query has WHERE, `false` otherwise.
/// Returns `None` if the SQL is invalid or not a DELETE statement.
///
/// # Example
/// ```
/// use pgorm_check::delete_has_where;
///
/// assert_eq!(delete_has_where("DELETE FROM users WHERE id = 1"), Some(true));
/// assert_eq!(delete_has_where("DELETE FROM users"), Some(false));
/// assert_eq!(delete_has_where("SELECT * FROM users"), None); // Not a DELETE
/// ```
pub fn delete_has_where(sql: &str) -> Option<bool> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.delete_has_where
    } else {
        None
    }
}

/// Check if an UPDATE query has a WHERE clause.
///
/// Returns `true` if the query has WHERE, `false` otherwise.
/// Returns `None` if the SQL is invalid or not an UPDATE statement.
///
/// # Example
/// ```
/// use pgorm_check::update_has_where;
///
/// assert_eq!(update_has_where("UPDATE users SET name = 'foo' WHERE id = 1"), Some(true));
/// assert_eq!(update_has_where("UPDATE users SET name = 'foo'"), Some(false));
/// assert_eq!(update_has_where("SELECT * FROM users"), None); // Not an UPDATE
/// ```
pub fn update_has_where(sql: &str) -> Option<bool> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.update_has_where
    } else {
        None
    }
}

/// Check if a SELECT query uses SELECT *.
///
/// Returns `true` if the query uses `*` in the select list.
/// Returns `None` if the SQL is invalid or not a SELECT statement.
///
/// # Example
/// ```
/// use pgorm_check::select_has_star;
///
/// assert_eq!(select_has_star("SELECT * FROM users"), Some(true));
/// assert_eq!(select_has_star("SELECT id, name FROM users"), Some(false));
/// assert_eq!(select_has_star("SELECT t.* FROM users t"), Some(true));
/// ```
pub fn select_has_star(sql: &str) -> Option<bool> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.select_has_star
    } else {
        None
    }
}

/// Get all table names referenced in a SQL query.
///
/// # Example
/// ```
/// use pgorm_check::get_table_names;
///
/// let tables = get_table_names("SELECT * FROM users u JOIN orders o ON u.id = o.user_id");
/// assert!(tables.contains(&"users".to_string()));
/// assert!(tables.contains(&"orders".to_string()));
/// ```
pub fn get_table_names(sql: &str) -> Vec<String> {
    let analysis = analyze_sql(sql);
    if analysis.parse_result.valid {
        analysis.table_names
    } else {
        Vec::new()
    }
}

/// Lint a SQL query for common issues.
///
/// Checks for:
/// - SELECT without LIMIT (warning for `select_many` patterns)
/// - DELETE without WHERE (error)
/// - UPDATE without WHERE (error)
/// - SELECT * usage (info)
///
/// # Example
/// ```
/// use pgorm_check::{lint_sql, LintLevel};
///
/// let result = lint_sql("DELETE FROM users");
/// assert!(result.has_errors());
///
/// let result = lint_sql("SELECT * FROM users");
/// assert!(!result.has_errors());
/// ```
pub fn lint_sql(sql: &str) -> LintResult {
    let mut result = LintResult::default();

    let analysis = analyze_sql(sql);
    if !analysis.parse_result.valid {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: LINT_E001,
            message: format!(
                "SQL syntax error: {}",
                analysis.parse_result.error.unwrap_or_default()
            ),
        });
        return result;
    }

    match analysis.statement_kind {
        Some(StatementKind::Select) => {
            // Check for SELECT *
            if analysis.select_has_star == Some(true) {
                result.issues.push(LintIssue {
                    level: LintLevel::Info,
                    code: LINT_I001,
                    message: "SELECT * used; consider specifying columns explicitly".to_string(),
                });
            }
        }
        Some(StatementKind::Delete) => {
            if analysis.delete_has_where == Some(false) {
                result.issues.push(LintIssue {
                    level: LintLevel::Error,
                    code: LINT_E002,
                    message: "DELETE without WHERE clause will delete all rows".to_string(),
                });
            }
        }
        Some(StatementKind::Update) => {
            if analysis.update_has_where == Some(false) {
                result.issues.push(LintIssue {
                    level: LintLevel::Error,
                    code: LINT_E003,
                    message: "UPDATE without WHERE clause will update all rows".to_string(),
                });
            }
        }
        Some(StatementKind::Truncate) => {
            result.issues.push(LintIssue {
                level: LintLevel::Warning,
                code: LINT_W001,
                message: "TRUNCATE will delete all rows from the table".to_string(),
            });
        }
        Some(StatementKind::DropTable) => {
            result.issues.push(LintIssue {
                level: LintLevel::Warning,
                code: LINT_W002,
                message: "DROP TABLE is a destructive operation".to_string(),
            });
        }
        _ => {}
    }

    result
}

/// Lint a SELECT query specifically for "select many" patterns.
///
/// This is useful when you expect a query to return multiple rows
/// and want to ensure it has proper safeguards like LIMIT.
///
/// # Example
/// ```
/// use pgorm_check::{lint_select_many, LintLevel};
///
/// let result = lint_select_many("SELECT * FROM users");
/// assert!(result.has_warnings()); // Missing LIMIT
///
/// let result = lint_select_many("SELECT id, name FROM users LIMIT 100");
/// assert!(result.is_ok()); // Has LIMIT, no SELECT *
/// ```
pub fn lint_select_many(sql: &str) -> LintResult {
    let mut result = LintResult::default();

    let analysis = analyze_sql(sql);
    if !analysis.parse_result.valid {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: LINT_E001,
            message: format!(
                "SQL syntax error: {}",
                analysis.parse_result.error.unwrap_or_default()
            ),
        });
        return result;
    }

    if analysis.statement_kind != Some(StatementKind::Select) {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: LINT_E004,
            message: "Expected a SELECT statement".to_string(),
        });
        return result;
    }

    if analysis.select_has_limit != Some(true) {
        result.issues.push(LintIssue {
            level: LintLevel::Warning,
            code: LINT_W003,
            message: "SELECT without LIMIT may return unbounded results".to_string(),
        });
    }

    if analysis.select_has_star == Some(true) {
        result.issues.push(LintIssue {
            level: LintLevel::Info,
            code: LINT_I001,
            message: "SELECT * used; consider specifying columns explicitly".to_string(),
        });
    }

    result
}

/// Column reference extracted from SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    /// Table qualifier (alias or table name), if present.
    pub qualifier: Option<String>,
    /// Column name.
    pub column: String,
}

/// Get all column references from a SQL query.
///
/// Returns a list of (qualifier, column) pairs. The qualifier is the table alias
/// or table name if specified, or None for unqualified column references.
///
/// # Example
/// ```
/// use pgorm_check::get_column_refs;
///
/// let cols = get_column_refs("SELECT id, u.name FROM users u WHERE u.status = 'active'");
/// assert!(cols.iter().any(|c| c.column == "id" && c.qualifier.is_none()));
/// assert!(cols.iter().any(|c| c.column == "name" && c.qualifier == Some("u".to_string())));
/// assert!(cols.iter().any(|c| c.column == "status" && c.qualifier == Some("u".to_string())));
/// ```
pub fn get_column_refs(sql: &str) -> Vec<ColumnRef> {
    let analysis = analyze_sql(sql);
    if !analysis.parse_result.valid {
        return Vec::new();
    }

    let mut columns = Vec::new();
    for full in &analysis.column_refs {
        let Some(col_ref) = to_column_ref(full) else {
            continue;
        };
        if !columns.contains(&col_ref) {
            columns.push(col_ref);
        }
    }
    columns
}

fn to_column_ref(full: &ColumnRefFull) -> Option<ColumnRef> {
    if full.has_star || full.parts.is_empty() {
        return None;
    }

    let col_ref = match full.parts.len() {
        1 => ColumnRef {
            qualifier: None,
            column: full.parts[0].clone(),
        },
        2 => ColumnRef {
            qualifier: Some(full.parts[0].clone()),
            column: full.parts[1].clone(),
        },
        3 => ColumnRef {
            // schema.table.column -> qualifier = table
            qualifier: Some(full.parts[1].clone()),
            column: full.parts[2].clone(),
        },
        _ => return None,
    };

    Some(col_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_sql() {
        assert!(is_valid_sql("SELECT * FROM users").valid);
        assert!(is_valid_sql("SELECT id, name FROM users WHERE id = 1").valid);
        assert!(is_valid_sql("INSERT INTO users (name) VALUES ('foo')").valid);

        let invalid = is_valid_sql("SELEC * FROM users");
        assert!(!invalid.valid);
        assert!(invalid.error.is_some());
    }

    #[test]
    fn test_detect_statement_kind() {
        assert_eq!(
            detect_statement_kind("SELECT * FROM users"),
            Some(StatementKind::Select)
        );
        assert_eq!(
            detect_statement_kind("INSERT INTO users (name) VALUES ('foo')"),
            Some(StatementKind::Insert)
        );
        assert_eq!(
            detect_statement_kind("UPDATE users SET name = 'bar'"),
            Some(StatementKind::Update)
        );
        assert_eq!(
            detect_statement_kind("DELETE FROM users"),
            Some(StatementKind::Delete)
        );
        assert_eq!(
            detect_statement_kind("CREATE TABLE foo (id INT)"),
            Some(StatementKind::CreateTable)
        );
        assert_eq!(
            detect_statement_kind("TRUNCATE users"),
            Some(StatementKind::Truncate)
        );
    }

    #[test]
    fn test_select_has_limit() {
        assert_eq!(select_has_limit("SELECT * FROM users LIMIT 10"), Some(true));
        assert_eq!(
            select_has_limit("SELECT * FROM users LIMIT 10 OFFSET 5"),
            Some(true)
        );
        assert_eq!(select_has_limit("SELECT * FROM users OFFSET 5"), Some(true));
        assert_eq!(select_has_limit("SELECT * FROM users"), Some(false));
        assert_eq!(select_has_limit("DELETE FROM users"), None);
    }

    #[test]
    fn test_delete_has_where() {
        assert_eq!(
            delete_has_where("DELETE FROM users WHERE id = 1"),
            Some(true)
        );
        assert_eq!(delete_has_where("DELETE FROM users"), Some(false));
        assert_eq!(delete_has_where("SELECT * FROM users"), None);
    }

    #[test]
    fn test_update_has_where() {
        assert_eq!(
            update_has_where("UPDATE users SET name = 'foo' WHERE id = 1"),
            Some(true)
        );
        assert_eq!(
            update_has_where("UPDATE users SET name = 'foo'"),
            Some(false)
        );
        assert_eq!(update_has_where("SELECT * FROM users"), None);
    }

    #[test]
    fn test_select_has_star() {
        assert_eq!(select_has_star("SELECT * FROM users"), Some(true));
        assert_eq!(select_has_star("SELECT t.* FROM users t"), Some(true));
        assert_eq!(select_has_star("SELECT id, name FROM users"), Some(false));
        assert_eq!(select_has_star("DELETE FROM users"), None);
    }

    #[test]
    fn test_get_table_names() {
        let tables = get_table_names("SELECT * FROM users");
        assert_eq!(tables, vec!["users"]);

        let tables = get_table_names("SELECT * FROM users u JOIN orders o ON u.id = o.user_id");
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"orders".to_string()));

        let tables = get_table_names("SELECT * FROM public.users");
        assert_eq!(tables, vec!["public.users"]);
    }

    #[test]
    fn test_lint_sql() {
        // DELETE without WHERE
        let result = lint_sql("DELETE FROM users");
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == LINT_E002));

        // UPDATE without WHERE
        let result = lint_sql("UPDATE users SET name = 'foo'");
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == LINT_E003));

        // Valid queries
        let result = lint_sql("DELETE FROM users WHERE id = 1");
        assert!(!result.has_errors());

        let result = lint_sql("UPDATE users SET name = 'foo' WHERE id = 1");
        assert!(!result.has_errors());

        // SELECT * (info level, not an error)
        let result = lint_sql("SELECT * FROM users");
        assert!(!result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == LINT_I001));
    }

    #[test]
    fn test_lint_select_many() {
        // Without LIMIT
        let result = lint_select_many("SELECT * FROM users");
        assert!(result.has_warnings());
        assert!(result.issues.iter().any(|i| i.code == LINT_W003));

        // With LIMIT
        let result = lint_select_many("SELECT * FROM users LIMIT 100");
        assert!(!result.has_warnings());

        // Not a SELECT
        let result = lint_select_many("DELETE FROM users");
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == LINT_E004));
    }
}
