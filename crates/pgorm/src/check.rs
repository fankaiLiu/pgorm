//! SQL schema checking and linting utilities.
//!
//! This module provides runtime SQL validation against registered table schemas.
//! It can detect issues like missing tables, missing columns, dangerous operations, etc.
//!
//! `SchemaRegistry::check_sql` uses an internal LRU parse cache (via `pgorm-check`) to reduce
//! repeated `pg_query` parsing overhead.
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
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    /// Map of (schema, table) -> TableSchema
    tables: HashMap<(String, String), TableSchema>,
    #[cfg(feature = "check")]
    parse_cache: std::sync::Arc<pgorm_check::SqlParseCache>,
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
            let is_pk = pk == Some(*col);
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

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self {
            tables: HashMap::new(),
            #[cfg(feature = "check")]
            parse_cache: std::sync::Arc::new(pgorm_check::SqlParseCache::default()),
        }
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
    /// Column reference is ambiguous across visible tables.
    AmbiguousColumn,
    /// SQL feature not supported by the checker.
    Unsupported,
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

impl std::fmt::Display for SchemaIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {:?}: {}", self.level, self.kind, self.message)
    }
}

// Re-export from pgorm-check when check feature is enabled
#[cfg(feature = "check")]
#[allow(unused_imports)]
pub use pgorm_check::{
    CheckClient,
    CheckError,
    CheckResult,
    ColumnInfo,
    // Lint types
    ColumnRef,
    ColumnRefFull,
    DbSchema,
    InsertAnalysis,
    LintIssue,
    LintLevel,
    LintResult,
    OnConflictAnalysis,
    ParseResult,
    RelationKind,
    SchemaCache,
    SchemaCacheConfig,
    SchemaCacheLoad,
    SqlAnalysis,
    SqlCheckIssue,
    SqlCheckIssueKind,
    SqlCheckLevel,
    SqlParseCache,
    StatementKind,
    TableInfo,
    TargetColumn,
    UpdateAnalysis,
    // Schema check from database
    check_sql,
    check_sql_analysis,
    check_sql_cached,
    // Lint functions
    delete_has_where,
    detect_statement_kind,
    get_column_refs,
    get_table_names,
    is_valid_sql,
    lint_select_many,
    lint_sql,
    // Schema introspection
    schema_introspect::load_schema_from_db,
    select_has_limit,
    select_has_star,
    update_has_where,
};

// Schema checking with lint features
#[cfg(feature = "check")]
impl SchemaRegistry {
    pub(crate) fn analyze_sql(&self, sql: &str) -> std::sync::Arc<SqlAnalysis> {
        self.parse_cache.analyze(sql)
    }

    /// Replace the internal SQL parse cache with a new capacity.
    ///
    /// Set `capacity` to 0 to disable caching (always re-parse).
    pub fn with_parse_cache_capacity(mut self, capacity: usize) -> Self {
        self.parse_cache = std::sync::Arc::new(SqlParseCache::new(capacity));
        self
    }

    /// Check SQL against this registry's schema.
    ///
    /// Validates:
    /// - Tables referenced in the SQL exist in the registry
    /// - Columns referenced in the SQL exist in the appropriate tables
    pub fn check_sql(&self, sql: &str) -> Vec<SchemaIssue> {
        let mut issues = Vec::new();

        let analysis = self.parse_cache.analyze(sql);
        if !analysis.parse_result.valid {
            issues.push(SchemaIssue {
                level: SchemaIssueLevel::Error,
                kind: SchemaIssueKind::ParseError,
                message: format!(
                    "SQL syntax error: {}",
                    analysis.parse_result.error.clone().unwrap_or_default()
                ),
            });
            return issues;
        }

        // System columns exist on every table but are not modeled in TableMeta.
        // Keep this allocation-free (hot path).
        fn is_system_column(col: &str) -> bool {
            matches!(col, "ctid" | "xmin" | "xmax" | "cmin" | "cmax" | "tableoid")
        }

        // Build a map of qualifier -> table using RangeVar + alias info.
        let mut qualifier_to_table: std::collections::HashMap<String, &TableSchema> =
            std::collections::HashMap::new();
        let mut visible_tables: Vec<&TableSchema> = Vec::new();

        for rv in &analysis.range_vars {
            // Skip CTE references.
            if analysis.cte_names.contains(&rv.table) {
                continue;
            }

            let rel_schema = rv.schema.as_deref();
            let rel_name = rv.table.as_str();
            let qualifier = rv.alias.as_deref().unwrap_or(rel_name);

            let table = if let Some(s) = rel_schema {
                self.get_table(s, rel_name)
            } else {
                self.find_table(rel_name)
            };

            match table {
                Some(t) => {
                    // If an alias exists, the base name should not be visible.
                    if qualifier_to_table
                        .insert(qualifier.to_string(), t)
                        .is_none()
                    {
                        visible_tables.push(t);
                    }
                }
                None => {
                    let name = match rel_schema {
                        Some(s) => format!("{s}.{rel_name}"),
                        None => rel_name.to_string(),
                    };
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Table not found in registry: {name}"),
                    });
                }
            }
        }

        // Validate target columns for INSERT/UPDATE/ON CONFLICT.
        if let Some(insert) = &analysis.insert {
            if let Some(target) = &insert.target {
                let table = if let Some(s) = target.schema.as_deref() {
                    self.get_table(s, &target.table)
                } else {
                    self.find_table(&target.table)
                };

                if let Some(t) = table {
                    for col in &insert.columns {
                        if is_system_column(col.name.as_str()) {
                            continue;
                        }
                        if !t.has_column(&col.name) {
                            issues.push(SchemaIssue {
                                level: SchemaIssueLevel::Error,
                                kind: SchemaIssueKind::MissingColumn,
                                message: format!(
                                    "Column not found: {}.{} (INSERT target table '{}')",
                                    t.name, col.name, t.name
                                ),
                            });
                        }
                    }

                    if let Some(oc) = &insert.on_conflict {
                        if oc.has_inference_expressions {
                            issues.push(SchemaIssue {
                                level: SchemaIssueLevel::Warning,
                                kind: SchemaIssueKind::Unsupported,
                                message: "ON CONFLICT inference uses expressions; only simple column targets are checked".to_string(),
                            });
                        }

                        for col in &oc.inference_columns {
                            if is_system_column(col.name.as_str()) {
                                continue;
                            }
                            if !t.has_column(&col.name) {
                                issues.push(SchemaIssue {
                                    level: SchemaIssueLevel::Error,
                                    kind: SchemaIssueKind::MissingColumn,
                                    message: format!(
                                        "Column not found: {}.{} (ON CONFLICT target table '{}')",
                                        t.name, col.name, t.name
                                    ),
                                });
                            }
                        }

                        for col in &oc.update_set_columns {
                            if is_system_column(col.name.as_str()) {
                                continue;
                            }
                            if !t.has_column(&col.name) {
                                issues.push(SchemaIssue {
                                    level: SchemaIssueLevel::Error,
                                    kind: SchemaIssueKind::MissingColumn,
                                    message: format!(
                                        "Column not found: {}.{} (ON CONFLICT DO UPDATE SET on table '{}')",
                                        t.name, col.name, t.name
                                    ),
                                });
                            }
                        }
                    }
                } else {
                    let name = match target.schema.as_deref() {
                        Some(s) => format!("{s}.{}", target.table),
                        None => target.table.clone(),
                    };
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Table not found in registry: {name}"),
                    });
                }
            }
        }

        if let Some(update) = &analysis.update {
            if let Some(target) = &update.target {
                let table = if let Some(s) = target.schema.as_deref() {
                    self.get_table(s, &target.table)
                } else {
                    self.find_table(&target.table)
                };

                if let Some(t) = table {
                    for col in &update.set_columns {
                        if is_system_column(col.name.as_str()) {
                            continue;
                        }
                        if !t.has_column(&col.name) {
                            issues.push(SchemaIssue {
                                level: SchemaIssueLevel::Error,
                                kind: SchemaIssueKind::MissingColumn,
                                message: format!(
                                    "Column not found: {}.{} (UPDATE target table '{}')",
                                    t.name, col.name, t.name
                                ),
                            });
                        }
                    }
                } else {
                    let name = match target.schema.as_deref() {
                        Some(s) => format!("{s}.{}", target.table),
                        None => target.table.clone(),
                    };
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Table not found in registry: {name}"),
                    });
                }
            }
        }

        // Validate column references from expressions (SELECT list, WHERE, JOIN ON, etc).
        for c in &analysis.column_refs {
            if c.has_star || c.parts.is_empty() {
                continue;
            }

            // Unqualified: col
            if c.parts.len() == 1 {
                let col = c.parts[0].as_str();
                if is_system_column(col) {
                    continue;
                }

                let matches = visible_tables.iter().filter(|t| t.has_column(col)).count();

                match matches {
                    0 => {
                        if !visible_tables.is_empty() {
                            issues.push(SchemaIssue {
                                level: SchemaIssueLevel::Error,
                                kind: SchemaIssueKind::MissingColumn,
                                message: format!(
                                    "Column not found: {col} (not in any referenced tables)"
                                ),
                            });
                        }
                    }
                    1 => {}
                    _ => issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::AmbiguousColumn,
                        message: format!(
                            "Ambiguous column reference: {col} (found in multiple tables)"
                        ),
                    }),
                }

                continue;
            }

            // Qualified: qualifier.col
            if c.parts.len() == 2 {
                let qualifier = c.parts[0].as_str();
                let col = c.parts[1].as_str();

                if is_system_column(col) {
                    continue;
                }

                if let Some(t) = qualifier_to_table.get(qualifier) {
                    if !t.has_column(col) {
                        issues.push(SchemaIssue {
                            level: SchemaIssueLevel::Error,
                            kind: SchemaIssueKind::MissingColumn,
                            message: format!(
                                "Column not found: {qualifier}.{col} (table resolved to '{}')",
                                t.name
                            ),
                        });
                    }
                } else {
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Unknown table/alias qualifier: {qualifier}"),
                    });
                }

                continue;
            }

            // schema.table.col OR catalog.schema.table.col
            if c.parts.len() == 3 || c.parts.len() == 4 {
                let (schema_part, table_part, col_part) = if c.parts.len() == 3 {
                    (&c.parts[0], &c.parts[1], &c.parts[2])
                } else {
                    (&c.parts[1], &c.parts[2], &c.parts[3])
                };

                if is_system_column(col_part.as_str()) {
                    continue;
                }

                let Some(t) = self.get_table(schema_part, table_part) else {
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingTable,
                        message: format!("Table not found in registry: {schema_part}.{table_part}"),
                    });
                    continue;
                };

                if !t.has_column(col_part) {
                    issues.push(SchemaIssue {
                        level: SchemaIssueLevel::Error,
                        kind: SchemaIssueKind::MissingColumn,
                        message: format!("Column not found: {schema_part}.{table_part}.{col_part}"),
                    });
                }

                continue;
            }

            issues.push(SchemaIssue {
                level: SchemaIssueLevel::Warning,
                kind: SchemaIssueKind::Unsupported,
                message: format!(
                    "Unsupported column reference form ({} parts): {}",
                    c.parts.len(),
                    c.parts.join(".")
                ),
            });
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

/// Check multiple models against a schema registry and return results.
///
/// This macro simplifies batch model validation by automatically checking
/// each model's generated SQL against the registry.
///
/// # Example
///
/// ```ignore
/// use pgorm::{check_models, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "orders")]
/// struct Order { #[orm(id)] id: i64, user_id: i64 }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Check all models at once
/// let results = check_models!(registry, User, Order);
/// for (name, issues) in &results {
///     if issues.is_empty() {
///         println!("✓ {}", name);
///     } else {
///         println!("✗ {} ({} issues)", name, issues.len());
///     }
/// }
/// ```
#[macro_export]
macro_rules! check_models {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        let mut results: Vec<(&'static str, std::collections::HashMap<&'static str, Vec<$crate::SchemaIssue>>)> = Vec::new();
        $(
            results.push((stringify!($model), <$model>::check_schema(&$registry)));
        )+
        results
    }};
}

/// Check models and panic if any have schema issues.
///
/// Useful for startup validation to catch schema mismatches early.
///
/// # Example
///
/// ```ignore
/// use pgorm::{assert_models_valid, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Panics if any model has schema issues
/// assert_models_valid!(registry, User);
/// ```
#[macro_export]
macro_rules! assert_models_valid {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        let mut all_issues: Vec<(&'static str, Vec<String>)> = Vec::new();
        $(
            let issues = <$model>::check_schema(&$registry);
            if !issues.is_empty() {
                let messages: Vec<String> = issues
                    .iter()
                    .flat_map(|(sql_name, issue_list)| {
                        issue_list.iter().map(move |i| format!("{}: {}", sql_name, i.message))
                    })
                    .collect();
                all_issues.push((stringify!($model), messages));
            }
        )+
        if !all_issues.is_empty() {
            let mut msg = String::from("Schema validation failed:\n");
            for (model, issues) in &all_issues {
                msg.push_str(&format!("\n{}:\n", model));
                for issue in issues {
                    msg.push_str(&format!("  - {}\n", issue));
                }
            }
            panic!("{}", msg);
        }
    }};
}

/// Print schema check results for multiple models.
///
/// A convenient macro for debugging and validation output.
///
/// # Example
///
/// ```ignore
/// use pgorm::{print_model_check, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Prints validation results
/// print_model_check!(registry, User);
/// // Output:
/// // Model Schema Validation:
/// //   ✓ User
/// ```
#[macro_export]
macro_rules! print_model_check {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        println!("Model Schema Validation:");
        let mut all_valid = true;
        $(
            let issues = <$model>::check_schema(&$registry);
            if issues.is_empty() {
                println!("  ✓ {}", stringify!($model));
            } else {
                all_valid = false;
                let total: usize = issues.values().map(|v| v.len()).sum();
                println!("  ✗ {} ({} issues)", stringify!($model), total);
                for (sql_name, issue_list) in &issues {
                    for issue in issue_list {
                        println!("      {}: {:?} - {}", sql_name, issue.kind, issue.message);
                    }
                }
            }
        )+
        all_valid
    }};
}

/// Check models against the actual database schema.
///
/// This macro loads the schema from the database and checks each model
/// to ensure its columns match the actual table structure.
///
/// # Example
///
/// ```ignore
/// use pgorm::{check_models_db, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Check models against actual database
/// let results = check_models_db!(pg, User, Order).await?;
/// for result in &results {
///     result.print();
/// }
/// ```
#[macro_export]
macro_rules! check_models_db {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            let mut results: Vec<$crate::ModelCheckResult> = Vec::new();
            $(
                results.push($crate::ModelCheckResult::check::<$model>(&db_schema));
            )+
            Ok::<_, $crate::OrmError>(results)
        }
    }};
}

/// Check models against database and print results.
///
/// # Example
///
/// ```ignore
/// use pgorm::{print_models_db_check, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Check and print results
/// let all_valid = print_models_db_check!(pg, User, Order).await?;
/// ```
#[macro_export]
macro_rules! print_models_db_check {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            println!("Model Database Validation:");
            let mut all_valid = true;
            $(
                let result = $crate::ModelCheckResult::check::<$model>(&db_schema);
                if !result.is_valid() {
                    all_valid = false;
                }
                result.print();
            )+
            Ok::<_, $crate::OrmError>(all_valid)
        }
    }};
}

/// Assert that all models match the database schema, panic if not.
///
/// # Example
///
/// ```ignore
/// use pgorm::{assert_models_db_valid, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Panics if any model doesn't match the database
/// assert_models_db_valid!(pg, User, Order).await?;
/// ```
#[macro_export]
macro_rules! assert_models_db_valid {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            let mut errors: Vec<String> = Vec::new();
            $(
                let result = $crate::ModelCheckResult::check::<$model>(&db_schema);
                if !result.table_found {
                    errors.push(format!("{}: table '{}' not found", result.model, result.table));
                } else if !result.missing_in_db.is_empty() {
                    errors.push(format!("{}: missing columns {:?}", result.model, result.missing_in_db));
                }
            )+
            if !errors.is_empty() {
                panic!("Schema validation failed:\n  {}", errors.join("\n  "));
            }
            Ok::<_, $crate::OrmError>(())
        }
    }};
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

        #[test]
        fn test_check_sql_alias_and_ambiguous_column() {
            let mut registry = SchemaRegistry::new();
            registry.register::<TestUser>();
            registry.register::<TestOrder>();

            // Alias-qualified columns should resolve via FROM/JOIN alias mapping.
            let issues = registry.check_sql(
                "SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE o.status = 'paid'",
            );
            assert!(issues.is_empty());

            // Unqualified `id` is ambiguous across `users` and `orders`.
            let issues =
                registry.check_sql("SELECT id FROM users u JOIN orders o ON u.id = o.user_id");
            assert!(
                issues
                    .iter()
                    .any(|i| i.kind == SchemaIssueKind::AmbiguousColumn)
            );
        }

        #[test]
        fn test_check_sql_insert_update_on_conflict_columns() {
            let mut registry = SchemaRegistry::new();
            registry.register::<TestUser>();

            // INSERT column list should be validated against the target table.
            let issues = registry.check_sql("INSERT INTO users (id, missing_col) VALUES (1, 'x')");
            assert!(
                issues
                    .iter()
                    .any(|i| i.kind == SchemaIssueKind::MissingColumn)
            );

            // UPDATE SET column list should be validated against the target table.
            let issues = registry.check_sql("UPDATE users SET missing_col = 1 WHERE id = 1");
            assert!(
                issues
                    .iter()
                    .any(|i| i.kind == SchemaIssueKind::MissingColumn)
            );

            // ON CONFLICT inference / DO UPDATE SET columns should be validated too.
            let issues = registry.check_sql(
                "INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET missing_col = EXCLUDED.name",
            );
            assert!(
                issues
                    .iter()
                    .any(|i| i.kind == SchemaIssueKind::MissingColumn)
            );
        }

        #[test]
        fn test_check_sql_allows_system_columns() {
            let mut registry = SchemaRegistry::new();
            registry.register::<TestUser>();

            // System columns exist on every table (even if they aren't modeled).
            let issues = registry.check_sql("SELECT ctid FROM users");
            assert!(issues.is_empty());

            // Validate INSERT/UPDATE paths also skip system columns.
            let issues = registry.check_sql("INSERT INTO users (ctid) VALUES ('(0,0)')");
            assert!(issues.is_empty());

            let issues = registry.check_sql("UPDATE users SET ctid = ctid");
            assert!(issues.is_empty());
        }
    }
}
