use crate::client::CheckClient;
use crate::error::{CheckError, CheckResult};
use crate::schema_cache::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad};
use crate::schema_introspect::DbSchema;
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
    let parsed = pg_query::parse(sql).map_err(|e| {
        CheckError::Validation(format!("pg_query parse failed: {e}"))
    })?;

    let mut issues = Vec::<SqlCheckIssue>::new();

    let cte_names: std::collections::HashSet<String> = parsed.cte_names.into_iter().collect();

    // Resolve RangeVar -> (schema, table) and build visible qualifiers (alias OR bare table name).
    #[derive(Debug, Clone)]
    struct ResolvedTable {
        schema: String,
        table: String,
    }

    let mut visible_tables: Vec<ResolvedTable> = Vec::new();
    let mut qualifier_to_table: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();

    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::RangeVar(v) = node {
            if cte_names.contains(&v.relname) {
                continue;
            }

            let rel_schema = if v.schemaname.is_empty() {
                None
            } else {
                Some(v.schemaname.as_str())
            };
            let rel_name = v.relname.as_str();
            let qualifier = v
                .alias
                .as_ref()
                .map(|a| a.aliasname.as_str())
                .unwrap_or(rel_name)
                .to_string();

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
                        location: Some(v.location),
                    });
                }
                Err(msg) => issues.push(SqlCheckIssue {
                    level: SqlCheckLevel::Error,
                    kind: SqlCheckIssueKind::MissingTable,
                    message: msg,
                    location: Some(v.location),
                }),
            }
        }
    }

    // System columns exist on every table but are not exposed via our introspection query.
    let system_columns: std::collections::HashSet<&'static str> = [
        "ctid", "xmin", "xmax", "cmin", "cmax", "tableoid",
    ]
    .into_iter()
    .collect();

    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::ColumnRef(c) = node {
            let mut has_star = false;
            let mut parts: Vec<String> = Vec::new();
            for f in &c.fields {
                match f.node.as_ref() {
                    Some(pg_query::NodeEnum::String(s)) => parts.push(s.sval.to_string()),
                    Some(pg_query::NodeEnum::AStar(_)) => has_star = true,
                    _ => {}
                }
            }
            if has_star || parts.is_empty() {
                continue;
            }

            // Unqualified: col
            if parts.len() == 1 {
                let col = parts[0].as_str();
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
                        location: Some(c.location),
                    }),
                    1 => {}
                    _ => issues.push(SqlCheckIssue {
                        level: SqlCheckLevel::Error,
                        kind: SqlCheckIssueKind::AmbiguousColumn,
                        message: format!(
                            "Ambiguous column reference: {col} (found in multiple tables)"
                        ),
                        location: Some(c.location),
                    }),
                }
                continue;
            }

            // Qualified: qualifier.col
            if parts.len() == 2 {
                let qualifier = parts[0].as_str();
                let col = parts[1].as_str();

                if system_columns.contains(col) {
                    continue;
                }

                if let Some((resolved_schema, resolved_table)) = qualifier_to_table.get(qualifier) {
                    if !table_has_column(schema, resolved_schema, resolved_table, col) {
                        issues.push(SqlCheckIssue {
                            level: SqlCheckLevel::Error,
                            kind: SqlCheckIssueKind::MissingColumn,
                            message: format!(
                                "Column not found: {qualifier}.{col} (table resolved to {}.{})",
                                resolved_schema, resolved_table
                            ),
                            location: Some(c.location),
                        });
                    }
                } else {
                    issues.push(SqlCheckIssue {
                        level: SqlCheckLevel::Error,
                        kind: SqlCheckIssueKind::MissingTable,
                        message: format!("Unknown table/alias qualifier: {qualifier}"),
                        location: Some(c.location),
                    });
                }

                continue;
            }

            // schema.table.col OR catalog.schema.table.col
            if parts.len() == 3 || parts.len() == 4 {
                let (schema_part, table_part, col_part) = if parts.len() == 3 {
                    (&parts[0], &parts[1], &parts[2])
                } else {
                    (&parts[1], &parts[2], &parts[3])
                };

                if system_columns.contains(col_part.as_str()) {
                    continue;
                }

                if schema.find_table(schema_part, table_part).is_none() {
                    issues.push(SqlCheckIssue {
                        level: SqlCheckLevel::Error,
                        kind: SqlCheckIssueKind::MissingTable,
                        message: format!("Table not found: {schema_part}.{table_part}"),
                        location: Some(c.location),
                    });
                    continue;
                }

                if !table_has_column(schema, schema_part, table_part, col_part) {
                    issues.push(SqlCheckIssue {
                        level: SqlCheckLevel::Error,
                        kind: SqlCheckIssueKind::MissingColumn,
                        message: format!(
                            "Column not found: {schema_part}.{table_part}.{col_part}"
                        ),
                        location: Some(c.location),
                    });
                }

                continue;
            }

            issues.push(SqlCheckIssue {
                level: SqlCheckLevel::Warning,
                kind: SqlCheckIssueKind::Unsupported,
                message: format!(
                    "Unsupported column reference form ({} parts): {}",
                    parts.len(),
                    parts.join(".")
                ),
                location: Some(c.location),
            });
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
