//! Example demonstrating pgorm's SQL builder + condition helpers.
//!
//! Run with:
//!   cargo run --example sql_builder -p pgorm
//!
//! Optional (run the query against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{Condition, NullsOrder, Op, OrderBy, OrmError, OrmResult, Pagination, RowExt, WhereExpr, query, sql};
use std::env;

#[derive(Debug)]
struct Filters {
    status: Option<String>,
    search: Option<String>,
    roles_any_of: Vec<String>,
    include_deleted: bool,
    page: i64,
    per_page: i64,
    sort_by: Option<String>,
}

fn build_list_users_sql(filters: &Filters) -> OrmResult<pgorm::Sql> {
    let mut where_expr = WhereExpr::and(Vec::new());

    if let Some(status) = &filters.status {
        where_expr =
            where_expr.and_with(WhereExpr::atom(Condition::eq("status", status.clone())?));
    }

    if let Some(search) = &filters.search {
        where_expr = where_expr.and_with(WhereExpr::atom(Condition::ilike(
            "name",
            format!("%{search}%"),
        )?));
    }

    if !filters.roles_any_of.is_empty() {
        let mut roles = Vec::with_capacity(filters.roles_any_of.len());
        for role in &filters.roles_any_of {
            roles.push(WhereExpr::atom(Condition::eq("role", role.clone())?));
        }
        where_expr = where_expr.and_with(WhereExpr::or(roles));
    }

    if !filters.include_deleted {
        where_expr = where_expr.and_with(WhereExpr::atom(Condition::is_null("deleted_at")?));
    }

    let mut q = sql("SELECT id, name, status, role, created_at FROM users");

    if !where_expr.is_trivially_true() {
        q.push(" WHERE ");
        where_expr.append_to_sql(&mut q);
    }

    // Safe dynamic ORDER BY (identifiers are validated).
    let mut order = OrderBy::new().with_nulls("created_at", pgorm::SortDir::Desc, NullsOrder::Last)?;
    if let Some(sort_by) = &filters.sort_by {
        order = order.asc(sort_by.as_str())?;
    }
    order.append_to_sql(&mut q);

    Pagination::page(filters.page, filters.per_page)?.append_to_sql(&mut q);

    Ok(q)
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    let filters = Filters {
        status: Some("active".to_string()),
        search: Some("a".to_string()),
        roles_any_of: vec!["admin".to_string(), "owner".to_string()],
        include_deleted: false,
        page: 1,
        per_page: 10,
        sort_by: Some("id".to_string()),
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

    // Setup schema + seed data (idempotent for the demo).
    query("DROP TABLE IF EXISTS users CASCADE").execute(&client).await?;
    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            role TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            deleted_at TIMESTAMPTZ
        )",
    )
    .execute(&client)
    .await?;

    for (name, status, role, deleted) in [
        ("alice", "active", "admin", false),
        ("bob", "active", "owner", false),
        ("carol", "disabled", "admin", false),
        ("dave", "active", "member", true),
    ] {
        let deleted_at = if deleted { Some(chrono::Utc::now()) } else { None };
        query(
            "INSERT INTO users (name, status, role, deleted_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(name)
        .bind(status)
        .bind(role)
        .bind(deleted_at)
        .execute(&client)
        .await?;
    }

    // Execute the built SQL.
    let rows = q.fetch_all(&client).await?;
    println!("\nmatched rows = {}", rows.len());

    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let name: String = row.try_get_column("name")?;
        let status: String = row.try_get_column("status")?;
        let role: String = row.try_get_column("role")?;
        println!("- id={id} name={name} status={status} role={role}");
    }

    // A more complex condition tree example: (status=active) AND (role=admin OR role=owner) AND id BETWEEN 1 AND 100
    let complex = WhereExpr::and(vec![
        Condition::eq("status", "active")?.into(),
        WhereExpr::or(vec![
            Condition::eq("role", "admin")?.into(),
            Condition::eq("role", "owner")?.into(),
        ]),
        Condition::new("id", Op::between(1_i64, 100_i64))?.into(),
    ]);

    let mut q2 = sql("SELECT COUNT(*) FROM users WHERE ");
    complex.append_to_sql(&mut q2);
    let count: i64 = q2.fetch_scalar_one(&client).await?;
    println!("\ncomplex filter count = {count}");

    Ok(())
}
