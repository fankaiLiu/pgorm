use crate::client::CheckClient;
use crate::error::{CheckError, CheckResult};
use crate::schema_introspect::{DbSchema, load_schema_from_db, schema_fingerprint};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SchemaCacheConfig {
    /// Directory to store cache files (default: `./.pgorm`).
    pub cache_dir: PathBuf,
    /// Cache file name inside `cache_dir` (default: `schema.json`).
    pub cache_file_name: String,
    /// Which PostgreSQL schemas to introspect (default: `["public"]`).
    pub schemas: Vec<String>,
}

impl Default for SchemaCacheConfig {
    fn default() -> Self {
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".pgorm");

        Self {
            cache_dir,
            cache_file_name: "schema.json".to_string(),
            schemas: vec!["public".to_string()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaCacheLoad {
    /// Loaded from local cache (fingerprint unchanged).
    CacheHit,
    /// Loaded from database (cache missing/invalid or fingerprint changed).
    Refreshed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaCache {
    pub version: u32,
    pub retrieved_at: DateTime<Utc>,
    pub schemas: Vec<String>,
    pub fingerprint: String,
    pub schema: DbSchema,
}

impl SchemaCache {
    pub fn cache_path(config: &SchemaCacheConfig) -> PathBuf {
        config.cache_dir.join(&config.cache_file_name)
    }

    pub async fn load_or_refresh<C: CheckClient>(
        client: &C,
        config: &SchemaCacheConfig,
    ) -> CheckResult<(Self, SchemaCacheLoad)> {
        let cache_path = Self::cache_path(config);

        if let Ok(cached) = read_cache_file(&cache_path) {
            if cached.schemas == config.schemas && cached.version == 1 {
                let current_fp = schema_fingerprint(client, &config.schemas).await?;
                if current_fp == cached.fingerprint {
                    return Ok((cached, SchemaCacheLoad::CacheHit));
                }
            }
        }

        let (schema, fingerprint) = load_schema_from_db(client, &config.schemas).await?;
        let refreshed = SchemaCache {
            version: 1,
            retrieved_at: Utc::now(),
            schemas: config.schemas.clone(),
            fingerprint,
            schema,
        };

        write_cache_file(&cache_path, &refreshed)?;
        Ok((refreshed, SchemaCacheLoad::Refreshed))
    }
}

fn read_cache_file(path: &Path) -> CheckResult<SchemaCache> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CheckError::Other(e.to_string()));
        }
        Err(e) => return Err(CheckError::Other(e.to_string())),
    };

    serde_json::from_slice::<SchemaCache>(&data)
        .map_err(|e| CheckError::Serialization(format!("Failed to parse schema cache: {e}")))
}

fn write_cache_file(path: &Path, cache: &SchemaCache) -> CheckResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CheckError::Other(e.to_string()))?;
    }

    let tmp_path = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(cache)
        .map_err(|e| CheckError::Serialization(format!("Failed to serialize schema cache: {e}")))?;

    std::fs::write(&tmp_path, data).map_err(|e| CheckError::Other(e.to_string()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| CheckError::Other(e.to_string()))?;
    Ok(())
}
