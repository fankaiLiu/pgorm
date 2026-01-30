use crate::cli::SqlCheckArgs;
use crate::config::{ProjectConfig, SchemaCacheMode};
use crate::schema::{connect_db, load_project_schema_cache, load_schema_cache};
use crate::sql_validate::validate_sql;
use pgorm_check::SchemaCacheConfig;
use std::io::Read;

pub async fn run(args: SqlCheckArgs) -> anyhow::Result<()> {
    let (cache, _schemas) = if args.config.exists() {
        let project = ProjectConfig::load(args.config.clone())?;
        load_project_schema_cache(&project, args.database.clone(), args.schemas.clone()).await?
    } else {
        let Some(database_url) = args.database.clone() else {
            anyhow::bail!(
                "failed to load config {}; provide --database or run `pgorm gen init` first",
                args.config.display()
            );
        };

        let schemas = args
            .schemas
            .clone()
            .unwrap_or_else(|| vec!["public".to_string()]);
        let cache_cfg = SchemaCacheConfig {
            cache_dir: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".pgorm"),
            cache_file_name: "schema.json".to_string(),
            schemas: schemas.clone(),
        };

        let client = connect_db(&database_url).await?;
        let (cache, _load) =
            load_schema_cache(&client, &cache_cfg, SchemaCacheMode::Refresh).await?;
        (cache, schemas)
    };

    let mut had_error = false;
    let mut had_warning = false;

    if args.files.is_empty() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;

        if buf.trim().is_empty() {
            anyhow::bail!("no SQL provided (pass files or pipe SQL to stdin)");
        }

        let stmts = pg_query::split_with_parser(&buf)
            .map_err(|e| anyhow::anyhow!("failed to split SQL statements from stdin: {e}"))?;
        if stmts.is_empty() {
            anyhow::bail!("no SQL statements found in stdin");
        }
        for (idx, stmt) in stmts.into_iter().enumerate() {
            let header = format!("stdin:stmt{}", idx + 1);
            let summary = validate_sql(&header, stmt, &cache.schema);
            had_error |= summary.had_error;
            had_warning |= summary.had_warning;
        }
    } else {
        for file in &args.files {
            let content = std::fs::read_to_string(file)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", file.display()))?;
            let stmts = pg_query::split_with_parser(&content).map_err(|e| {
                anyhow::anyhow!(
                    "failed to split SQL statements from {}: {e}",
                    file.display()
                )
            })?;
            if stmts.is_empty() {
                anyhow::bail!("no SQL statements found in {}", file.display());
            }
            for (idx, stmt) in stmts.into_iter().enumerate() {
                let header = format!("{}:stmt{}", file.display(), idx + 1);
                let summary = validate_sql(&header, stmt, &cache.schema);
                had_error |= summary.had_error;
                had_warning |= summary.had_warning;
            }
        }
    }

    if had_error || (args.deny_warnings && had_warning) {
        anyhow::bail!("sql check failed");
    }

    Ok(())
}
