//! Example demonstrating InsertModel UPSERT helpers (ON CONFLICT).
//!
//! Run with:
//!   cargo run --example upsert -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, InsertModel, Model, OrmError, OrmResult, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "tags")]
#[allow(dead_code)]
struct Tag {
    #[orm(id)]
    id: i64,
    name: String,
    color: Option<String>,
}

/// UPSERT using a conflict target (columns).
#[derive(Debug, Clone, InsertModel)]
#[orm(
    table = "tags",
    returning = "Tag",
    conflict_target = "name",
    conflict_update = "color"
)]
struct TagUpsertByTarget {
    name: String,
    color: Option<String>,
}

/// UPSERT using a named constraint.
#[derive(Debug, Clone, InsertModel)]
#[orm(
    table = "tags",
    returning = "Tag",
    conflict_constraint = "tags_name_unique",
    conflict_update = "color"
)]
struct TagUpsertByConstraint {
    name: String,
    color: Option<String>,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS tags CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE tags (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            color TEXT,
            CONSTRAINT tags_name_unique UNIQUE (name)
        )",
    )
    .execute(&client)
    .await?;

    let rust_1 = TagUpsertByTarget {
        name: "rust".into(),
        color: Some("orange".into()),
    }
    .upsert_returning(&client)
    .await?;
    println!("upsert (target) inserted: {rust_1:?}");

    let rust_2 = TagUpsertByTarget {
        name: "rust".into(),
        color: Some("brown".into()),
    }
    .upsert_returning(&client)
    .await?;
    println!("upsert (target) updated:  {rust_2:?}");

    let go = TagUpsertByConstraint {
        name: "go".into(),
        color: Some("blue".into()),
    }
    .upsert_returning(&client)
    .await?;
    println!("upsert (constraint) inserted: {go:?}");

    let batch = TagUpsertByTarget::upsert_many_returning(
        &client,
        vec![
            TagUpsertByTarget {
                name: "rust".into(),
                color: Some("red".into()),
            },
            TagUpsertByTarget {
                name: "zig".into(),
                color: None,
            },
        ],
    )
    .await?;
    println!("\nbatch upsert returned {} row(s):", batch.len());
    for t in &batch {
        println!("- {t:?}");
    }

    let all: Vec<Tag> = pgorm::query("SELECT id, name, color FROM tags ORDER BY id")
        .fetch_all_as(&client)
        .await?;
    println!("\nall tags:");
    for t in &all {
        println!("- {t:?}");
    }

    Ok(())
}
