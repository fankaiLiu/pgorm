//! Example demonstrating `#[derive(QueryParams)]` for reusing filters between `search` and `count`.
//!
//! Run with:
//!   cargo run --example query_params -p pgorm
//!
//! Optional (run the query against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use pgorm::{FromRow, Model, OrmError, OrmResult, QueryParams, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "audit_logs")]
struct AuditLog {
    #[orm(id)]
    id: i64,
    user_id: uuid::Uuid,
    operation_type: String,
    created_at: DateTime<Utc>,
    ip_address: Option<std::net::IpAddr>,
    status_code: i16,
}

fn parse_ip(s: &str) -> Option<std::net::IpAddr> {
    s.parse().ok()
}

#[derive(QueryParams)]
#[orm(model = "AuditLog")]
struct AuditLogSearchParams<'a> {
    // WHERE
    #[orm(eq(AuditLogQuery::COL_USER_ID))]
    user_id: Option<uuid::Uuid>,

    #[orm(eq(AuditLogQuery::COL_OPERATION_TYPE))]
    operation_type: Option<&'a str>,

    #[orm(gte(AuditLogQuery::COL_CREATED_AT))]
    start_date: Option<DateTime<Utc>>,

    #[orm(lte(AuditLogQuery::COL_CREATED_AT))]
    end_date: Option<DateTime<Utc>>,

    #[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
    ip_address: Option<&'a str>,

    #[orm(in_list(AuditLogQuery::COL_STATUS_CODE))]
    status_any: Option<Vec<i16>>,

    // ORDER BY
    #[orm(order_by_desc)]
    order_by_desc: Option<&'a str>,

    // Pagination (page-based)
    #[orm(page(per_page = per_page.unwrap_or(10)))]
    page: Option<i64>,

    per_page: Option<i64>,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    // In a real API, you'd build this from request inputs.
    let params = AuditLogSearchParams {
        user_id: Some(uuid::Uuid::nil()),
        operation_type: Some("LOGIN"),
        start_date: None,
        end_date: None,
        ip_address: Some("127.0.0.1"),
        status_any: Some(vec![200, 201, 204]),
        order_by_desc: Some("created_at"),
        page: Some(1),
        per_page: Some(5),
    };

    // This is the reusable query builder: use it for both list + count.
    let q = params.into_query()?;
    println!("built query builder = {q:?}");

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
    query("DROP TABLE IF EXISTS audit_logs CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE audit_logs (
            id BIGSERIAL PRIMARY KEY,
            user_id UUID NOT NULL,
            operation_type TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            ip_address INET,
            status_code SMALLINT NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    let user_id = uuid::Uuid::nil();
    query("INSERT INTO audit_logs (user_id, operation_type, created_at, ip_address, status_code) VALUES ($1, $2, NOW(), $3, $4)")
        .bind(user_id)
        .bind("LOGIN")
        .bind("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        .bind(200_i16)
        .execute(&client)
        .await?;
    query("INSERT INTO audit_logs (user_id, operation_type, created_at, ip_address, status_code) VALUES ($1, $2, NOW(), $3, $4)")
        .bind(user_id)
        .bind("LOGIN")
        .bind("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        .bind(204_i16)
        .execute(&client)
        .await?;
    query("INSERT INTO audit_logs (user_id, operation_type, created_at, ip_address, status_code) VALUES ($1, $2, NOW(), $3, $4)")
        .bind(user_id)
        .bind("LOGOUT")
        .bind("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        .bind(200_i16)
        .execute(&client)
        .await?;

    // Rebuild query after seeding (same params).
    let q = AuditLogSearchParams {
        user_id: Some(uuid::Uuid::nil()),
        operation_type: Some("LOGIN"),
        start_date: None,
        end_date: None,
        ip_address: Some("127.0.0.1"),
        status_any: Some(vec![200, 201, 204]),
        order_by_desc: Some("created_at"),
        page: Some(1),
        per_page: Some(10),
    }
    .into_query()?;

    let rows = q.find(&client).await?;
    let total = q.count(&client).await?;

    println!("total = {total}");
    for row in rows {
        println!("row = {row:?}");
    }

    Ok(())
}
