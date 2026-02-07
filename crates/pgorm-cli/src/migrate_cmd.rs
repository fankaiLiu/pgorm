use crate::cli::{
    MigrateCommand, MigrateDiffArgs, MigrateDownArgs, MigrateInitArgs, MigrateNewArgs,
    MigrateStatusArgs, MigrateUpArgs,
};
use crate::config::ProjectConfig;
use anyhow::Context;
use chrono::Utc;
use heck::ToSnakeCase;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub async fn run(cmd: MigrateCommand) -> anyhow::Result<()> {
    match cmd {
        MigrateCommand::Init(args) => run_init(args),
        MigrateCommand::New(args) => run_new(args),
        MigrateCommand::Up(args) => run_up(args).await,
        MigrateCommand::Down(args) => run_down(args).await,
        MigrateCommand::Status(args) => run_status(args).await,
        MigrateCommand::Diff(args) => run_diff(args).await,
    }
}

fn normalize_name(name: &str) -> anyhow::Result<String> {
    let mut s = name.to_snake_case();
    s = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        anyhow::bail!("migration name becomes empty after normalization");
    }
    Ok(s)
}

fn current_version() -> anyhow::Result<i64> {
    Utc::now()
        .format("%Y%m%d%H%M%S")
        .to_string()
        .parse::<i64>()
        .context("failed to create migration version")
}

fn existing_versions(dir: &Path) -> HashSet<i64> {
    if !dir.exists() {
        return HashSet::new();
    }
    match pgorm::migrate::scan_migrations_dir(dir) {
        Ok(v) => v.into_iter().map(|m| m.version).collect(),
        Err(_) => HashSet::new(),
    }
}

fn run_init(args: MigrateInitArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.dir)
        .with_context(|| format!("failed to create {}", args.dir.display()))?;

    let readme = args.dir.join("README.md");
    if !readme.exists() {
        std::fs::write(
            &readme,
            "# Migrations\n\nUse files like:\n- V20260101090000__create_users.up.sql\n- V20260101090000__create_users.down.sql\n",
        )
        .with_context(|| format!("failed to write {}", readme.display()))?;
    }

    println!("initialized migrations dir: {}", args.dir.display());
    Ok(())
}

fn run_new(args: MigrateNewArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.dir)
        .with_context(|| format!("failed to create {}", args.dir.display()))?;

    let name = normalize_name(&args.name)?;
    let used_versions = existing_versions(&args.dir);

    let mut version = current_version()?;
    while used_versions.contains(&version) {
        version += 1;
    }

    let base = format!("V{version}__{name}");
    let up_path = if args.with_down {
        args.dir.join(format!("{base}.up.sql"))
    } else {
        args.dir.join(format!("{base}.sql"))
    };

    if up_path.exists() {
        anyhow::bail!("refusing to overwrite existing file: {}", up_path.display());
    }

    let up_template = format!(
        "-- Migration: {base}\n-- Created at: {} UTC\n\n-- Write your UP migration here.\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S")
    );
    std::fs::write(&up_path, up_template)
        .with_context(|| format!("failed to write {}", up_path.display()))?;
    println!("created {}", up_path.display());

    if args.with_down {
        let down_path = args.dir.join(format!("{base}.down.sql"));
        if down_path.exists() {
            anyhow::bail!(
                "refusing to overwrite existing file: {}",
                down_path.display()
            );
        }

        let down_template = format!(
            "-- Rollback for: {base}\n-- Created at: {} UTC\n\n-- Write your DOWN migration here.\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        std::fs::write(&down_path, down_template)
            .with_context(|| format!("failed to write {}", down_path.display()))?;
        println!("created {}", down_path.display());
    }

    Ok(())
}

fn resolve_config(config: &Path) -> anyhow::Result<Option<ProjectConfig>> {
    if config.exists() {
        Ok(Some(ProjectConfig::load(config.to_path_buf())?))
    } else {
        Ok(None)
    }
}

fn resolve_dir(config: Option<&ProjectConfig>, dir: Option<PathBuf>) -> PathBuf {
    match dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => config.map(|c| c.resolve_path(&path)).unwrap_or(path),
        None => config
            .map(|c| c.resolve_path("migrations"))
            .unwrap_or_else(|| PathBuf::from("migrations")),
    }
}

fn resolve_database(
    config_path: &Path,
    config: Option<&ProjectConfig>,
    database: Option<String>,
) -> anyhow::Result<String> {
    if let Some(v) = database {
        return Ok(v);
    }
    if let Some(cfg) = config {
        return Ok(cfg.file.database.url.clone());
    }
    anyhow::bail!(
        "database URL is required: pass --database or provide {}",
        config_path.display()
    )
}

async fn connect(database_url: &str) -> anyhow::Result<tokio_postgres::Client> {
    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
        .await
        .with_context(|| format!("failed to connect to database: {}", database_url))?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            eprintln!("connection error: {err}");
        }
    });

    Ok(client)
}

async fn run_up(args: MigrateUpArgs) -> anyhow::Result<()> {
    let cfg = resolve_config(&args.config)?;
    let dir = resolve_dir(cfg.as_ref(), args.dir);
    let database_url = resolve_database(&args.config, cfg.as_ref(), args.database)?;
    let mut client = connect(&database_url).await?;

    if args.dry_run {
        let st = pgorm::migrate::status(&mut client, &dir).await?;
        let pending: Vec<_> = match args.to {
            Some(v) => st.pending.into_iter().filter(|m| m.version <= v).collect(),
            None => st.pending,
        };

        if pending.is_empty() {
            println!("no pending migrations");
        } else {
            println!("pending migrations (dry-run):");
            for m in pending {
                println!("  V{}__{}", m.version, m.name);
            }
        }
        return Ok(());
    }

    let report = match args.to {
        Some(v) => pgorm::migrate::run_to(&mut client, &dir, v).await?,
        None => pgorm::migrate::up_dir(&mut client, &dir).await?,
    };

    let applied = report.applied_migrations();
    println!("applied {} migration(s)", applied.len());
    for m in applied {
        println!("  {}", m);
    }
    Ok(())
}

async fn run_down(args: MigrateDownArgs) -> anyhow::Result<()> {
    let cfg = resolve_config(&args.config)?;
    let dir = resolve_dir(cfg.as_ref(), args.dir);
    let database_url = resolve_database(&args.config, cfg.as_ref(), args.database)?;
    let mut client = connect(&database_url).await?;

    if args.dry_run {
        let st = pgorm::migrate::status(&mut client, &dir).await?;
        let candidates: Vec<_> = match args.to {
            Some(v) => st.applied.into_iter().filter(|m| m.version > v).collect(),
            None => {
                let n = args.steps.unwrap_or(1);
                st.applied.into_iter().rev().take(n).collect()
            }
        };

        if candidates.is_empty() {
            println!("no migrations to roll back");
        } else {
            println!("rollback migrations (dry-run):");
            for m in candidates {
                println!("  V{}__{}", m.version, m.name);
            }
        }
        return Ok(());
    }

    let rolled_back = match args.to {
        Some(v) => pgorm::migrate::down_to(&mut client, &dir, v).await?,
        None => pgorm::migrate::down_steps(&mut client, &dir, args.steps.unwrap_or(1)).await?,
    };

    if rolled_back.is_empty() {
        println!("no migrations rolled back");
    } else {
        println!("rolled back {} migration(s)", rolled_back.len());
        for m in rolled_back {
            println!("  V{}__{}", m.version, m.name);
        }
    }
    Ok(())
}

async fn run_status(args: MigrateStatusArgs) -> anyhow::Result<()> {
    let cfg = resolve_config(&args.config)?;
    let dir = resolve_dir(cfg.as_ref(), args.dir);
    let database_url = resolve_database(&args.config, cfg.as_ref(), args.database)?;
    let mut client = connect(&database_url).await?;

    let st = pgorm::migrate::status(&mut client, &dir).await?;

    println!("migrations dir: {}", dir.display());
    println!("local:   {}", st.local.len());
    println!("applied: {}", st.applied.len());
    println!("pending: {}", st.pending.len());

    if !st.pending.is_empty() {
        println!("\npending:");
        for m in st.pending {
            println!("  V{}__{}", m.version, m.name);
        }
    }

    if !st.missing_local.is_empty() {
        println!("\nmissing local files (applied in DB, not found on disk):");
        for m in st.missing_local {
            println!("  V{}__{}", m.version, m.name);
        }
    }

    Ok(())
}

async fn run_diff(args: MigrateDiffArgs) -> anyhow::Result<()> {
    let cfg = resolve_config(&args.config)?;
    let dir = resolve_dir(cfg.as_ref(), args.dir);
    let database_url = resolve_database(&args.config, cfg.as_ref(), args.database)?;
    let mut client = connect(&database_url).await?;

    let draft = pgorm::migrate::diff_pending_sql(&mut client, &dir).await?;

    if let Some(output) = args.output {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create output directory {}", parent.display())
                })?;
            }
        }
        std::fs::write(&output, draft)
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!("wrote {}", output.display());
    } else {
        print!("{draft}");
    }

    Ok(())
}
