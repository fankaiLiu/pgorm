//! SQL vs schema check example (with local schema cache)
//!
//! Run with:
//! `cargo run --example sql_check -p pgorm-check --features sql`
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use colored::Colorize;
use pgorm::{create_pool, OrmError};
use pgorm_check::{check_sql_cached, SchemaCache, SchemaCacheConfig, SchemaCacheLoad, SqlCheckLevel, SqlCheckIssueKind};
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

    println!(
        "{} {}",
        "Cache path:".dimmed(),
        SchemaCache::cache_path(&config).display().to_string().cyan()
    );
    println!();

    // First run: will refresh cache if missing or schema changed.
    let sql_ok = "SELECT id, title FROM tasks";
    let (load, issues) = check_sql_cached(&client, &config, sql_ok).await?;
    print_load(load);
    print_result(sql_ok, &issues);

    println!();

    // Second run: should be a cache hit when schema fingerprint is unchanged.
    let sql_bad = "SELECT id, missing_col FROM tasks";
    let (load, issues) = check_sql_cached(&client, &config, sql_bad).await?;
    print_load(load);
    print_result(sql_bad, &issues);

    Ok(())
}

fn print_load(load: SchemaCacheLoad) {
    let status = match load {
        SchemaCacheLoad::CacheHit => "cache hit".green(),
        SchemaCacheLoad::Refreshed => "refreshed".yellow(),
    };
    println!("{} {}", "Schema:".dimmed(), status);
}

fn print_result(sql: &str, issues: &[pgorm_check::SqlCheckIssue]) {
    if issues.is_empty() {
        println!("{} {}", "✓".green().bold(), sql.white());
        return;
    }

    println!("{} {}", "✗".red().bold(), sql.white());

    for issue in issues {
        let level_icon = match issue.level {
            SqlCheckLevel::Error => "ERROR".red().bold(),
            SqlCheckLevel::Warning => "WARN".yellow().bold(),
        };

        let kind = match issue.kind {
            SqlCheckIssueKind::ParseError => "parse error",
            SqlCheckIssueKind::MissingTable => "missing table",
            SqlCheckIssueKind::MissingColumn => "missing column",
            SqlCheckIssueKind::AmbiguousColumn => "ambiguous column",
            SqlCheckIssueKind::Unsupported => "unsupported",
        };

        let location = issue
            .location
            .map(|loc| format!("@{}", loc))
            .unwrap_or_default();

        println!(
            "  {} [{}]{} {}",
            level_icon,
            kind.cyan(),
            location.dimmed(),
            issue.message.white()
        );

        // Show caret pointing to the error location in the SQL
        if let Some(loc) = issue.location {
            let loc = loc as usize;
            if loc < sql.len() {
                let prefix = " ".repeat(4 + loc);
                println!("  {}{}", prefix, "^".red().bold());
            }
        }
    }
}
