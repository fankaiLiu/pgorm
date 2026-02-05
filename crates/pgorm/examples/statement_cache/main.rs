//! Example demonstrating PgClient prepared statement cache.
//!
//! Run with: `cargo run --example statement_cache -p pgorm`
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres

use pgorm::{OrmResult, PgClient, PgClientConfig, query};
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() -> OrmResult<()> {
    let database_url = std::env::var("DATABASE_URL").expect(
        "DATABASE_URL is required, e.g. postgres://postgres:postgres@localhost:5432/postgres",
    );

    let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .map_err(pgorm::OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    // Prepared statements are per-connection, so the cache also lives per connection.
    let pg = PgClient::with_config(client, PgClientConfig::new().no_check().statement_cache(64));

    let sql = "SELECT $1::bigint + $2::bigint";
    let tag = "examples.statement_cache.add";

    // Phase 1: first run prepares once, then hits cache.
    let n = 100_u64;
    for i in 0..n {
        let _v: i64 = query(sql)
            .tag(tag)
            .bind(i as i64)
            .bind(1_i64)
            .fetch_scalar_one(&pg)
            .await?;
    }
    let s1 = pg.stats();
    println!(
        "phase1: hits={}, misses={}, prepares={}, prepare_time={:?}",
        s1.stmt_cache_hits, s1.stmt_cache_misses, s1.stmt_prepare_count, s1.stmt_prepare_duration
    );

    // Phase 2: same SQL again (should be all hits; no prepares).
    pg.reset_stats();
    for i in 0..n {
        let _v: i64 = query(sql)
            .tag(tag)
            .bind(i as i64)
            .bind(1_i64)
            .fetch_scalar_one(&pg)
            .await?;
    }
    let s2 = pg.stats();
    println!(
        "phase2: hits={}, misses={}, prepares={}, prepare_time={:?}",
        s2.stmt_cache_hits, s2.stmt_cache_misses, s2.stmt_prepare_count, s2.stmt_prepare_duration
    );

    Ok(())
}
