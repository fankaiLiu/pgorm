//! Example demonstrating keyset/cursor pagination.
//!
//! Run with:
//!   cargo run --example keyset_pagination -p pgorm
//!
//! Optional (run the query against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{Condition, Keyset2, OrmError, OrmResult, WhereExpr, query, sql};
use std::env;

#[derive(Debug)]
struct Filters {
    status: Option<String>,
    after_created_at: Option<i64>,
    after_id: Option<i64>,
    limit: i64,
}

fn build_list_users_sql(filters: &Filters) -> OrmResult<pgorm::Sql> {
    let mut where_expr = WhereExpr::and(Vec::new());

    if let Some(status) = &filters.status {
        where_expr = where_expr.and_with(Condition::eq("status", status.clone())?.into());
    }

    // Stable order: created_at DESC, id DESC (tie-breaker).
    let mut keyset = Keyset2::desc("created_at", "id")?.limit(filters.limit);
    if let (Some(created_at), Some(id)) = (filters.after_created_at, filters.after_id) {
        keyset = keyset.after(created_at, id);
        where_expr = where_expr.and_with(keyset.into_where_expr()?);
    }

    let mut q = sql("SELECT id, name, status, created_at FROM users");
    if !where_expr.is_trivially_true() {
        q.push(" WHERE ");
        where_expr.append_to_sql(&mut q);
    }
    keyset.append_order_by_limit_to_sql(&mut q)?;

    Ok(q)
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    let filters = Filters {
        status: Some("active".to_string()),
        after_created_at: Some(1_700_000_002),
        after_id: Some(2),
        limit: 10,
    };

    let q = build_list_users_sql(&filters)?;
    println!("built sql:\n{}\n", q.to_sql());
    println!("params = {}", q.params_ref().len());

    let database_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("\nDATABASE_URL not set; skipping DB execution.");
            return Ok(());
        }
    };

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    // Minimal schema for the demo.
    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at BIGINT NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    for (name, status, created_at) in [
        ("alice", "active", 1_700_000_003_i64),
        ("bob", "active", 1_700_000_002_i64),
        ("carol", "disabled", 1_700_000_001_i64),
    ] {
        query("INSERT INTO users (name, status, created_at) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(status)
            .bind(created_at)
            .execute(&client)
            .await?;
    }

    let rows = q.fetch_all(&client).await?;
    println!("\nmatched rows = {}", rows.len());

    Ok(())
}
