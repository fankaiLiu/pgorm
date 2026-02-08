use crate::cli::{
    BuildArgs, CheckArgs, GenCheckArgs, GenInitArgs, GenRunArgs, InitArgs, MigrateCommand,
    MigrateInitArgs, ModelRunArgs,
};
use crate::config::ProjectConfig;
use crate::{gen_check, generate, init, migrate_cmd, model_generate};

pub async fn run_init(args: InitArgs) -> anyhow::Result<()> {
    init::run(GenInitArgs {
        config: args.config.clone(),
    })?;

    if let Some(dir) = args.migrations_dir {
        migrate_cmd::run(MigrateCommand::Init(MigrateInitArgs { dir })).await?;
    }

    Ok(())
}

pub async fn run_build(args: BuildArgs) -> anyhow::Result<()> {
    let project = ProjectConfig::load(args.config.clone())?;
    let has_packages = !project.file.packages.is_empty();
    let has_models = project.file.models.is_some();

    let mut ran_any = false;

    if !args.skip_queries {
        if has_packages {
            generate::run(GenRunArgs {
                config: args.config.clone(),
                database: args.database.clone(),
                dry_run: args.dry_run,
                check: args.check,
            })
            .await?;
            ran_any = true;
        } else {
            eprintln!(
                "[INFO] skipping query codegen: no [[packages]] configured in {}",
                args.config.display()
            );
        }
    }

    if !args.skip_models {
        if has_models {
            model_generate::run(ModelRunArgs {
                config: args.config.clone(),
                database: args.database.clone(),
                dry_run: args.dry_run,
                check: args.check,
            })
            .await?;
            ran_any = true;
        } else {
            eprintln!(
                "[INFO] skipping model codegen: missing [models] in {}",
                args.config.display()
            );
        }
    }

    if !ran_any {
        anyhow::bail!(
            "nothing to build: configure [[packages]] and/or [models], or remove skip flags"
        );
    }

    Ok(())
}

pub async fn run_check(args: CheckArgs) -> anyhow::Result<()> {
    let project = ProjectConfig::load(args.config.clone())?;
    let has_packages = !project.file.packages.is_empty();
    let has_models = project.file.models.is_some();

    let mut ran_any = false;
    let mut failures: Vec<String> = Vec::new();

    if !args.skip_queries {
        if has_packages {
            ran_any = true;
            if let Err(err) = gen_check::run(GenCheckArgs {
                config: args.config.clone(),
                database: args.database.clone(),
                deny_warnings: args.deny_warnings,
            })
            .await
            {
                failures.push(format!("query check failed: {err:#}"));
            }
        } else {
            eprintln!(
                "[INFO] skipping query checks: no [[packages]] configured in {}",
                args.config.display()
            );
        }
    }

    if !args.skip_models {
        if has_models {
            ran_any = true;
            if let Err(err) = model_generate::run(ModelRunArgs {
                config: args.config.clone(),
                database: args.database.clone(),
                dry_run: false,
                check: true,
            })
            .await
            {
                failures.push(format!("model check failed: {err:#}"));
            }
        } else {
            eprintln!(
                "[INFO] skipping model checks: missing [models] in {}",
                args.config.display()
            );
        }
    }

    if !ran_any {
        anyhow::bail!(
            "nothing to check: configure [[packages]] and/or [models], or remove skip flags"
        );
    }

    if !failures.is_empty() {
        anyhow::bail!("{}", failures.join("\n\n"));
    }

    Ok(())
}
