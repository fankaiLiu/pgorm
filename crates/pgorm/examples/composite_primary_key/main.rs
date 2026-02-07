//! Example demonstrating composite primary key support with `#[derive(Model)]`.
//!
//! Run with:
//!   cargo run --example composite_primary_key -p pgorm
//!
//! Optional (run against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, Model, OrmError, OrmResult, TableMeta, query};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "enrollments")]
#[allow(dead_code)]
struct Enrollment {
    #[orm(id)]
    user_id: i64,
    #[orm(id)]
    course_id: i64,
    status: String,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    println!(
        "primary keys: {:?}",
        <Enrollment as TableMeta>::primary_keys()
    );
    println!("IDS const: {:?}", Enrollment::IDS);

    let database_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("DATABASE_URL not set; skipping DB execution.");
            return Ok(());
        }
    };

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS enrollments CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE enrollments (
            user_id BIGINT NOT NULL,
            course_id BIGINT NOT NULL,
            status TEXT NOT NULL,
            PRIMARY KEY (user_id, course_id)
        )",
    )
    .execute(&client)
    .await?;

    query("INSERT INTO enrollments (user_id, course_id, status) VALUES ($1, $2, $3)")
        .bind(1_i64)
        .bind(101_i64)
        .bind("active")
        .execute(&client)
        .await?;
    query("INSERT INTO enrollments (user_id, course_id, status) VALUES ($1, $2, $3)")
        .bind(1_i64)
        .bind(102_i64)
        .bind("dropped")
        .execute(&client)
        .await?;

    let row = Enrollment::select_by_pk(&client, 1_i64, 101_i64).await?;
    println!("select_by_pk => {:?}", row);

    let deleted = Enrollment::delete_by_pk_returning(&client, 1_i64, 102_i64).await?;
    println!("delete_by_pk_returning => {:?}", deleted);

    let affected = Enrollment::delete_by_pk(&client, 1_i64, 101_i64).await?;
    println!("delete_by_pk affected rows => {affected}");

    Ok(())
}
