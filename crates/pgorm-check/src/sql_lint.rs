//! SQL linting utilities for common safety and correctness checks.
//!
//! This module provides functions to validate SQL queries for common issues:
//! - SELECT queries missing LIMIT (potential memory issues)
//! - DELETE/UPDATE queries missing WHERE (dangerous operations)
//! - SQL syntax validation
//! - Statement type detection

use serde::{Deserialize, Serialize};

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
    match pg_query::parse(sql) {
        Ok(_) => ParseResult {
            valid: true,
            error: None,
            error_location: None,
        },
        Err(e) => {
            let error_str = e.to_string();
            // pg_query error format: "error message at or near \"token\" at position N"
            let location = extract_error_location(&error_str);
            ParseResult {
                valid: false,
                error: Some(error_str),
                error_location: location,
            }
        }
    }
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
    let parsed = pg_query::parse(sql).ok()?;
    let stmts = parsed.protobuf.stmts;

    if stmts.is_empty() {
        return None;
    }

    let stmt = stmts.first()?.stmt.as_ref()?;

    use pg_query::NodeEnum;
    match stmt.node.as_ref()? {
        NodeEnum::SelectStmt(_) => Some(StatementKind::Select),
        NodeEnum::InsertStmt(_) => Some(StatementKind::Insert),
        NodeEnum::UpdateStmt(_) => Some(StatementKind::Update),
        NodeEnum::DeleteStmt(_) => Some(StatementKind::Delete),
        NodeEnum::CreateStmt(_) => Some(StatementKind::CreateTable),
        NodeEnum::AlterTableStmt(_) => Some(StatementKind::AlterTable),
        NodeEnum::DropStmt(_) => Some(StatementKind::DropTable),
        NodeEnum::IndexStmt(_) => Some(StatementKind::CreateIndex),
        NodeEnum::TruncateStmt(_) => Some(StatementKind::Truncate),
        NodeEnum::TransactionStmt(t) => match t.kind() {
            pg_query::protobuf::TransactionStmtKind::TransStmtBegin => Some(StatementKind::Begin),
            pg_query::protobuf::TransactionStmtKind::TransStmtCommit => Some(StatementKind::Commit),
            pg_query::protobuf::TransactionStmtKind::TransStmtRollback => {
                Some(StatementKind::Rollback)
            }
            _ => Some(StatementKind::Other),
        },
        _ => Some(StatementKind::Other),
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
    let parsed = pg_query::parse(sql).ok()?;
    let stmts = parsed.protobuf.stmts;

    if stmts.is_empty() {
        return None;
    }

    let stmt = stmts.first()?.stmt.as_ref()?;

    if let pg_query::NodeEnum::SelectStmt(select) = stmt.node.as_ref()? {
        return Some(select.limit_count.is_some() || select.limit_offset.is_some());
    }

    None
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
    let parsed = pg_query::parse(sql).ok()?;
    let stmts = parsed.protobuf.stmts;

    if stmts.is_empty() {
        return None;
    }

    let stmt = stmts.first()?.stmt.as_ref()?;

    if let pg_query::NodeEnum::DeleteStmt(delete) = stmt.node.as_ref()? {
        return Some(delete.where_clause.is_some());
    }

    None
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
    let parsed = pg_query::parse(sql).ok()?;
    let stmts = parsed.protobuf.stmts;

    if stmts.is_empty() {
        return None;
    }

    let stmt = stmts.first()?.stmt.as_ref()?;

    if let pg_query::NodeEnum::UpdateStmt(update) = stmt.node.as_ref()? {
        return Some(update.where_clause.is_some());
    }

    None
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
    let parsed = pg_query::parse(sql).ok()?;

    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::ColumnRef(c) = node {
            for f in &c.fields {
                if let Some(pg_query::NodeEnum::AStar(_)) = f.node.as_ref() {
                    return Some(true);
                }
            }
        }
    }

    // Make sure it's a SELECT statement
    let stmts = parsed.protobuf.stmts;
    if stmts.is_empty() {
        return None;
    }
    let stmt = stmts.first()?.stmt.as_ref()?;
    if let pg_query::NodeEnum::SelectStmt(_) = stmt.node.as_ref()? {
        return Some(false);
    }

    None
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
    let mut tables = Vec::new();

    let Ok(parsed) = pg_query::parse(sql) else {
        return tables;
    };

    let cte_names: std::collections::HashSet<String> = parsed.cte_names.into_iter().collect();

    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::RangeVar(v) = node {
            // Skip CTE references
            if cte_names.contains(&v.relname) {
                continue;
            }

            let table_name = if v.schemaname.is_empty() {
                v.relname.clone()
            } else {
                format!("{}.{}", v.schemaname, v.relname)
            };

            if !tables.contains(&table_name) {
                tables.push(table_name);
            }
        }
    }

    tables
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

    // First check if SQL is valid
    let parse_result = is_valid_sql(sql);
    if !parse_result.valid {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: "E001",
            message: format!("SQL syntax error: {}", parse_result.error.unwrap_or_default()),
        });
        return result;
    }

    let kind = detect_statement_kind(sql);

    match kind {
        Some(StatementKind::Select) => {
            // Check for SELECT *
            if select_has_star(sql) == Some(true) {
                result.issues.push(LintIssue {
                    level: LintLevel::Info,
                    code: "I001",
                    message: "SELECT * used; consider specifying columns explicitly".to_string(),
                });
            }
        }
        Some(StatementKind::Delete) => {
            if delete_has_where(sql) == Some(false) {
                result.issues.push(LintIssue {
                    level: LintLevel::Error,
                    code: "E002",
                    message: "DELETE without WHERE clause will delete all rows".to_string(),
                });
            }
        }
        Some(StatementKind::Update) => {
            if update_has_where(sql) == Some(false) {
                result.issues.push(LintIssue {
                    level: LintLevel::Error,
                    code: "E003",
                    message: "UPDATE without WHERE clause will update all rows".to_string(),
                });
            }
        }
        Some(StatementKind::Truncate) => {
            result.issues.push(LintIssue {
                level: LintLevel::Warning,
                code: "W001",
                message: "TRUNCATE will delete all rows from the table".to_string(),
            });
        }
        Some(StatementKind::DropTable) => {
            result.issues.push(LintIssue {
                level: LintLevel::Warning,
                code: "W002",
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

    let parse_result = is_valid_sql(sql);
    if !parse_result.valid {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: "E001",
            message: format!("SQL syntax error: {}", parse_result.error.unwrap_or_default()),
        });
        return result;
    }

    let kind = detect_statement_kind(sql);
    if kind != Some(StatementKind::Select) {
        result.issues.push(LintIssue {
            level: LintLevel::Error,
            code: "E004",
            message: "Expected a SELECT statement".to_string(),
        });
        return result;
    }

    if select_has_limit(sql) != Some(true) {
        result.issues.push(LintIssue {
            level: LintLevel::Warning,
            code: "W003",
            message: "SELECT without LIMIT may return unbounded results".to_string(),
        });
    }

    if select_has_star(sql) == Some(true) {
        result.issues.push(LintIssue {
            level: LintLevel::Info,
            code: "I001",
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
    let mut columns = Vec::new();

    let Ok(parsed) = pg_query::parse(sql) else {
        return columns;
    };

    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::ColumnRef(c) = node {
            let mut parts: Vec<String> = Vec::new();
            let mut has_star = false;

            for f in &c.fields {
                match f.node.as_ref() {
                    Some(pg_query::NodeEnum::String(s)) => parts.push(s.sval.clone()),
                    Some(pg_query::NodeEnum::AStar(_)) => has_star = true,
                    _ => {}
                }
            }

            // Skip SELECT * or table.*
            if has_star || parts.is_empty() {
                continue;
            }

            let col_ref = match parts.len() {
                1 => ColumnRef {
                    qualifier: None,
                    column: parts[0].clone(),
                },
                2 => ColumnRef {
                    qualifier: Some(parts[0].clone()),
                    column: parts[1].clone(),
                },
                3 => ColumnRef {
                    // schema.table.column -> qualifier = table
                    qualifier: Some(parts[1].clone()),
                    column: parts[2].clone(),
                },
                _ => continue,
            };

            // Avoid duplicates
            if !columns.contains(&col_ref) {
                columns.push(col_ref);
            }
        }
    }

    columns
}

/// Extract error location from pg_query error message.
fn extract_error_location(error: &str) -> Option<usize> {
    // Format: "... at position N" or similar
    if let Some(pos) = error.rfind("position ") {
        let after_pos = &error[pos + 9..];
        let num_str: String = after_pos.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse().ok();
    }
    None
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
        assert!(result.issues.iter().any(|i| i.code == "E002"));

        // UPDATE without WHERE
        let result = lint_sql("UPDATE users SET name = 'foo'");
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == "E003"));

        // Valid queries
        let result = lint_sql("DELETE FROM users WHERE id = 1");
        assert!(!result.has_errors());

        let result = lint_sql("UPDATE users SET name = 'foo' WHERE id = 1");
        assert!(!result.has_errors());

        // SELECT * (info level, not an error)
        let result = lint_sql("SELECT * FROM users");
        assert!(!result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == "I001"));
    }

    #[test]
    fn test_lint_select_many() {
        // Without LIMIT
        let result = lint_select_many("SELECT * FROM users");
        assert!(result.has_warnings());
        assert!(result.issues.iter().any(|i| i.code == "W003"));

        // With LIMIT
        let result = lint_select_many("SELECT * FROM users LIMIT 100");
        assert!(!result.has_warnings());

        // Not a SELECT
        let result = lint_select_many("DELETE FROM users");
        assert!(result.has_errors());
        assert!(result.issues.iter().any(|i| i.code == "E004"));
    }
}
