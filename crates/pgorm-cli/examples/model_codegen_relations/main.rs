//! Example demonstrating model codegen with inferred relations and generated write models.
//!
//! Run with:
//!   cargo run -p pgorm-cli --example model_codegen_relations
//!
//! This example uses a local schema cache (`schema_cache.mode = "cache_only"`),
//! so it does not require a live database.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn make_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "pgorm-model-codegen-example-{}-{nanos}",
        std::process::id()
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = make_temp_dir();
    fs::create_dir_all(&root)?;

    let config_path = root.join("pgorm.toml");
    write_file(
        &config_path,
        r#"
version = "1"

[database]
url = "postgres://unused/unused"
schemas = ["public"]

[schema_cache]
dir = ".pgorm"
file = "schema.json"
mode = "cache_only"

[models]
out = "generated_models"
tables = ["users", "posts", "comments"]

[models.rename]
users = "User"
posts = "Post"
comments = "Comment"
"#,
    )?;

    let schema_cache_path = root.join(".pgorm").join("schema.json");
    write_file(
        &schema_cache_path,
        r#"
{
  "version": 1,
  "retrieved_at": "2026-02-08T00:00:00Z",
  "schemas": ["public"],
  "fingerprint": "demo-fingerprint",
  "schema": {
    "schemas": ["public"],
    "tables": [
      {
        "schema": "public",
        "name": "users",
        "kind": "table",
        "columns": [
          { "name": "id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 1 },
          { "name": "name", "data_type": "text", "not_null": true, "default_expr": null, "ordinal": 2 }
        ]
      },
      {
        "schema": "public",
        "name": "posts",
        "kind": "table",
        "columns": [
          { "name": "id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 1 },
          { "name": "user_id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 2 },
          { "name": "title", "data_type": "text", "not_null": true, "default_expr": null, "ordinal": 3 }
        ]
      },
      {
        "schema": "public",
        "name": "comments",
        "kind": "table",
        "columns": [
          { "name": "id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 1 },
          { "name": "post_id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 2 },
          { "name": "user_id", "data_type": "bigint", "not_null": true, "default_expr": null, "ordinal": 3 },
          { "name": "body", "data_type": "text", "not_null": true, "default_expr": null, "ordinal": 4 }
        ]
      }
    ]
  }
}
"#,
    )?;

    pgorm_cli::run(vec![
        "pgorm".to_string(),
        "model".to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
    ])
    .await?;

    let out_dir = root.join("generated_models");
    let user_rs = fs::read_to_string(out_dir.join("users.rs"))?;
    let post_rs = fs::read_to_string(out_dir.join("posts.rs"))?;

    assert!(
        user_rs
            .contains("#[orm(has_many(super::Post, foreign_key = \"user_id\", as = \"posts\"))]")
    );
    assert!(
        post_rs
            .contains("#[orm(belongs_to(super::User, foreign_key = \"user_id\", as = \"user\"))]")
    );
    assert!(user_rs.contains("pub struct NewUser"));
    assert!(user_rs.contains("pub struct UserPatch"));

    println!("generated files in: {}", out_dir.display());
    println!("\nusers.rs key lines:");
    for line in user_rs.lines().filter(|line| {
        line.contains("has_many(")
            || line.contains("pub struct User")
            || line.contains("pub struct NewUser")
            || line.contains("pub struct UserPatch")
    }) {
        println!("{line}");
    }

    println!("\nposts.rs key lines:");
    for line in post_rs.lines().filter(|line| {
        line.contains("belongs_to(")
            || line.contains("pub struct Post")
            || line.contains("pub struct NewPost")
            || line.contains("pub struct PostPatch")
    }) {
        println!("{line}");
    }

    Ok(())
}
