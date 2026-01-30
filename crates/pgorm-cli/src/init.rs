use crate::cli::GenInitArgs;
use std::path::Path;

pub fn run(args: GenInitArgs) -> anyhow::Result<()> {
    write_template(&args.config)
}

fn write_template(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing file: {}", path.display());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!("failed to create directory {}: {e}", parent.display())
            })?;
        }
    }

    let content = r#"
version = "1"
engine = "postgres"

[database]
url = "${DATABASE_URL}"
schemas = ["public"]

[schema_cache]
dir = ".pgorm"
file = "schema.json"
mode = "auto" # auto | refresh | cache_only

[[packages]]
name = "db"
queries = ["queries/**/*.sql"]
out = "src/db"

[packages.codegen]
emit_queries_struct = true
emit_query_constants = true
emit_tagged_exec = true

row_derives = ["FromRow", "Debug", "Clone"]
param_derives = ["Debug", "Clone"]
extra_uses = ["pgorm::FromRow"]

[packages.types]
"uuid" = "uuid::Uuid"
"timestamptz" = "chrono::DateTime<chrono::Utc>"
"jsonb" = "serde_json::Value"

[packages.overrides]
# param."GetUser".1 = "i64"
# column."SearchUsers".created_at = "chrono::DateTime<chrono::Utc>"

# --- Optional: generate Rust models from schema ---
#
# [models]
# out = "src/models"
# dialect = "pgorm"
# include_views = false
# # If empty, generate all tables in the selected schemas.
# # Items can be "table" or "schema.table".
# tables = []
#
# # Map "table" or "schema.table" -> Rust struct name.
# # rename."public.users" = "User"
#
# # Map "table" or "schema.table" -> primary key column (emits #[orm(id)]).
# # primary_key."public.users" = "id"
#
# [models.types]
# "uuid" = "uuid::Uuid"
# "timestamptz" = "chrono::DateTime<chrono::Utc>"
# "jsonb" = "serde_json::Value"
"#
    .trim_start_matches('\n');

    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;

    println!("wrote {}", path.display());
    Ok(())
}
