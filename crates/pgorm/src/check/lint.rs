use super::registry::{
    SchemaIssue, SchemaIssueKind, SchemaIssueLevel, SchemaRegistry, TableSchema,
};

/// PostgreSQL system columns that exist on every table but are not modeled in schema metadata.
///
/// See: <https://www.postgresql.org/docs/current/ddl-system-columns.html>
const SYSTEM_COLUMNS: &[&str] = &["ctid", "xmin", "xmax", "cmin", "cmax", "tableoid"];

/// Check whether a column name is a PostgreSQL system column.
fn is_system_column(col: &str) -> bool {
    SYSTEM_COLUMNS.contains(&col)
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
    // Lint code constants
    LINT_E001,
    LINT_E002,
    LINT_E003,
    LINT_E004,
    LINT_I001,
    LINT_W001,
    LINT_W002,
    LINT_W003,
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

        // Build a map of qualifier -> table using RangeVar + alias info.
        let mut qualifier_to_table: std::collections::HashMap<&str, &TableSchema> =
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
                    if qualifier_to_table.insert(qualifier, t).is_none() {
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
                } else if analysis.cte_names.contains(qualifier) {
                    // We don't track CTE column sets. Treat CTE qualifiers as valid and skip.
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
