use crate::codegen::GeneratedFile;
use crate::config::{ModelsConfig, ModelsDialect, ProjectConfig};
use crate::type_mapper::TypeMapper;
use heck::ToUpperCamelCase;
use pgorm_check::{DbSchema, RelationKind};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferredBelongsTo {
    model_path: String,
    foreign_key: String,
    method_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferredHasMany {
    model_path: String,
    foreign_key: String,
    method_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InferredTableRelations {
    belongs_to: Vec<InferredBelongsTo>,
    has_many: Vec<InferredHasMany>,
}

pub fn generate_models(
    project: &ProjectConfig,
    cfg: &ModelsConfig,
    schema: &DbSchema,
) -> anyhow::Result<Vec<GeneratedFile>> {
    match cfg.dialect {
        ModelsDialect::Pgorm => generate_pgorm_models(project, cfg, schema),
    }
}

fn generate_pgorm_models(
    project: &ProjectConfig,
    cfg: &ModelsConfig,
    schema: &DbSchema,
) -> anyhow::Result<Vec<GeneratedFile>> {
    let out_dir = project.resolve_path(&cfg.out);
    let type_mapper = TypeMapper::new(cfg.types.clone());

    let selected_tables = select_tables(cfg, schema)?;

    let mut table_name_counts: HashMap<&str, usize> = HashMap::new();
    for t in &selected_tables {
        *table_name_counts.entry(t.name.as_str()).or_insert(0) += 1;
    }

    let mut table_struct_names: HashMap<String, String> = HashMap::new();
    let mut table_primary_keys: HashMap<String, Option<String>> = HashMap::new();
    for t in &selected_tables {
        let needs_schema_prefix = table_name_counts.get(t.name.as_str()).copied().unwrap_or(0) > 1;
        let struct_name = sanitize_type_ident(&struct_name_for_table(cfg, t, needs_schema_prefix));
        table_struct_names.insert(table_key(t), struct_name);
        table_primary_keys.insert(table_key(t), primary_key_for_table(cfg, t));
    }

    let inferred_relations =
        infer_relations(&selected_tables, &table_struct_names, &table_primary_keys);

    let qualify_table_names = schema.schemas.len() > 1;

    let mut files: Vec<GeneratedFile> = Vec::new();

    // mod.rs
    let mod_rs = generate_models_mod_rs(cfg, &selected_tables, &table_name_counts)?;
    files.push(GeneratedFile {
        path: out_dir.join("mod.rs"),
        content: mod_rs,
    });

    // per-table module
    for t in &selected_tables {
        let needs_schema_prefix = table_name_counts.get(t.name.as_str()).copied().unwrap_or(0) > 1;
        let module_base = if needs_schema_prefix {
            format!("{}_{}", t.schema, t.name)
        } else {
            t.name.clone()
        };
        let module_ident = sanitize_module_ident(&module_base);
        let module_file = format!("{}.rs", module_file_stem(&module_ident));

        let struct_name = struct_name_for_table(cfg, t, needs_schema_prefix);
        let struct_name = sanitize_type_ident(&struct_name);

        let table_attr_value = if qualify_table_names {
            format!("{}.{}", t.schema, t.name)
        } else {
            t.name.clone()
        };

        let pk_col = table_primary_keys
            .get(&table_key(t))
            .cloned()
            .unwrap_or(None);
        if let Some(pk) = pk_col.as_deref() {
            if !t.columns.iter().any(|c| c.name == pk) {
                anyhow::bail!("primary key column not found: {}.{}.{pk}", t.schema, t.name);
            }
        }

        let inferred = inferred_relations.get(&table_key(t));

        let module_rs = generate_table_module_rs(
            cfg,
            &type_mapper,
            t,
            &struct_name,
            &table_attr_value,
            pk_col.as_deref(),
            inferred,
            is_table_like(t.kind),
        )?;

        files.push(GeneratedFile {
            path: out_dir.join(module_file),
            content: module_rs,
        });
    }

    Ok(files)
}

fn table_key(t: &pgorm_check::TableInfo) -> String {
    format!("{}.{}", t.schema, t.name)
}

fn pluralize_candidate(base: &str) -> String {
    if base.ends_with('y')
        && base.len() > 1
        && !matches!(
            base.chars().nth(base.len() - 2),
            Some('a' | 'e' | 'i' | 'o' | 'u')
        )
    {
        format!("{}ies", &base[..base.len() - 1])
    } else if base.ends_with('s')
        || base.ends_with('x')
        || base.ends_with('z')
        || base.ends_with("ch")
        || base.ends_with("sh")
    {
        format!("{base}es")
    } else {
        format!("{base}s")
    }
}

fn infer_relations(
    tables: &[pgorm_check::TableInfo],
    table_struct_names: &HashMap<String, String>,
    table_primary_keys: &HashMap<String, Option<String>>,
) -> HashMap<String, InferredTableRelations> {
    let mut out: HashMap<String, InferredTableRelations> = HashMap::new();

    for child in tables {
        for c in &child.columns {
            let Some(base) = c.name.strip_suffix("_id") else {
                continue;
            };
            if base.is_empty() {
                continue;
            }

            let plural = pluralize_candidate(base);

            // Prefer same-schema target tables to reduce false positives.
            let mut candidates: Vec<&pgorm_check::TableInfo> = tables
                .iter()
                .filter(|t| t.schema == child.schema && (t.name == base || t.name == plural))
                .collect();
            if candidates.len() != 1 {
                candidates = tables
                    .iter()
                    .filter(|t| t.name == base || t.name == plural)
                    .collect();
            }
            if candidates.len() != 1 {
                continue;
            }
            let parent = candidates[0];
            let parent_key = table_key(parent);

            // Heuristic safety: only infer belongs_to for `id` primary key targets.
            if table_primary_keys
                .get(&parent_key)
                .and_then(|v| v.as_deref())
                != Some("id")
            {
                continue;
            }

            let Some(parent_struct) = table_struct_names.get(&parent_key) else {
                continue;
            };
            let child_key = table_key(child);
            let Some(child_struct) = table_struct_names.get(&child_key) else {
                continue;
            };

            let child_rel = out.entry(child_key.clone()).or_default();
            let belongs_to = InferredBelongsTo {
                model_path: format!("super::{parent_struct}"),
                foreign_key: c.name.clone(),
                method_name: base.to_string(),
            };
            if !child_rel.belongs_to.iter().any(|x| x == &belongs_to) {
                child_rel.belongs_to.push(belongs_to);
            }

            let parent_rel = out.entry(parent_key).or_default();
            let has_many = InferredHasMany {
                model_path: format!("super::{child_struct}"),
                foreign_key: c.name.clone(),
                method_name: sanitize_field_ident(&child.name),
            };
            if !parent_rel.has_many.iter().any(|x| x == &has_many) {
                parent_rel.has_many.push(has_many);
            }
        }
    }

    out
}

fn select_tables(
    cfg: &ModelsConfig,
    schema: &DbSchema,
) -> anyhow::Result<Vec<pgorm_check::TableInfo>> {
    let mut tables: Vec<pgorm_check::TableInfo> = Vec::new();

    if cfg.tables.is_empty() {
        for t in &schema.tables {
            if !cfg.include_views && !is_table_like(t.kind) {
                continue;
            }
            if cfg.include_views && t.kind == RelationKind::Other {
                continue;
            }
            tables.push(t.clone());
        }
    } else {
        for item in &cfg.tables {
            let (schema_name, table_name) = resolve_table_ref(schema, item)?;
            let Some(t) = schema.find_table(&schema_name, &table_name) else {
                anyhow::bail!("table not found: {schema_name}.{table_name}");
            };
            if !cfg.include_views && !is_table_like(t.kind) {
                anyhow::bail!(
                    "relation is not a table ({schema_name}.{table_name}) - set models.include_views=true to include views"
                );
            }
            tables.push(t.clone());
        }
    }

    tables.sort_by(|a, b| {
        (a.schema.as_str(), a.name.as_str()).cmp(&(b.schema.as_str(), b.name.as_str()))
    });
    tables.dedup_by(|a, b| a.schema == b.schema && a.name == b.name);

    Ok(tables)
}

fn is_table_like(kind: RelationKind) -> bool {
    matches!(kind, RelationKind::Table | RelationKind::PartitionedTable)
}

fn resolve_table_ref(schema: &DbSchema, table_ref: &str) -> anyhow::Result<(String, String)> {
    let table_ref = table_ref.trim();
    if table_ref.is_empty() {
        anyhow::bail!("empty table reference");
    }

    // schema.table
    if let Some((s, t)) = table_ref.split_once('.') {
        let s = s.trim();
        let t = t.trim();
        if s.is_empty() || t.is_empty() {
            anyhow::bail!("invalid table reference: {table_ref}");
        }
        if schema.find_table(s, t).is_none() {
            anyhow::bail!("table not found: {s}.{t}");
        }
        return Ok((s.to_string(), t.to_string()));
    }

    // unqualified: search in configured schemas
    let mut found: Option<(String, String)> = None;
    for s in &schema.schemas {
        if schema.find_table(s, table_ref).is_some() {
            if found.is_some() {
                anyhow::bail!("table name is ambiguous in configured schemas: {table_ref}");
            }
            found = Some((s.to_string(), table_ref.to_string()));
        }
    }

    found.ok_or_else(|| anyhow::anyhow!("table not found: {table_ref}"))
}

fn generate_models_mod_rs(
    cfg: &ModelsConfig,
    tables: &[pgorm_check::TableInfo],
    table_name_counts: &HashMap<&str, usize>,
) -> anyhow::Result<String> {
    let mut out = String::new();
    out.push_str("// @generated by pgorm (pgorm-cli)\n\n");

    let mut seen_modules: HashSet<String> = HashSet::new();
    let mut seen_structs: HashSet<String> = HashSet::new();
    let mut module_lines: Vec<(String, String, String)> = Vec::new(); // (module_ident, module_path, struct_name)

    for t in tables {
        let needs_schema_prefix = table_name_counts.get(t.name.as_str()).copied().unwrap_or(0) > 1;
        let module_base = if needs_schema_prefix {
            format!("{}_{}", t.schema, t.name)
        } else {
            t.name.clone()
        };
        let module_ident = sanitize_module_ident(&module_base);
        let module_path = module_ident.clone();

        if !seen_modules.insert(module_path.clone()) {
            anyhow::bail!("duplicate module name after sanitization: {module_path}");
        }

        let struct_name = sanitize_type_ident(&struct_name_for_table(cfg, t, needs_schema_prefix));
        if !seen_structs.insert(struct_name.clone()) {
            anyhow::bail!(
                "duplicate model struct name after sanitization: {struct_name} (table: {}.{})",
                t.schema,
                t.name
            );
        }
        module_lines.push((module_ident, module_path, struct_name));
    }

    module_lines.sort_by(|a, b| a.1.cmp(&b.1));
    for (module_ident, _module_path, _struct_name) in &module_lines {
        out.push_str(&format!("pub mod {module_ident};\n"));
    }

    out.push('\n');
    for (module_ident, _module_path, struct_name) in &module_lines {
        out.push_str(&format!("pub use {module_ident}::{struct_name};\n"));
    }

    Ok(out)
}

fn struct_name_for_table(
    cfg: &ModelsConfig,
    t: &pgorm_check::TableInfo,
    needs_schema_prefix: bool,
) -> String {
    let key = format!("{}.{}", t.schema, t.name);
    if let Some(v) = cfg.rename.get(&key) {
        return v.clone();
    }
    if let Some(v) = cfg.rename.get(t.name.as_str()) {
        return v.clone();
    }

    let base = t.name.to_upper_camel_case();
    if needs_schema_prefix {
        format!("{}{}", t.schema.to_upper_camel_case(), base)
    } else {
        base
    }
}

fn primary_key_for_table(cfg: &ModelsConfig, t: &pgorm_check::TableInfo) -> Option<String> {
    let key = format!("{}.{}", t.schema, t.name);
    if let Some(v) = cfg.primary_key.get(&key) {
        return Some(v.clone());
    }
    if let Some(v) = cfg.primary_key.get(t.name.as_str()) {
        return Some(v.clone());
    }

    if t.columns.iter().any(|c| c.name == "id") {
        return Some("id".to_string());
    }

    None
}

fn generate_table_module_rs(
    cfg: &ModelsConfig,
    type_mapper: &TypeMapper,
    t: &pgorm_check::TableInfo,
    struct_name: &str,
    table_attr_value: &str,
    pk_col: Option<&str>,
    inferred_rel: Option<&InferredTableRelations>,
    emit_write_models: bool,
) -> anyhow::Result<String> {
    let mut out = String::new();
    out.push_str("// @generated by pgorm (pgorm-cli)\n\n");

    let mut uses = cfg.extra_uses.clone();
    uses.sort();
    uses.dedup();
    for u in uses {
        out.push_str(&format!("use {u};\n"));
    }
    if !cfg.extra_uses.is_empty() {
        out.push('\n');
    }

    if !cfg.derives.is_empty() {
        out.push_str("#[derive(");
        out.push_str(
            &cfg.derives
                .iter()
                .map(|d| render_derive(cfg.dialect, d))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(")]\n");
    }

    if let Some(rel) = inferred_rel {
        let mut belongs_to = rel.belongs_to.clone();
        belongs_to.sort_by(|a, b| {
            (&a.model_path, &a.foreign_key, &a.method_name).cmp(&(
                &b.model_path,
                &b.foreign_key,
                &b.method_name,
            ))
        });
        for r in belongs_to {
            out.push_str(&format!(
                "#[orm(belongs_to({}, foreign_key = \"{}\", as = \"{}\"))]\n",
                r.model_path, r.foreign_key, r.method_name
            ));
        }

        let mut has_many = rel.has_many.clone();
        has_many.sort_by(|a, b| {
            (&a.model_path, &a.foreign_key, &a.method_name).cmp(&(
                &b.model_path,
                &b.foreign_key,
                &b.method_name,
            ))
        });
        for r in has_many {
            out.push_str(&format!(
                "#[orm(has_many({}, foreign_key = \"{}\", as = \"{}\"))]\n",
                r.model_path, r.foreign_key, r.method_name
            ));
        }
    }

    out.push_str(&format!("#[orm(table = \"{table_attr_value}\")]\n"));
    out.push_str(&format!("pub struct {struct_name} {{\n"));

    let mut seen_fields: HashSet<String> = HashSet::new();
    let mut writable_fields: Vec<(String, String, String)> = Vec::new(); // (field_ident, ty, column_name)
    for c in &t.columns {
        let field_ident = sanitize_field_ident(&c.name);
        if !seen_fields.insert(field_ident.clone()) {
            anyhow::bail!(
                "duplicate field name after sanitization in {}.{}: {field_ident}",
                t.schema,
                t.name
            );
        }

        let ty = column_rust_type(type_mapper, c);

        let mut orm_parts: Vec<String> = Vec::new();
        if pk_col.is_some_and(|pk| pk == c.name) {
            orm_parts.push("id".to_string());
        } else {
            writable_fields.push((field_ident.clone(), ty.clone(), c.name.clone()));
        }

        let field_ident_for_compare = field_ident.trim_start_matches("r#");
        if field_ident_for_compare != c.name {
            orm_parts.push(format!("column = \"{}\"", c.name));
        }

        if !orm_parts.is_empty() {
            out.push_str(&format!("    #[orm({})]\n", orm_parts.join(", ")));
        }
        out.push_str(&format!("    pub {field_ident}: {ty},\n"));
    }

    out.push_str("}\n");

    if emit_write_models && !writable_fields.is_empty() {
        let insert_name = format!("New{struct_name}");
        let update_name = format!("{struct_name}Patch");

        out.push('\n');
        out.push_str("#[derive(Debug, Clone, pgorm::InsertModel)]\n");
        out.push_str(&format!(
            "#[orm(table = \"{table_attr_value}\", returning = \"{struct_name}\")]\n"
        ));
        out.push_str(&format!("pub struct {insert_name} {{\n"));
        for (field_ident, ty, column_name) in &writable_fields {
            let field_ident_for_compare = field_ident.trim_start_matches("r#");
            if field_ident_for_compare != column_name {
                out.push_str(&format!("    #[orm(column = \"{}\")]\n", column_name));
            }
            out.push_str(&format!("    pub {field_ident}: {ty},\n"));
        }
        out.push_str("}\n");

        out.push('\n');
        out.push_str("#[derive(Debug, Clone, pgorm::UpdateModel)]\n");
        out.push_str(&format!(
            "#[orm(table = \"{table_attr_value}\", model = \"{struct_name}\", returning = \"{struct_name}\")]\n"
        ));
        out.push_str(&format!("pub struct {update_name} {{\n"));
        for (field_ident, ty, column_name) in &writable_fields {
            let field_ident_for_compare = field_ident.trim_start_matches("r#");
            if field_ident_for_compare != column_name {
                out.push_str(&format!("    #[orm(column = \"{}\")]\n", column_name));
            }
            out.push_str(&format!("    pub {field_ident}: Option<{ty}>,\n"));
        }
        out.push_str("}\n");
    }

    Ok(out)
}

fn column_rust_type(type_mapper: &TypeMapper, c: &pgorm_check::ColumnInfo) -> String {
    let mut ty = type_mapper.map(&c.data_type);
    if !c.not_null && !ty.starts_with("Option<") {
        ty = format!("Option<{ty}>");
    }
    ty
}

fn render_derive(dialect: ModelsDialect, derive: &str) -> String {
    match dialect {
        ModelsDialect::Pgorm => match derive {
            "FromRow" | "Model" | "ViewModel" | "InsertModel" | "UpdateModel" => {
                format!("pgorm::{derive}")
            }
            _ => derive.to_string(),
        },
    }
}

fn sanitize_type_ident(name: &str) -> String {
    let mut s = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();

    if s.is_empty() {
        s.push('_');
    }

    if s.chars().next().unwrap().is_ascii_digit() {
        s.insert(0, '_');
    }

    s
}

fn sanitize_module_ident(name: &str) -> String {
    sanitize_field_ident(name)
}

fn module_file_stem(module_ident: &str) -> &str {
    module_ident.trim_start_matches("r#")
}

fn sanitize_field_ident(column: &str) -> String {
    let mut s = column
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    s = heck::ToSnakeCase::to_snake_case(s.as_str());
    if s.is_empty() {
        s.push('_');
    }
    if s.chars().next().unwrap().is_ascii_digit() {
        s.insert(0, '_');
    }
    if is_rust_keyword(&s) {
        format!("r#{s}")
    } else {
        s
    }
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModelsDialect;

    fn test_schema() -> DbSchema {
        DbSchema {
            schemas: vec!["public".to_string()],
            tables: vec![pgorm_check::TableInfo {
                schema: "public".to_string(),
                name: "users".to_string(),
                kind: RelationKind::Table,
                columns: vec![
                    pgorm_check::ColumnInfo {
                        name: "id".to_string(),
                        data_type: "bigint".to_string(),
                        not_null: true,
                        default_expr: None,
                        ordinal: 1,
                    },
                    pgorm_check::ColumnInfo {
                        name: "type".to_string(),
                        data_type: "text".to_string(),
                        not_null: false,
                        default_expr: None,
                        ordinal: 2,
                    },
                ],
            }],
        }
    }

    fn test_schema_with_relations() -> DbSchema {
        DbSchema {
            schemas: vec!["public".to_string()],
            tables: vec![
                pgorm_check::TableInfo {
                    schema: "public".to_string(),
                    name: "users".to_string(),
                    kind: RelationKind::Table,
                    columns: vec![
                        pgorm_check::ColumnInfo {
                            name: "id".to_string(),
                            data_type: "bigint".to_string(),
                            not_null: true,
                            default_expr: None,
                            ordinal: 1,
                        },
                        pgorm_check::ColumnInfo {
                            name: "email".to_string(),
                            data_type: "text".to_string(),
                            not_null: true,
                            default_expr: None,
                            ordinal: 2,
                        },
                    ],
                },
                pgorm_check::TableInfo {
                    schema: "public".to_string(),
                    name: "posts".to_string(),
                    kind: RelationKind::Table,
                    columns: vec![
                        pgorm_check::ColumnInfo {
                            name: "id".to_string(),
                            data_type: "bigint".to_string(),
                            not_null: true,
                            default_expr: None,
                            ordinal: 1,
                        },
                        pgorm_check::ColumnInfo {
                            name: "user_id".to_string(),
                            data_type: "bigint".to_string(),
                            not_null: true,
                            default_expr: None,
                            ordinal: 2,
                        },
                        pgorm_check::ColumnInfo {
                            name: "title".to_string(),
                            data_type: "text".to_string(),
                            not_null: true,
                            default_expr: None,
                            ordinal: 3,
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn generates_pgorm_model_with_id_and_option() {
        let cfg = ModelsConfig {
            out: "src/models".to_string(),
            dialect: ModelsDialect::Pgorm,
            include_views: false,
            tables: Vec::new(),
            rename: Default::default(),
            primary_key: Default::default(),
            types: Default::default(),
            derives: vec![
                "Debug".to_string(),
                "Clone".to_string(),
                "FromRow".to_string(),
                "Model".to_string(),
            ],
            extra_uses: Vec::new(),
        };

        let schema = test_schema();
        let project = ProjectConfig {
            config_path: "pgorm.toml".into(),
            config_dir: ".".into(),
            file: crate::config::ConfigFile {
                version: "1".to_string(),
                engine: Some("postgres".to_string()),
                database: crate::config::DatabaseConfig {
                    url: "postgres://".to_string(),
                    schemas: vec!["public".to_string()],
                },
                schema_cache: crate::config::SchemaCacheConfig::default(),
                models: None,
                packages: Vec::new(),
            },
        };

        let files = generate_models(&project, &cfg, &schema).unwrap();
        let users = files
            .iter()
            .find(|f| f.path.ends_with("users.rs"))
            .unwrap()
            .content
            .clone();

        assert!(users.contains("#[derive(Debug, Clone, pgorm::FromRow, pgorm::Model)]"));
        assert!(users.contains("#[orm(table = \"users\")]"));
        assert!(users.contains("#[orm(id)]"));
        assert!(users.contains("pub r#type: Option<String>"));
        assert!(users.contains("pub struct NewUsers"));
        assert!(users.contains("pub struct UsersPatch"));
    }

    #[test]
    fn generates_inferred_relations_and_write_models() {
        let cfg = ModelsConfig {
            out: "src/models".to_string(),
            dialect: ModelsDialect::Pgorm,
            include_views: false,
            tables: Vec::new(),
            rename: Default::default(),
            primary_key: Default::default(),
            types: Default::default(),
            derives: vec![
                "Debug".to_string(),
                "Clone".to_string(),
                "FromRow".to_string(),
                "Model".to_string(),
            ],
            extra_uses: Vec::new(),
        };

        let schema = test_schema_with_relations();
        let project = ProjectConfig {
            config_path: "pgorm.toml".into(),
            config_dir: ".".into(),
            file: crate::config::ConfigFile {
                version: "1".to_string(),
                engine: Some("postgres".to_string()),
                database: crate::config::DatabaseConfig {
                    url: "postgres://".to_string(),
                    schemas: vec!["public".to_string()],
                },
                schema_cache: crate::config::SchemaCacheConfig::default(),
                models: None,
                packages: Vec::new(),
            },
        };

        let files = generate_models(&project, &cfg, &schema).unwrap();

        let users = files
            .iter()
            .find(|f| f.path.ends_with("users.rs"))
            .unwrap()
            .content
            .clone();
        let posts = files
            .iter()
            .find(|f| f.path.ends_with("posts.rs"))
            .unwrap()
            .content
            .clone();

        assert!(
            users.contains(
                "#[orm(has_many(super::Posts, foreign_key = \"user_id\", as = \"posts\"))]"
            )
        );
        assert!(posts.contains(
            "#[orm(belongs_to(super::Users, foreign_key = \"user_id\", as = \"user\"))]"
        ));
        assert!(users.contains("pub struct NewUsers"));
        assert!(users.contains("pub struct UsersPatch"));
        assert!(posts.contains("pub struct NewPosts"));
        assert!(posts.contains("pub struct PostsPatch"));
    }
}
