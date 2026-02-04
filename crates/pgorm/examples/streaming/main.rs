use futures_util::StreamExt;
use pgorm::{FromRow, OrmResult, query};
use std::time::Duration;
use tokio_postgres::NoTls;

#[derive(Debug, FromRow)]
struct Item {
    n: i64,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is required, e.g. postgres://postgres:postgres@localhost:5432/postgres");

    let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .map_err(pgorm::OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let mut stream = query("SELECT generate_series(1, $1) AS n")
        .bind(10_i64)
        .tag("examples.streaming.generate_series")
        .stream_as::<Item>(&client)
        .await?;

    while let Some(item) = stream.next().await {
        let item = item?;
        println!("{}", item.n);

        // Simulate slow consumer to demonstrate backpressure.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
