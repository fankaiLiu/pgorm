//! Example demonstrating pgorm's bulk update and delete operations.
//!
//! Run with:
//!   cargo run --example bulk_operations -p pgorm
//!
//! Optional (run queries against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{Condition, FromRow, OrmError, OrmResult, SetExpr, WhereExpr, query, sql};
use std::env;

// ─── FromRow struct for RETURNING queries ───────────────────────────────────

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    name: String,
    status: String,
    login_count: i32,
}

// ─── Pure SQL generation demos (no DB required) ─────────────────────────────

fn demo_update_many_basic() -> OrmResult<()> {
    // UPDATE users SET status = $1 WHERE active = $2
    let builder = sql("users")
        .update_many([SetExpr::set("status", "inactive")?])?
        .filter(Condition::eq("active", false)?);

    // Inspect the generated SQL (access via build_sql for demo purposes)
    let inner_sql = builder.build_sql()?;
    println!("[update_many basic]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_update_many_multi_set() -> OrmResult<()> {
    // UPDATE orders SET status = $1, shipped_at = NOW() WHERE id = ANY($2)
    let builder = sql("orders")
        .update_many([
            SetExpr::set("status", "shipped")?,
            SetExpr::raw("shipped_at = NOW()"),
        ])?
        .filter(Condition::eq_any("id", vec![1_i64, 2, 3])?);

    let inner_sql = builder.build_sql()?;
    println!("[update_many multi-set]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_update_many_increment() -> OrmResult<()> {
    // UPDATE products SET view_count = view_count + 1 WHERE id = $1
    let builder = sql("products")
        .update_many([SetExpr::increment("view_count", 1)?])?
        .filter(Condition::eq("id", 42_i64)?);

    let inner_sql = builder.build_sql()?;
    println!("[update_many increment]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_update_many_decrement() -> OrmResult<()> {
    // UPDATE products SET stock = stock - 3 WHERE id = $1
    let builder = sql("products")
        .update_many([SetExpr::increment("stock", -3)?])?
        .filter(Condition::eq("id", 1_i64)?);

    let inner_sql = builder.build_sql()?;
    println!("[update_many decrement]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_delete_many_basic() -> OrmResult<()> {
    // DELETE FROM sessions WHERE expires_at < NOW()
    let builder = sql("sessions")
        .delete_many()?
        .filter(WhereExpr::raw("expires_at < NOW()"));

    let inner_sql = builder.build_sql()?;
    println!("[delete_many basic]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_delete_many_compound_filter() -> OrmResult<()> {
    // DELETE FROM audit_logs WHERE (level = $1 AND archived = $2)
    let builder = sql("audit_logs")
        .delete_many()?
        .filter(Condition::eq("level", "debug")?)
        .filter(Condition::eq("archived", true)?);

    let inner_sql = builder.build_sql()?;
    println!("[delete_many compound filter]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!("  params: {}", inner_sql.params_ref().len());
    println!();
    Ok(())
}

fn demo_all_rows() -> OrmResult<()> {
    // UPDATE temp_data SET status = $1  (full table, explicit opt-in)
    let builder = sql("temp_data")
        .update_many([SetExpr::set("status", "archived")?])?
        .all_rows();

    let inner_sql = builder.build_sql()?;
    println!("[update_many all_rows]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!();

    // DELETE FROM temp_data  (full table, explicit opt-in)
    let builder = sql("temp_data").delete_many()?.all_rows();

    let inner_sql = builder.build_sql()?;
    println!("[delete_many all_rows]");
    println!("  SQL:    {}", inner_sql.to_sql());
    println!();
    Ok(())
}

fn demo_safety_check() {
    // Without .filter() or .all_rows(), build_sql() returns an error
    let builder = sql("users")
        .update_many([SetExpr::set("status", "x").unwrap()])
        .unwrap();

    match builder.build_sql() {
        Err(e) => println!("[safety] update without filter correctly rejected:\n  {e}\n"),
        Ok(_) => println!("[safety] BUG: should have been rejected\n"),
    }

    let builder = sql("users").delete_many().unwrap();
    match builder.build_sql() {
        Err(e) => println!("[safety] delete without filter correctly rejected:\n  {e}\n"),
        Ok(_) => println!("[safety] BUG: should have been rejected\n"),
    }
}

// ─── Live DB demo ───────────────────────────────────────────────────────────

async fn demo_live(client: &tokio_postgres::Client) -> OrmResult<()> {
    // Setup
    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(client)
        .await?;
    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            login_count INT NOT NULL DEFAULT 0
        )",
    )
    .execute(client)
    .await?;

    // Seed data
    for (name, status, logins) in [
        ("alice", "active", 50),
        ("bob", "active", 3),
        ("carol", "inactive", 0),
        ("dave", "active", 120),
        ("eve", "banned", 10),
    ] {
        query("INSERT INTO users (name, status, login_count) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(status)
            .bind(logins)
            .execute(client)
            .await?;
    }
    println!("=== Live DB demo ===\n");
    println!("Seeded 5 users.\n");

    // 1. Bulk update: deactivate users with few logins
    let affected = sql("users")
        .update_many([SetExpr::set("status", "inactive")?])?
        .filter(Condition::lt("login_count", 10_i32)?)
        .execute(client)
        .await?;
    println!("[live] Deactivated users with < 10 logins: {affected} rows affected");

    // 2. Bulk update with increment + RETURNING
    let updated: Vec<User> = sql("users")
        .update_many([SetExpr::increment("login_count", 1)?])?
        .filter(Condition::eq("status", "active")?)
        .returning(client)
        .await?;
    println!(
        "[live] Incremented login_count for active users, {} returned:",
        updated.len()
    );
    for u in &updated {
        println!("       id={} name={} status={} login_count={}", u.id, u.name, u.status, u.login_count);
    }

    // 3. Bulk delete with RETURNING
    let deleted: Vec<User> = sql("users")
        .delete_many()?
        .filter(Condition::eq("status", "banned")?)
        .returning(client)
        .await?;
    println!(
        "[live] Deleted banned users, {} returned:",
        deleted.len()
    );
    for u in &deleted {
        println!("       id={} name={} status={}", u.id, u.name, u.status);
    }

    // 4. Show final state
    let remaining: i64 = sql("SELECT COUNT(*) FROM users")
        .fetch_scalar_one(client)
        .await?;
    println!("\n[live] Remaining users: {remaining}");

    Ok(())
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    println!("=== Bulk Operations Examples ===\n");

    // SQL generation demos (no DB needed)
    demo_update_many_basic()?;
    demo_update_many_multi_set()?;
    demo_update_many_increment()?;
    demo_update_many_decrement()?;
    demo_delete_many_basic()?;
    demo_delete_many_compound_filter()?;
    demo_all_rows()?;
    demo_safety_check();

    // Live DB demo
    let database_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("DATABASE_URL not set; skipping live DB demo.");
            return Ok(());
        }
    };

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    demo_live(&client).await?;

    Ok(())
}
