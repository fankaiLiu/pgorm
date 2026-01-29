//! Run embedded SQL migrations using `pgorm` + `refinery`.
//!
//! Run with:
//! `cargo run --example migrate -p pgorm --features migrate`
//!
//! Set `DATABASE_URL` in `.env` or environment variable:
//! `DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example`

use pgorm::{create_pool, migrate};
use std::env;

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./examples/migrations");
}

#[tokio::main]
async fn main() -> pgorm::OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| pgorm::OrmError::Connection("DATABASE_URL is not set".to_string()))?;

    let pool = create_pool(&database_url)?;
    let report = migrate::run_pool(&pool, embedded::migrations::runner()).await?;

    println!(
        "Applied {} migration(s)",
        report.applied_migrations().len()
    );
    Ok(())
}

