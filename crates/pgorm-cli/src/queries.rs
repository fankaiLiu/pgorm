use crate::config::{PackageConfig, ProjectConfig};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    One,
    Opt,
    Many,
    Exec,
    ExecRows,
}

impl QueryKind {
    pub fn from_sqlc_annotation(s: &str) -> anyhow::Result<Self> {
        match s {
            "one" => Ok(Self::One),
            "opt" => Ok(Self::Opt),
            "many" => Ok(Self::Many),
            "exec" => Ok(Self::Exec),
            "execrows" => Ok(Self::ExecRows),
            _ => anyhow::bail!("unknown query kind: :{s}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryDef {
    pub name: String,
    pub kind: QueryKind,
    pub sql: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone)]
pub struct QueryFile {
    pub file: PathBuf,
    pub module: String,
    pub queries: Vec<QueryDef>,
}

pub fn load_package_queries(project: &ProjectConfig, pkg: &PackageConfig) -> anyhow::Result<Vec<QueryFile>> {
    let files = expand_globs(project, &pkg.queries)?;

    let mut module_by_file: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut file_by_module: BTreeMap<String, PathBuf> = BTreeMap::new();
    for f in &files {
        let stem = f
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid file name: {}", f.display()))?;
        let module = sanitize_module_name(stem);
        if let Some(existing_file) = file_by_module.get(&module) {
            anyhow::bail!(
                "module name collision: {module} ({} and {})",
                existing_file.display(),
                f.display()
            );
        }
        module_by_file.insert(f.clone(), module.clone());
        file_by_module.insert(module, f.clone());
    }

    let mut seen_query_names: BTreeMap<String, SourceLocation> = BTreeMap::new();

    let mut out: Vec<QueryFile> = Vec::new();
    for f in files {
        let content = std::fs::read_to_string(&f)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", f.display()))?;
        let queries = parse_sqlc_file(&f, &content)?;

        for q in &queries {
            if let Some(prev) = seen_query_names.insert(q.name.clone(), q.location.clone()) {
                anyhow::bail!(
                    "duplicate query name: {} ({}:{}) previously defined at {}:{}",
                    q.name,
                    q.location.file.display(),
                    q.location.line,
                    prev.file.display(),
                    prev.line
                );
            }
        }

        let module = module_by_file
            .get(&f)
            .cloned()
            .unwrap_or_else(|| sanitize_module_name(f.file_stem().unwrap_or_default().to_string_lossy().as_ref()));

        out.push(QueryFile {
            file: f,
            module,
            queries,
        });
    }

    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

fn expand_globs(project: &ProjectConfig, patterns: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();

    for p in patterns {
        let abs = project.resolve_path(p);
        let pattern = abs
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid glob pattern: {}", abs.display()))?;

        let mut matched_any = false;
        for entry in glob::glob(pattern).map_err(|e| anyhow::anyhow!("invalid glob {pattern}: {e}"))? {
            let path = entry.map_err(|e| anyhow::anyhow!("glob error for {pattern}: {e}"))?;
            if path.is_file() {
                matched_any = true;
                files.insert(path);
            }
        }

        if !matched_any {
            anyhow::bail!("glob pattern matched no files: {p}");
        }
    }

    Ok(files.into_iter().collect())
}

pub fn parse_sqlc_file(path: &Path, content: &str) -> anyhow::Result<Vec<QueryDef>> {
    let mut queries: Vec<QueryDef> = Vec::new();

    let mut current_name: Option<String> = None;
    let mut current_kind: Option<QueryKind> = None;
    let mut current_decl_line: usize = 0;
    let mut sql_lines: Vec<&str> = Vec::new();

    let mut before_first = true;

    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("-- name:") {
            before_first = false;
            if let Some(name) = current_name.take() {
                let kind = current_kind.take().expect("kind present with name");
                let sql = join_sql(sql_lines.drain(..));
                if sql.trim().is_empty() {
                    anyhow::bail!("empty SQL body for query {name} at {}:{current_decl_line}", path.display());
                }
                queries.push(QueryDef {
                    name,
                    kind,
                    sql,
                    location: SourceLocation {
                        file: path.to_path_buf(),
                        line: current_decl_line,
                    },
                });
            }

            let rest = rest.trim();
            let mut parts = rest.split_whitespace();
            let Some(name) = parts.next() else {
                anyhow::bail!("missing query name after `-- name:` at {}:{line_no}", path.display());
            };
            let Some(kind_raw) = parts.next() else {
                anyhow::bail!(
                    "missing query kind (e.g. :one/:opt) after name at {}:{line_no}",
                    path.display()
                );
            };
            if parts.next().is_some() {
                anyhow::bail!("unexpected tokens in query header at {}:{line_no}", path.display());
            }

            let kind_raw = kind_raw
                .strip_prefix(':')
                .ok_or_else(|| anyhow::anyhow!("invalid query kind {kind_raw} at {}:{line_no}", path.display()))?;
            let kind = QueryKind::from_sqlc_annotation(kind_raw)?;

            current_name = Some(name.to_string());
            current_kind = Some(kind);
            current_decl_line = line_no;
            continue;
        }

        if before_first {
            if is_blank_or_comment(trimmed) {
                continue;
            }
            anyhow::bail!(
                "unexpected SQL before first `-- name:` in {}:{line_no}",
                path.display()
            );
        }

        if current_name.is_some() {
            sql_lines.push(line);
        }
    }

    if let Some(name) = current_name.take() {
        let kind = current_kind.take().expect("kind present with name");
        let sql = join_sql(sql_lines.drain(..));
        if sql.trim().is_empty() {
            anyhow::bail!(
                "empty SQL body for query {name} at {}:{current_decl_line}",
                path.display()
            );
        }
        queries.push(QueryDef {
            name,
            kind,
            sql,
            location: SourceLocation {
                file: path.to_path_buf(),
                line: current_decl_line,
            },
        });
    }

    if queries.is_empty() {
        anyhow::bail!("no queries found in file {}", path.display());
    }

    Ok(queries)
}

fn join_sql<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for (i, line) in lines.enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.trim().to_string()
}

fn is_blank_or_comment(trimmed_line: &str) -> bool {
    trimmed_line.is_empty() || trimmed_line.starts_with("--")
}

fn sanitize_module_name(stem: &str) -> String {
    let s = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let s = heck::AsSnakeCase(&s).to_string();
    if s.is_empty() {
        "_".to_string()
    } else if s.chars().next().unwrap().is_ascii_digit() {
        format!("_{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sqlc_file_basic() {
        let sql = r#"
-- name: GetUser :opt
SELECT id, email
FROM users
WHERE id = $1;

-- name: ListUsers :many
SELECT id, email
FROM users;
"#;

        let qs = parse_sqlc_file(Path::new("queries/users.sql"), sql).unwrap();
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].name, "GetUser");
        assert_eq!(qs[0].kind, QueryKind::Opt);
        assert!(qs[0].sql.contains("SELECT id, email"));
        assert_eq!(qs[1].name, "ListUsers");
        assert_eq!(qs[1].kind, QueryKind::Many);
    }

    #[test]
    fn parse_sqlc_file_rejects_sql_before_first_name() {
        let err = parse_sqlc_file(Path::new("queries/users.sql"), "SELECT 1;").unwrap_err();
        assert!(err.to_string().contains("before first `-- name:`"));
    }

    #[test]
    fn parse_sqlc_file_requires_kind() {
        let err = parse_sqlc_file(Path::new("queries/users.sql"), "-- name: GetUser\nSELECT 1;")
            .unwrap_err();
        assert!(err.to_string().contains("missing query kind"));
    }
}
