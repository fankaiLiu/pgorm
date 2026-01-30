use crate::cli::GenSchemaArgs;
use crate::config::{ProjectConfig, SchemaCacheMode};
use pgorm_check::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad};
use tokio_postgres::NoTls;

pub async fn run(args: GenSchemaArgs) -> anyhow::Result<()> {
    let (database_url, cache_cfg, schemas, mode) = if args.config.exists() {
        let project = ProjectConfig::load(args.config.clone())?;

        let database_url = args
            .database
            .clone()
            .unwrap_or_else(|| project.file.database.url.clone());

        let schemas = if let Some(schemas) = args.schemas.clone() {
            schemas
        } else if !project.file.database.schemas.is_empty() {
            project.file.database.schemas.clone()
        } else {
            vec!["public".to_string()]
        };

        let cache_cfg = to_cache_config(&project, &schemas);
        (database_url, cache_cfg, schemas, SchemaCacheMode::Refresh)
    } else {
        let Some(database_url) = args.database.clone() else {
            anyhow::bail!(
                "failed to load config {}; provide --database or run `pgorm gen init` first",
                args.config.display()
            );
        };
        let schemas = args.schemas.clone().unwrap_or_else(|| vec!["public".to_string()]);
        let cache_cfg = SchemaCacheConfig {
            cache_dir: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".pgorm"),
            cache_file_name: "schema.json".to_string(),
            schemas: schemas.clone(),
        };
        (database_url, cache_cfg, schemas, SchemaCacheMode::Refresh)
    };

    let client = connect_db(&database_url).await?;
    let (cache, load) = load_schema_cache(&client, &cache_cfg, mode).await?;

    println!(
        "schema cache: {} (schemas: {})",
        match load {
            SchemaCacheLoad::CacheHit => "cache hit",
            SchemaCacheLoad::Refreshed => "refreshed",
        },
        schemas.join(",")
    );
    println!(
        "cache file: {}",
        SchemaCache::cache_path(&cache_cfg).display()
    );
    println!("fingerprint: {}", cache.fingerprint);

    Ok(())
}

pub async fn connect_db(database_url: &str) -> anyhow::Result<tokio_postgres::Client> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("postgres connection error: {e}");
        }
    });
    Ok(client)
}

pub async fn load_schema_cache(
    client: &impl pgorm_check::CheckClient,
    cfg: &SchemaCacheConfig,
    mode: SchemaCacheMode,
) -> anyhow::Result<(SchemaCache, SchemaCacheLoad)> {
    match mode {
        SchemaCacheMode::Auto => {
            let (cache, load) = SchemaCache::load_or_refresh(client, cfg).await?;
            Ok((cache, load))
        }
        SchemaCacheMode::Refresh => {
            let (schema, fingerprint) =
                pgorm_check::schema_introspect::load_schema_from_db(client, &cfg.schemas).await?;
            let cache = SchemaCache {
                version: 1,
                retrieved_at: chrono::Utc::now(),
                schemas: cfg.schemas.clone(),
                fingerprint,
                schema,
            };
            write_schema_cache(&SchemaCache::cache_path(cfg), &cache)?;
            Ok((cache, SchemaCacheLoad::Refreshed))
        }
        SchemaCacheMode::CacheOnly => {
            let cache_path = SchemaCache::cache_path(cfg);
            let data = std::fs::read(&cache_path).map_err(|e| {
                anyhow::anyhow!("failed to read schema cache {}: {e}", cache_path.display())
            })?;
            let cache: SchemaCache = serde_json::from_slice(&data).map_err(|e| {
                anyhow::anyhow!("failed to parse schema cache {}: {e}", cache_path.display())
            })?;

            if cache.version != 1 {
                anyhow::bail!(
                    "unsupported schema cache version {} in {}",
                    cache.version,
                    cache_path.display()
                );
            }
            if cache.schemas != cfg.schemas {
                anyhow::bail!(
                    "schema cache schemas mismatch (cache: {:?}, requested: {:?})",
                    cache.schemas,
                    cfg.schemas
                );
            }

            Ok((cache, SchemaCacheLoad::CacheHit))
        }
    }
}

fn write_schema_cache(path: &std::path::Path, cache: &SchemaCache) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            anyhow::anyhow!("failed to create directory {}: {e}", parent.display())
        })?;
    }

    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(cache)
        .map_err(|e| anyhow::anyhow!("failed to serialize schema cache: {e}"))?;

    std::fs::write(&tmp, data)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("failed to rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

fn to_cache_config(project: &ProjectConfig, schemas: &[String]) -> SchemaCacheConfig {
    let dir = project
        .file
        .schema_cache
        .dir
        .as_deref()
        .unwrap_or(".pgorm");
    let file = project
        .file
        .schema_cache
        .file
        .as_deref()
        .unwrap_or("schema.json");

    SchemaCacheConfig {
        cache_dir: project.resolve_path(dir),
        cache_file_name: file.to_string(),
        schemas: schemas.to_vec(),
    }
}
