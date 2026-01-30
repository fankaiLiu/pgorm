use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    #[allow(dead_code)]
    pub config_path: PathBuf,
    pub config_dir: PathBuf,
    pub file: ConfigFile,
}

impl ProjectConfig {
    pub fn load(config_path: PathBuf) -> anyhow::Result<Self> {
        let config_dir = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        let raw = std::fs::read_to_string(&config_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read config file {}: {e}",
                config_path.display()
            )
        })?;

        let mut file: ConfigFile = toml::from_str(&raw).map_err(|e| {
            anyhow::anyhow!(
                "failed to parse config file {}: {e}",
                config_path.display()
            )
        })?;

        file.expand_env()?;
        file.validate()?;

        Ok(Self {
            config_path,
            config_dir,
            file,
        })
    }

    pub fn resolve_path(&self, p: impl AsRef<Path>) -> PathBuf {
        let p = p.as_ref();
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.config_dir.join(p)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFile {
    pub version: String,
    pub engine: Option<String>,

    pub database: DatabaseConfig,

    #[serde(default)]
    pub schema_cache: SchemaCacheConfig,

    #[serde(default)]
    pub packages: Vec<PackageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default)]
    pub schemas: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaCacheMode {
    Auto,
    Refresh,
    CacheOnly,
}

impl Default for SchemaCacheMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchemaCacheConfig {
    pub dir: Option<String>,
    pub file: Option<String>,
    #[serde(default)]
    pub mode: SchemaCacheMode,
}

impl Default for SchemaCacheConfig {
    fn default() -> Self {
        Self {
            dir: Some(".pgorm".to_string()),
            file: Some("schema.json".to_string()),
            mode: SchemaCacheMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageConfig {
    pub name: String,
    pub queries: Vec<String>,
    pub out: String,

    #[serde(default)]
    pub codegen: CodegenConfig,

    #[serde(default)]
    pub types: BTreeMap<String, String>,

    #[serde(default)]
    pub overrides: OverridesConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodegenConfig {
    #[serde(default = "default_true")]
    pub emit_queries_struct: bool,
    #[serde(default = "default_true")]
    pub emit_query_constants: bool,
    #[serde(default = "default_true")]
    pub emit_tagged_exec: bool,

    #[serde(default = "default_row_derives")]
    pub row_derives: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub param_derives: Vec<String>,
    #[serde(default)]
    pub extra_uses: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_row_derives() -> Vec<String> {
    vec!["FromRow".to_string(), "Debug".to_string(), "Clone".to_string()]
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            emit_queries_struct: true,
            emit_query_constants: true,
            emit_tagged_exec: true,
            row_derives: default_row_derives(),
            param_derives: Vec::new(),
            extra_uses: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OverridesConfig {
    #[serde(default)]
    pub param: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub column: BTreeMap<String, BTreeMap<String, String>>,
}

impl OverridesConfig {
    pub fn param_override(&self, query_name: &str, pos: usize) -> Option<&str> {
        self.param
            .get(query_name)
            .and_then(|m| m.get(&pos.to_string()))
            .map(|s| s.as_str())
    }

    pub fn column_override(&self, query_name: &str, column: &str) -> Option<&str> {
        self.column
            .get(query_name)
            .and_then(|m| m.get(column))
            .map(|s| s.as_str())
    }
}

impl ConfigFile {
    fn expand_env(&mut self) -> anyhow::Result<()> {
        self.database.url = expand_env_vars(&self.database.url)?;

        for s in &mut self.database.schemas {
            *s = expand_env_vars(s)?;
        }

        if let Some(dir) = self.schema_cache.dir.as_mut() {
            *dir = expand_env_vars(dir)?;
        }
        if let Some(file) = self.schema_cache.file.as_mut() {
            *file = expand_env_vars(file)?;
        }

        for p in &mut self.packages {
            p.name = expand_env_vars(&p.name)?;
            p.out = expand_env_vars(&p.out)?;

            for q in &mut p.queries {
                *q = expand_env_vars(q)?;
            }

            for v in p.types.values_mut() {
                *v = expand_env_vars(v)?;
            }

            for map in p.overrides.param.values_mut() {
                for v in map.values_mut() {
                    *v = expand_env_vars(v)?;
                }
            }
            for map in p.overrides.column.values_mut() {
                for v in map.values_mut() {
                    *v = expand_env_vars(v)?;
                }
            }
        }

        Ok(())
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.version.trim() != "1" {
            anyhow::bail!("unsupported config version: {}", self.version);
        }
        if let Some(engine) = &self.engine {
            if engine != "postgres" {
                anyhow::bail!("unsupported engine: {engine}");
            }
        }

        if self.database.url.trim().is_empty() {
            anyhow::bail!("database.url must not be empty");
        }

        if self.database.schemas.is_empty() {
            // Keep it forgiving: default to public at runtime if empty.
        }

        if self.packages.is_empty() {
            anyhow::bail!("at least one [[packages]] entry is required");
        }

        let mut seen = std::collections::HashSet::<&str>::new();
        for p in &self.packages {
            if p.name.trim().is_empty() {
                anyhow::bail!("packages.name must not be empty");
            }
            if !seen.insert(p.name.as_str()) {
                anyhow::bail!("duplicate packages.name: {}", p.name);
            }
            if p.queries.is_empty() {
                anyhow::bail!("packages.queries must not be empty (package: {})", p.name);
            }
            if p.out.trim().is_empty() {
                anyhow::bail!("packages.out must not be empty (package: {})", p.name);
            }
        }

        Ok(())
    }
}

fn expand_env_vars(input: &str) -> anyhow::Result<String> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'

            let mut key = String::new();
            let mut closed = false;
            while let Some(&ch) = chars.peek() {
                chars.next();
                if ch == '}' {
                    closed = true;
                    break;
                }
                key.push(ch);
            }

            if !closed {
                anyhow::bail!("unterminated env var reference: ${{{key}}}");
            }
            if key.is_empty() {
                anyhow::bail!("invalid env var reference: ${{}}");
            }

            let v = std::env::var(&key)
                .map_err(|_| anyhow::anyhow!("missing env var for config expansion: {key}"))?;
            out.push_str(&v);
            continue;
        }

        out.push(c);
    }

    Ok(out)
}
