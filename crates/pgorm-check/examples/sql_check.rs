//! SQL vs schema check example (with local schema cache)
//!
//! Run with:
//! `cargo run --example sql_check -p pgorm --features check`
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, OrmError};
use pgorm_check::{SchemaCache, SchemaCacheConfig, SchemaCacheLoad, check_sql_cached};
use std::{env, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment variable");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup: ensure table exists
    client
        .execute(
            r#"
CREATE TABLE IF NOT EXISTS tasks (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL
)
"#,
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Cache schema under the current working directory (configurable).
    let mut config = SchemaCacheConfig::default();
    config.cache_dir = PathBuf::from(".pgorm");
    config.schemas = vec!["public".to_string()];

    println!("schema cache path: {}", SchemaCache::cache_path(&config).display());

    // First run: will refresh cache if missing or schema changed.
    let sql_ok = "SELECT id, title FROM tasks";
    let (load, issues) = check_sql_cached(&client, &config, sql_ok).await?;
    print_load(load);
    print_issues(sql_ok, &issues);

    // Second run: should be a cache hit when schema fingerprint is unchanged.
    let sql_bad = "SELECT id, missing_col FROM tasks";
    let (load, issues) = check_sql_cached(&client, &config, sql_bad).await?;
    print_load(load);
    print_issues(sql_bad, &issues);

    Ok(())
}

fn print_load(load: SchemaCacheLoad) {
    match load {
        SchemaCacheLoad::CacheHit => println!("schema cache: hit"),
        SchemaCacheLoad::Refreshed => println!("schema cache: refreshed"),
    }
}

fn print_issues(sql: &str, issues: &[pgorm_check::SqlCheckIssue]) {
    if issues.is_empty() {
        println!("SQL OK: {sql}");
        return;
    }

    println!("SQL issues: {sql}");
    for issue in issues {
        println!(
            "  - {:?} {:?} @{:?}: {}",
            issue.level, issue.kind, issue.location, issue.message
        );
    }
}

