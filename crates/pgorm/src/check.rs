//! SQL schema checking and linting utilities.
//!
//! This module provides runtime SQL validation against registered table schemas.
//! It can detect issues like missing tables, missing columns, dangerous operations, etc.
//!
//! # Example
//!
//! ```ignore
//! use pgorm::{SchemaRegistry, TableMeta, lint_sql, LintLevel};
//!
//! // Register tables
//! let mut registry = SchemaRegistry::new();
//! registry.register::<User>();
//! registry.register::<Order>();
//!
//! // Check SQL against schema
//! let issues = registry.check_sql("SELECT id, name FROM users");
//! assert!(issues.is_empty());
//!
//! // Lint SQL for common issues
//! let result = lint_sql("DELETE FROM users");
//! assert!(result.has_errors()); // Missing WHERE clause
//! ```

use std::collections::HashMap;

/// Metadata for a database table.
///
/// This trait is automatically implemented by the `#[derive(Model)]` macro.
/// It provides table name and column information for schema validation.
pub trait TableMeta {
    /// The database table name.
    fn table_name() -> &'static str;

    /// The database schema name (defaults to "public").
    fn schema_name() -> &'static str {
        "public"
    }

    /// List of column names in this table.
    fn columns() -> &'static [&'static str];

    /// The primary key column name, if any.
    fn primary_key() -> Option<&'static str> {
        None
    }
}

/// Column information for schema checking.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    /// Column name.
    pub name: String,
    /// Whether this column is the primary key.
    pub is_primary_key: bool,
}

/// Table information for schema checking.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Schema name (e.g., "public").
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Column metadata.
    pub columns: Vec<ColumnMeta>,
}

impl TableSchema {
    /// Create a new table schema.
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
            columns: Vec::new(),
        }
    }

    /// Add a column to this table schema.
    pub fn add_column(&mut self, name: impl Into<String>, is_primary_key: bool) {
        self.columns.push(ColumnMeta {
            name: name.into(),
            is_primary_key,
        });
    }

    /// Add multiple columns to this table schema.
    pub fn with_columns(mut self, columns: &[&str]) -> Self {
        for col in columns {
            self.columns.push(ColumnMeta {
                name: col.to_string(),
                is_primary_key: false,
            });
        }
        self
    }

    /// Set the primary key column.
    pub fn with_primary_key(mut self, pk: &str) -> Self {
        for col in &mut self.columns {
            col.is_primary_key = col.name == pk;
        }
        // If the primary key column doesn't exist, add it
        if !self.columns.iter().any(|c| c.name == pk) {
            self.columns.push(ColumnMeta {
                name: pk.to_string(),
                is_primary_key: true,
            });
        }
        self
    }

    /// Check if this table has a column with the given name.
    pub fn has_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c.name == name)
    }
}

/// Registry for table schemas.
///
/// Use this to register all your model tables and then check SQL against them.
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    /// Map of (schema, table) -> TableSchema
    tables: HashMap<(String, String), TableSchema>,
}

impl SchemaRegistry {
    /// Create a new empty schema registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a table from a type that implements `TableMeta`.
    pub fn register<T: TableMeta>(&mut self) {
        let schema_name = T::schema_name().to_string();
        let table_name = T::table_name().to_string();
        let columns = T::columns();
        let pk = T::primary_key();

        let mut table = TableSchema::new(&schema_name, &table_name);
        for col in columns {
            let is_pk = pk.map_or(false, |p| p == *col);
            table.add_column(*col, is_pk);
        }

        self.tables.insert((schema_name, table_name), table);
    }

    /// Register a table schema directly.
    pub fn register_table(&mut self, table: TableSchema) {
        let key = (table.schema.clone(), table.name.clone());
        self.tables.insert(key, table);
    }

    /// Get a table by schema and name.
    pub fn get_table(&self, schema: &str, name: &str) -> Option<&TableSchema> {
        self.tables.get(&(schema.to_string(), name.to_string()))
    }

    /// Find a table by name, searching all schemas.
    pub fn find_table(&self, name: &str) -> Option<&TableSchema> {
        // First try public schema
        if let Some(t) = self.get_table("public", name) {
            return Some(t);
        }
        // Then try any schema
        self.tables.values().find(|t| t.name == name)
    }

    /// Check if a table exists.
    pub fn has_table(&self, schema: &str, name: &str) -> bool {
        self.tables
            .contains_key(&(schema.to_string(), name.to_string()))
    }

    /// Get all registered tables.
    pub fn tables(&self) -> impl Iterator<Item = &TableSchema> {
        self.tables.values()
    }

    /// Get the number of registered tables.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Level of a schema issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaIssueLevel {
    /// Informational message.
    Info,
    /// Warning - may be intentional.
    Warning,
    /// Error - likely a bug.
    Error,
}

/// Kind of schema issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaIssueKind {
    /// SQL parse error.
    ParseError,
    /// Referenced table not found.
    MissingTable,
    /// Referenced column not found.
    MissingColumn,
}

/// A schema validation issue.
#[derive(Debug, Clone)]
pub struct SchemaIssue {
    /// Severity level.
    pub level: SchemaIssueLevel,
    /// Type of issue.
    pub kind: SchemaIssueKind,
    /// Human-readable message.
    pub message: String,
}

// ============================================================================
// SQL Linting (requires "check" feature)
// ============================================================================

#[cfg(feature = "check")]
mod lint {
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
    pub fn is_valid_sql(sql: &str) -> ParseResult {
        match pg_query::parse(sql) {
            Ok(_) => ParseResult {
                valid: true,
                error: None,
                error_location: None,
            },
            Err(e) => {
                let error_str = e.to_string();
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
                pg_query::protobuf::TransactionStmtKind::TransStmtBegin => {
                    Some(StatementKind::Begin)
                }
                pg_query::protobuf::TransactionStmtKind::TransStmtCommit => {
                    Some(StatementKind::Commit)
                }
                pg_query::protobuf::TransactionStmtKind::TransStmtRollback => {
                    Some(StatementKind::Rollback)
                }
                _ => Some(StatementKind::Other),
            },
            _ => Some(StatementKind::Other),
        }
    }

    /// Check if a SELECT query has a LIMIT clause.
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
    pub fn get_table_names(sql: &str) -> Vec<String> {
        let mut tables = Vec::new();

        let Ok(parsed) = pg_query::parse(sql) else {
            return tables;
        };

        let cte_names: std::collections::HashSet<String> =
            parsed.cte_names.into_iter().collect();

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
    pub fn lint_sql(sql: &str) -> LintResult {
        let mut result = LintResult::default();

        // First check if SQL is valid
        let parse_result = is_valid_sql(sql);
        if !parse_result.valid {
            result.issues.push(LintIssue {
                level: LintLevel::Error,
                code: "E001",
                message: format!(
                    "SQL syntax error: {}",
                    parse_result.error.unwrap_or_default()
                ),
            });
            return result;
        }

        let kind = detect_statement_kind(sql);

        match kind {
            Some(StatementKind::Select) => {
                if select_has_star(sql) == Some(true) {
                    result.issues.push(LintIssue {
                        level: LintLevel::Info,
                        code: "I001",
                        message: "SELECT * used; consider specifying columns explicitly"
                            .to_string(),
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
    pub fn lint_select_many(sql: &str) -> LintResult {
        let mut result = LintResult::default();

        let parse_result = is_valid_sql(sql);
        if !parse_result.valid {
            result.issues.push(LintIssue {
                level: LintLevel::Error,
                code: "E001",
                message: format!(
                    "SQL syntax error: {}",
                    parse_result.error.unwrap_or_default()
                ),
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

    fn extract_error_location(error: &str) -> Option<usize> {
        if let Some(pos) = error.rfind("position ") {
            let after_pos = &error[pos + 9..];
            let num_str: String = after_pos.chars().take_while(|c| c.is_ascii_digit()).collect();
            return num_str.parse().ok();
        }
        None
    }
}

#[cfg(feature = "check")]
pub use lint::{
    delete_has_where, detect_statement_kind, get_table_names, is_valid_sql, lint_select_many,
    lint_sql, select_has_limit, select_has_star, update_has_where, LintIssue, LintLevel,
    LintResult, ParseResult, StatementKind,
};

// Schema checking with lint features
#[cfg(feature = "check")]
impl SchemaRegistry {
    /// Check SQL against this registry's schema.
    pub fn check_sql(&self, sql: &str) -> Vec<SchemaIssue> {
        let mut issues = Vec::new();

        // First check if SQL is valid
        let parse_result = is_valid_sql(sql);
        if !parse_result.valid {
            issues.push(SchemaIssue {
                level: SchemaIssueLevel::Error,
                kind: SchemaIssueKind::ParseError,
                message: format!(
                    "SQL syntax error: {}",
                    parse_result.error.unwrap_or_default()
                ),
            });
            return issues;
        }

        // Get referenced tables
        let tables = get_table_names(sql);

        for table_ref in &tables {
            // Handle schema.table format
            let (schema, table_name) = if table_ref.contains('.') {
                let parts: Vec<&str> = table_ref.splitn(2, '.').collect();
                (parts[0], parts[1])
            } else {
                ("public", table_ref.as_str())
            };

            if !self.has_table(schema, table_name) {
                // Try to find in any schema
                if self.find_table(table_name).is_none() {
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Table not found in registry: {}", table_ref),
                    });
                }
            }
        }

        issues
    }

    /// Lint SQL for common issues (doesn't require schema).
    pub fn lint(&self, sql: &str) -> LintResult {
        lint_sql(sql)
    }

    /// Validate that SQL is syntactically correct.
    pub fn is_valid(&self, sql: &str) -> bool {
        is_valid_sql(sql).valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestUser;

    impl TableMeta for TestUser {
        fn table_name() -> &'static str {
            "users"
        }

        fn columns() -> &'static [&'static str] {
            &["id", "name", "email", "created_at"]
        }

        fn primary_key() -> Option<&'static str> {
            Some("id")
        }
    }

    struct TestOrder;

    impl TableMeta for TestOrder {
        fn table_name() -> &'static str {
            "orders"
        }

        fn columns() -> &'static [&'static str] {
            &["id", "user_id", "total", "status"]
        }

        fn primary_key() -> Option<&'static str> {
            Some("id")
        }
    }

    #[test]
    fn test_register_table() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();
        registry.register::<TestOrder>();

        assert_eq!(registry.len(), 2);
        assert!(registry.has_table("public", "users"));
        assert!(registry.has_table("public", "orders"));
        assert!(!registry.has_table("public", "products"));
    }

    #[test]
    fn test_find_table() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();

        let table = registry.find_table("users").unwrap();
        assert_eq!(table.name, "users");
        assert!(table.has_column("id"));
        assert!(table.has_column("name"));
        assert!(!table.has_column("nonexistent"));
    }

    #[test]
    fn test_table_schema_builder() {
        let table = TableSchema::new("public", "products")
            .with_columns(&["id", "name", "price"])
            .with_primary_key("id");

        assert_eq!(table.name, "products");
        assert!(table.has_column("id"));
        assert!(table.has_column("name"));
        assert!(table.has_column("price"));

        let pk_col = table.columns.iter().find(|c| c.is_primary_key).unwrap();
        assert_eq!(pk_col.name, "id");
    }

    #[cfg(feature = "check")]
    mod check_tests {
        use super::*;

        #[test]
        fn test_is_valid_sql() {
            assert!(is_valid_sql("SELECT * FROM users").valid);
            assert!(!is_valid_sql("SELEC * FROM users").valid);
        }

        #[test]
        fn test_detect_statement_kind() {
            assert_eq!(
                detect_statement_kind("SELECT * FROM users"),
                Some(StatementKind::Select)
            );
            assert_eq!(
                detect_statement_kind("DELETE FROM users"),
                Some(StatementKind::Delete)
            );
            assert_eq!(
                detect_statement_kind("UPDATE users SET name = 'foo'"),
                Some(StatementKind::Update)
            );
        }

        #[test]
        fn test_lint_sql() {
            let result = lint_sql("DELETE FROM users");
            assert!(result.has_errors());

            let result = lint_sql("DELETE FROM users WHERE id = 1");
            assert!(!result.has_errors());
        }

        #[test]
        fn test_check_sql_schema() {
            let mut registry = SchemaRegistry::new();
            registry.register::<TestUser>();
            registry.register::<TestOrder>();

            // Valid SQL - tables exist
            let issues = registry.check_sql("SELECT * FROM users");
            assert!(issues.is_empty());

            // Invalid SQL - table doesn't exist
            let issues = registry.check_sql("SELECT * FROM products");
            assert!(!issues.is_empty());
            assert!(matches!(issues[0].kind, SchemaIssueKind::MissingTable));
        }
    }
}
