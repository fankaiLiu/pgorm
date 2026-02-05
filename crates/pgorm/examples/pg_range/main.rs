//! Example demonstrating Range<T> type for PostgreSQL range columns.
//!
//! Run with:
//!   cargo run --example pg_range -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use chrono::{DateTime, Duration, NaiveDate, Utc};
use pgorm::types::Range;
use pgorm::{Condition, OrmError, OrmResult, RowExt, query, sql};
use std::env;

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

    // ── Setup ────────────────────────────────────────────────────────────────
    query("DROP TABLE IF EXISTS events CASCADE")
        .execute(&client)
        .await?;
    query("DROP TABLE IF EXISTS bookings CASCADE")
        .execute(&client)
        .await?;

    query(
        "CREATE TABLE events (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            during TSTZRANGE NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    query(
        "CREATE TABLE bookings (
            id BIGSERIAL PRIMARY KEY,
            room TEXT NOT NULL,
            reserved DATERANGE NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    // ── Insert events with tstzrange ─────────────────────────────────────────
    let now: DateTime<Utc> = Utc::now();

    query("INSERT INTO events (name, during) VALUES ($1, $2)")
        .bind("Team Meeting")
        .bind(Range::lower_inc(now, now + Duration::hours(2)))
        .execute(&client)
        .await?;

    query("INSERT INTO events (name, during) VALUES ($1, $2)")
        .bind("Workshop")
        .bind(Range::lower_inc(
            now + Duration::hours(3),
            now + Duration::hours(6),
        ))
        .execute(&client)
        .await?;

    query("INSERT INTO events (name, during) VALUES ($1, $2)")
        .bind("All-Day Conference")
        .bind(Range::lower_inc(
            now - Duration::hours(12),
            now + Duration::hours(12),
        ))
        .execute(&client)
        .await?;

    println!("[1] Inserted 3 events");

    // ── Read back ranges ─────────────────────────────────────────────────────
    let rows = query("SELECT id, name, during FROM events ORDER BY id")
        .fetch_all(&client)
        .await?;
    println!("\n[2] All events:");
    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let name: String = row.try_get_column("name")?;
        let during: Range<DateTime<Utc>> = row.try_get_column("during")?;
        let lower = during
            .lower
            .as_ref()
            .map(|b| format!("{}", b.value().format("%H:%M")))
            .unwrap_or_else(|| "-inf".into());
        let upper = during
            .upper
            .as_ref()
            .map(|b| format!("{}", b.value().format("%H:%M")))
            .unwrap_or_else(|| "+inf".into());
        println!("    id={id} name={name:20} during=[{lower}, {upper})");
    }

    // ── Overlap query (&&) ───────────────────────────────────────────────────
    // Find events that overlap with the next hour
    let query_range = Range::lower_inc(now, now + Duration::hours(1));
    let mut q = sql("SELECT id, name FROM events");
    q.push(" WHERE ");
    Condition::overlaps("during", query_range)?.append_to_sql(&mut q);
    let overlapping = q.fetch_all(&client).await?;
    println!(
        "\n[3] Events overlapping with the next hour ({} found):",
        overlapping.len()
    );
    for row in &overlapping {
        let name: String = row.try_get_column("name")?;
        println!("    {name}");
    }

    // ── Contains query (@>) ──────────────────────────────────────────────────
    // Find events that contain the current timestamp
    let mut q = sql("SELECT id, name FROM events");
    q.push(" WHERE ");
    Condition::contains("during", now)?.append_to_sql(&mut q);
    let containing = q.fetch_all(&client).await?;
    println!(
        "\n[4] Events containing 'now' ({} found):",
        containing.len()
    );
    for row in &containing {
        let name: String = row.try_get_column("name")?;
        println!("    {name}");
    }

    // ── Date range example ───────────────────────────────────────────────────
    let today = Utc::now().date_naive();

    query("INSERT INTO bookings (room, reserved) VALUES ($1, $2)")
        .bind("Room A")
        .bind(Range::lower_inc(today, today + chrono::Days::new(3)))
        .execute(&client)
        .await?;

    query("INSERT INTO bookings (room, reserved) VALUES ($1, $2)")
        .bind("Room B")
        .bind(Range::lower_inc(
            today + chrono::Days::new(1),
            today + chrono::Days::new(5),
        ))
        .execute(&client)
        .await?;

    query("INSERT INTO bookings (room, reserved) VALUES ($1, $2)")
        .bind("Room A")
        .bind(Range::lower_inc(
            today + chrono::Days::new(5),
            today + chrono::Days::new(7),
        ))
        .execute(&client)
        .await?;

    println!("\n[5] Inserted 3 bookings");

    // Read back date ranges
    let rows = query("SELECT room, reserved FROM bookings ORDER BY id")
        .fetch_all(&client)
        .await?;
    println!("\n[6] All bookings:");
    for row in &rows {
        let room: String = row.try_get_column("room")?;
        let reserved: Range<NaiveDate> = row.try_get_column("reserved")?;
        let from = reserved
            .lower
            .as_ref()
            .map(|b| b.value().to_string())
            .unwrap_or_else(|| "-inf".into());
        let to = reserved
            .upper
            .as_ref()
            .map(|b| b.value().to_string())
            .unwrap_or_else(|| "+inf".into());
        println!("    room={room:8} reserved=[{from}, {to})");
    }

    // ── Range constructors showcase ──────────────────────────────────────────
    println!("\n[7] Range constructors:");
    let r = Range::<i32>::inclusive(1, 10);
    println!(
        "    inclusive(1,10) lower={:?} upper={:?}",
        r.lower, r.upper
    );
    let r = Range::<i32>::exclusive(1, 10);
    println!(
        "    exclusive(1,10) lower={:?} upper={:?}",
        r.lower, r.upper
    );
    let r = Range::<i32>::lower_inc(1, 10);
    println!(
        "    lower_inc(1,10) lower={:?} upper={:?}",
        r.lower, r.upper
    );
    let r = Range::<i32>::empty();
    println!("    empty()         is_empty={}", r.is_empty());
    let r = Range::<i32>::unbounded();
    println!(
        "    unbounded()     lower={:?} upper={:?}",
        r.lower, r.upper
    );

    println!("\nDone.");
    Ok(())
}
