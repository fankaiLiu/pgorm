use crate::cli::GenRunArgs;
use crate::codegen::generate_package;
use crate::config::ProjectConfig;
use crate::queries::load_package_queries;
use crate::schema::load_project_schema_cache;
use crate::sql_validate::validate_sql;
use crate::write::{WriteOptions, apply_generated_files};

pub async fn run(args: GenRunArgs) -> anyhow::Result<()> {
    let project = ProjectConfig::load(args.config.clone())?;
    if project.file.packages.is_empty() {
        anyhow::bail!(
            "no [[packages]] configured in {}; run `pgorm gen init` to create a template",
            args.config.display()
        );
    }

    let (cache, _schemas) =
        load_project_schema_cache(&project, args.database.clone(), None).await?;

    let mut had_error = false;
    let mut had_warning = false;

    let mut generated_files: Vec<crate::codegen::GeneratedFile> = Vec::new();

    for pkg in &project.file.packages {
        let query_files = load_package_queries(&project, pkg)?;

        for qf in &query_files {
            for q in &qf.queries {
                let loc = format!("{}:{}", q.location.file.display(), q.location.line);
                let header = format!("{loc} {}", q.name);

                let summary = validate_sql(&header, &q.sql, &cache.schema);
                had_error |= summary.had_error;
                had_warning |= summary.had_warning;
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
