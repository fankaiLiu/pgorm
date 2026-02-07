//! Example demonstrating migration toolchain helpers (`status/up/down/diff`).
//!
//! Run with:
//!   cargo run --example migration_toolchain -p pgorm --features migrate
//!
//! Optional (run against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, migrate};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn make_temp_migration_dir() -> OrmResult<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| OrmError::Other(format!("clock error: {e}")))?
        .as_nanos();

    let dir = env::temp_dir().join(format!("pgorm-migrate-example-{nonce}"));
    fs::create_dir_all(&dir)
        .map_err(|e| OrmError::Other(format!("failed to create {}: {e}", dir.display())))?;

    fs::write(
        dir.join("V1__create_notes.up.sql"),
        "CREATE TABLE IF NOT EXISTS notes (id BIGSERIAL PRIMARY KEY, body TEXT NOT NULL);",
    )
    .map_err(|e| OrmError::Other(format!("failed to write up sql: {e}")))?;
    fs::write(
        dir.join("V1__create_notes.down.sql"),
        "DROP TABLE IF EXISTS notes;",
    )
    .map_err(|e| OrmError::Other(format!("failed to write down sql: {e}")))?;

    fs::write(
        dir.join("V2__seed_notes.up.sql"),
        "INSERT INTO notes (body) VALUES ('hello from migration toolchain');",
    )
    .map_err(|e| OrmError::Other(format!("failed to write up sql: {e}")))?;
    fs::write(
        dir.join("V2__seed_notes.down.sql"),
        "DELETE FROM notes WHERE body = 'hello from migration toolchain';",
    )
    .map_err(|e| OrmError::Other(format!("failed to write down sql: {e}")))?;

    Ok(dir)
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    let migrations_dir = make_temp_migration_dir()?;

    let local = migrate::scan_migrations_dir(&migrations_dir)?;
    println!("local migrations in {}:", migrations_dir.display());
    for m in &local {
        println!("  V{}__{}", m.version, m.name);
    }

    let db_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("DATABASE_URL not set; skipping database execution.");
            return Ok(());
        }
    };

    let (mut client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let before = migrate::status(&mut client, &migrations_dir).await?;
    println!("before: pending={}", before.pending.len());

    let report = migrate::up_dir(&mut client, &migrations_dir).await?;
    println!("applied {} migration(s)", report.applied_migrations().len());

    let draft = migrate::diff_pending_sql(&mut client, &migrations_dir).await?;
    println!("pending diff draft:\n{}", draft.trim_end());

    let rolled = migrate::down_steps(&mut client, &migrations_dir, 1).await?;
    println!("rolled back {} migration(s)", rolled.len());

    Ok(())
}
