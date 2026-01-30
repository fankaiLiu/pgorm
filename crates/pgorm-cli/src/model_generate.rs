use crate::cli::ModelRunArgs;
use crate::config::ProjectConfig;
use crate::model_codegen::generate_models;
use crate::schema::load_project_schema_cache;
use crate::write::{WriteOptions, apply_generated_files};

pub async fn run(args: ModelRunArgs) -> anyhow::Result<()> {
    let project = ProjectConfig::load(args.config.clone())?;

    let (cache, _schemas) =
        load_project_schema_cache(&project, args.database.clone(), None).await?;

    let Some(models_cfg) = project.file.models.as_ref() else {
        anyhow::bail!("missing [models] section in {}", args.config.display());
    };

    let generated_files = generate_models(&project, models_cfg, &cache.schema)?;

    apply_generated_files(
        &generated_files,
        WriteOptions {
            dry_run: args.dry_run,
            check: args.check,
        },
    )?;

    Ok(())
}
