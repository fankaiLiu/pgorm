use crate::client::CheckClient;
use crate::error::{CheckError, CheckResult};
use crate::schema_cache::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad};
use crate::schema_introspect::DbSchema;
use crate::sql_analysis::analyze_sql;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlCheckLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlCheckIssueKind {
    ParseError,
    MissingTable,
    MissingColumn,
    AmbiguousColumn,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlCheckIssue {
    pub level: SqlCheckLevel,
    pub kind: SqlCheckIssueKind,
    pub message: String,
    /// Byte offset in the SQL string (as reported by Postgres parser), if available.
    pub location: Option<i32>,
}

pub fn check_sql(schema: &DbSchema, sql: &str) -> CheckResult<Vec<SqlCheckIssue>> {
    let analysis = analyze_sql(sql);
    check_sql_analysis(schema, &analysis)
}

pub fn check_sql_analysis(
    schema: &DbSchema,
    analysis: &crate::sql_analysis::SqlAnalysis,
) -> CheckResult<Vec<SqlCheckIssue>> {
    if !analysis.parse_result.valid {
        return Err(CheckError::Validation(format!(
            "pg_query parse failed: {}",
            analysis.parse_result.error.clone().unwrap_or_default()
        )));
    }

    let mut issues = Vec::<SqlCheckIssue>::new();

    let cte_names = &analysis.cte_names;

    // Resolve RangeVar -> (schema, table) and build visible qualifiers (alias OR bare table name).
    #[derive(Debug, Clone)]
    struct ResolvedTable {
        schema: String,
        table: String,
    }

    let mut visible_tables: Vec<ResolvedTable> = Vec::new();
    let mut qualifier_to_table: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();

    for rv in &analysis.range_vars {
        if cte_names.contains(&rv.table) {
            continue;
        }

        let rel_schema = rv.schema.as_deref();
        let rel_name = rv.table.as_str();
        let qualifier = rv.alias.as_deref().unwrap_or(rel_name).to_string();

        match resolve_table(schema, rel_schema, rel_name) {
            Ok(Some((resolved_schema, resolved_table))) => {
                // If an alias exists, the base name is not visible.
                if qualifier_to_table
                    .insert(
                        qualifier.clone(),
                        (resolved_schema.clone(), resolved_table.clone()),
                    )
                    .is_none()
                {
                    visible_tables.push(ResolvedTable {
                        schema: resolved_schema,
                        table: resolved_table,
                    });
                }
            }
            Ok(None) => {
                let name = match rel_schema {
                    Some(s) => format!("{s}.{rel_name}"),
                    None => rel_name.to_string(),
                };
                issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: format!("Table not found: {name}"),
                    location: rv.location,
                });
            }
            Err(msg) => issues.push(SqlCheckIssue {
                level: SqlCheckLevel::Error,
                kind: SqlCheckIssueKind::MissingTable,
                message: msg,
                location: rv.location,
            }),
        }
    }

    // System columns exist on every table but are not exposed via our introspection query.
    let system_columns: std::collections::HashSet<&'static str> =
        ["ctid", "xmin", "xmax", "cmin", "cmax", "tableoid"]
            .into_iter()
            .collect();

    for c in &analysis.column_refs {
        if c.has_star || c.parts.is_empty() {
            continue;
        }

        // Unqualified: col
        if c.parts.len() == 1 {
            let col = c.parts[0].as_str();
            if system_columns.contains(col) {
                continue;
            }

            let mut matches = 0usize;
            for t in &visible_tables {
                if table_has_column(schema, &t.schema, &t.table, col) {
                    matches += 1;
                }
            }

            match matches {
                0 => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingColumn,
                    message: format!("Column not found: {col}"),
                    location: c.location,
                }),
                1 => {}
                _ => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::AmbiguousColumn,
                    message: format!(
                        "Ambiguous column reference: {col} (found in multiple tables)"
                    ),
                    location: c.location,
                }),
            }
            continue;
        }

        // Qualified: qualifier.col
        if c.parts.len() == 2 {
            let qualifier = c.parts[0].as_str();
            let col = c.parts[1].as_str();

            if system_columns.contains(col) {
                continue;
            }

            if let Some((resolved_schema, resolved_table)) = qualifier_to_table.get(qualifier) {
                if !table_has_column(schema, resolved_schema, resolved_table, col) {
                    issues.push(SqlCheckIssue {
                        level: SqlCheckLevel::Error,
                        kind: SqlCheckIssueKind::MissingColumn,
                        message: format!(
                            "Column not found: {qualifier}.{col} (table resolved to {resolved_schema}.{resolved_table})"
                        ),
                        location: c.location,
                    });
                }
            } else if cte_names.contains(qualifier) {
                // We don't track CTE column sets. Treat CTE qualifiers as valid and skip.
            } else {
                issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: format!("Unknown table/alias qualifier: {qualifier}"),
                    location: c.location,
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

            if system_columns.contains(col_part.as_str()) {
                continue;
            }

            if schema.find_table(schema_part, table_part).is_none() {
                issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: format!("Table not found: {schema_part}.{table_part}"),
                    location: c.location,
                });
                continue;
            }

            if !table_has_column(schema, schema_part, table_part, col_part) {
                issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingColumn,
                    message: format!("Column not found: {schema_part}.{table_part}.{col_part}"),
                    location: c.location,
                });
            }

            continue;
        }

        issues.push(SqlCheckIssue {
            level: SqlCheckLevel::Warning,
            kind: SqlCheckIssueKind::Unsupported,
            message: format!(
                "Unsupported column reference form ({} parts): {}",
                c.parts.len(),
                c.parts.join(".")
            ),
            location: c.location,
        });
    }

    // INSERT/UPDATE/ON CONFLICT target column validation.
    if let Some(insert) = &analysis.insert {
        if let Some(target) = &insert.target {
            let resolved = resolve_table(schema, target.schema.as_deref(), &target.table);
            match resolved {
                Ok(Some((s, t))) => {
                    for col in &insert.columns {
                        if system_columns.contains(col.name.as_str()) {
                            continue;
                        }
                        if !table_has_column(schema, &s, &t, &col.name) {
                            issues.push(SqlCheckIssue {
                                level: SqlCheckLevel::Error,
                                kind: SqlCheckIssueKind::MissingColumn,
                                message: format!(
                                    "Column not found: {s}.{t}.{} (INSERT target)",
                                    col.name
                                ),
                                location: col.location,
                            });
                        }
                    }

                    if let Some(oc) = &insert.on_conflict {
                        if oc.has_inference_expressions {
                            issues.push(SqlCheckIssue {
                                level: SqlCheckLevel::Warning,
                                kind: SqlCheckIssueKind::Unsupported,
                                message: "ON CONFLICT inference uses expressions; only simple column targets are checked".to_string(),
                                location: None,
                            });
                        }

                        for col in &oc.inference_columns {
                            if system_columns.contains(col.name.as_str()) {
                                continue;
                            }
                            if !table_has_column(schema, &s, &t, &col.name) {
                                issues.push(SqlCheckIssue {
                                    level: SqlCheckLevel::Error,
                                    kind: SqlCheckIssueKind::MissingColumn,
                                    message: format!(
                                        "Column not found: {s}.{t}.{} (ON CONFLICT target)",
                                        col.name
                                    ),
                                    location: col.location,
                                });
                            }
                        }

                        for col in &oc.update_set_columns {
                            if system_columns.contains(col.name.as_str()) {
                                continue;
                            }
                            if !table_has_column(schema, &s, &t, &col.name) {
                                issues.push(SqlCheckIssue {
                                    level: SqlCheckLevel::Error,
                                    kind: SqlCheckIssueKind::MissingColumn,
                                    message: format!(
                                        "Column not found: {s}.{t}.{} (ON CONFLICT DO UPDATE SET)",
                                        col.name
                                    ),
                                    location: col.location,
                                });
                            }
                        }
                    }
                }
                Ok(None) => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: format!("Table not found: {}", target.table),
                    location: target.location,
                }),
                Err(msg) => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: msg,
                    location: target.location,
                }),
            }
        }
    }

    if let Some(update) = &analysis.update {
        if let Some(target) = &update.target {
            let resolved = resolve_table(schema, target.schema.as_deref(), &target.table);
            match resolved {
                Ok(Some((s, t))) => {
                    for col in &update.set_columns {
                        if system_columns.contains(col.name.as_str()) {
                            continue;
                        }
                        if !table_has_column(schema, &s, &t, &col.name) {
                            issues.push(SqlCheckIssue {
                                level: SqlCheckLevel::Error,
                                kind: SqlCheckIssueKind::MissingColumn,
                                message: format!(
                                    "Column not found: {s}.{t}.{} (UPDATE target)",
                                    col.name
                                ),
                                location: col.location,
                            });
                        }
                    }
                }
                Ok(None) => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: format!("Table not found: {}", target.table),
                    location: target.location,
                }),
                Err(msg) => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: msg,
                    location: target.location,
                }),
            }
        }
    }

    Ok(issues)
}

pub async fn check_sql_cached<C: CheckClient>(
    client: &C,
    config: &SchemaCacheConfig,
    sql: &str,
) -> CheckResult<(SchemaCacheLoad, Vec<SqlCheckIssue>)> {
    let (cache, load) = SchemaCache::load_or_refresh(client, config).await?;
    let issues = check_sql(&cache.schema, sql)?;
    Ok((load, issues))
}

fn resolve_table(
    schema: &DbSchema,
    explicit_schema: Option<&str>,
    table: &str,
) -> Result<Option<(String, String)>, String> {
    if let Some(s) = explicit_schema {
        return Ok(schema
            .find_table(s, table)
            .map(|_| (s.to_string(), table.to_string())));
    }

    let mut found: Option<(String, String)> = None;
    for s in &schema.schemas {
        if schema.find_table(s, table).is_some() {
            if found.is_some() {
                return Err(format!(
                    "Table name is ambiguous in configured schemas: {table}"
                ));
            }
            found = Some((s.to_string(), table.to_string()));
        }
    }

    Ok(found)
}

fn table_has_column(schema: &DbSchema, table_schema: &str, table: &str, column: &str) -> bool {
    let Some(t) = schema.find_table(table_schema, table) else {
        return false;
    };
    t.columns.iter().any(|c| c.name == column)
}
