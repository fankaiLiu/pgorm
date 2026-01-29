//! SQL vs schema check example (with local schema cache)
//!
//! Run with:
//! `cargo run --example sql_check -p pgorm-check --features sql`
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use colored::Colorize;
use deadpool_postgres::{Config, Runtime};
use pgorm_check::{
    CheckClient, CheckError, SchemaCache, SchemaCacheConfig, SchemaCacheLoad, SqlCheckIssueKind,
    SqlCheckLevel, check_sql_cached,
};
use std::{env, path::PathBuf};
use tokio_postgres::NoTls;

// Implement CheckClient for deadpool's pooled client
struct PoolClient(deadpool_postgres::Client);

#[async_trait::async_trait]
impl CheckClient for PoolClient {
    async fn query(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> pgorm_check::CheckResult<Vec<tokio_postgres::Row>> {
        self.0.query(sql, params).await.map_err(CheckError::from)
    }

    async fn query_one(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> pgorm_check::CheckResult<tokio_postgres::Row> {
        self.0
            .query_one(sql, params)
            .await
            .map_err(CheckError::from)
    }

    async fn execute(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> pgorm_check::CheckResult<u64> {
        self.0.execute(sql, params).await.map_err(CheckError::from)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment variable");

    // Create connection pool
    let mut cfg = Config::new();
    cfg.url = Some(database_url);
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
    let client = PoolClient(pool.get().await?);

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
        .await?;

    // Cache schema under the current working directory (configurable).
    let mut config = SchemaCacheConfig::default();
    config.cache_dir = PathBuf::from(".pgorm");
    config.schemas = vec!["public".to_string()];

    println!(
        "{} {}",
        "Cache path:".dimmed(),
        SchemaCache::cache_path(&config)
            .display()
            .to_string()
            .cyan()
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
