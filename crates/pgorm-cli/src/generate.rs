use crate::cli::GenRunArgs;
use crate::codegen::generate_package;
use crate::config::{ProjectConfig, SchemaCacheMode};
use crate::queries::load_package_queries;
use crate::schema::{connect_db, load_schema_cache, read_schema_cache};
use crate::write::{WriteOptions, apply_generated_files};
use pgorm_check::{LintLevel, SqlCheckLevel};

pub async fn run(args: GenRunArgs) -> anyhow::Result<()> {
    let project = ProjectConfig::load(args.config.clone())?;

    let database_url = args
        .database
        .clone()
        .unwrap_or_else(|| project.file.database.url.clone());

    let schemas = if !project.file.database.schemas.is_empty() {
        project.file.database.schemas.clone()
    } else {
        vec!["public".to_string()]
    };

    let cache_cfg = pgorm_check::SchemaCacheConfig {
        cache_dir: project
            .resolve_path(project.file.schema_cache.dir.as_deref().unwrap_or(".pgorm")),
        cache_file_name: project
            .file
            .schema_cache
            .file
            .clone()
            .unwrap_or_else(|| "schema.json".to_string()),
        schemas: schemas.clone(),
    };
    let mode: SchemaCacheMode = project.file.schema_cache.mode;

    let cache = match mode {
        SchemaCacheMode::CacheOnly => read_schema_cache(&cache_cfg)?,
        _ => {
            let client = connect_db(&database_url).await?;
            let (cache, _load) = load_schema_cache(&client, &cache_cfg, mode).await?;
            cache
        }
    };

    let mut had_error = false;
    let mut had_warning = false;

    let mut generated_files: Vec<crate::codegen::GeneratedFile> = Vec::new();

    for pkg in &project.file.packages {
        let query_files = load_package_queries(&project, pkg)?;

        for qf in &query_files {
            for q in &qf.queries {
                let loc = format!("{}:{}", q.location.file.display(), q.location.line);
                let header = format!("{loc} {}", q.name);

                // Syntax
                let parse = pgorm_check::is_valid_sql(&q.sql);
                if !parse.valid {
                    had_error = true;
                    eprintln!(
                        "[ERROR] {header}: SQL syntax error: {}",
                        parse.error.unwrap_or_else(|| "unknown error".to_string())
                    );
                    continue;
                }

                // Multi-statement guard (MVP)
                if let Ok(parsed) = pg_query::parse(&q.sql) {
                    if parsed.protobuf.stmts.len() != 1 {
                        had_error = true;
                        eprintln!(
                            "[ERROR] {header}: multiple SQL statements are not supported (MVP)"
                        );
                        continue;
                    }
                }

                // Lint
                let lint = pgorm_check::lint_sql(&q.sql);
                for issue in lint.issues {
                    match issue.level {
                        LintLevel::Error => had_error = true,
                        LintLevel::Warning => had_warning = true,
                        LintLevel::Info => {}
                    }
                    eprintln!(
                        "[{:?}] {header}: {} {}",
                        issue.level, issue.code, issue.message
                    );
                }

                // Schema references
                match pgorm_check::check_sql(&cache.schema, &q.sql) {
                    Ok(issues) => {
                        for issue in issues {
                            match issue.level {
                                SqlCheckLevel::Error => had_error = true,
                                SqlCheckLevel::Warning => had_warning = true,
                            }
                            eprintln!("[{:?}] {header}: {}", issue.level, issue.message);
                        }
                    }
                    Err(e) => {
                        had_error = true;
                        eprintln!("[ERROR] {header}: check failed: {e}");
                    }
                }
            }
        }

        if had_error {
            continue;
        }

        let mut pkg_files = generate_package(&project, pkg, &cache.schema, &query_files)?;
        generated_files.append(&mut pkg_files);
    }

    if had_error {
        anyhow::bail!("gen failed due to previous errors");
    }

    if had_warning {
        eprintln!("[WARN] gen completed with warnings");
    }

    apply_generated_files(
        &generated_files,
        WriteOptions {
            dry_run: args.dry_run,
            check: args.check,
        },
    )?;

    Ok(())
}
