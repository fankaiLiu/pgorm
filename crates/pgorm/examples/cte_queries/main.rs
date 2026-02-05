//! Example demonstrating pgorm's CTE (WITH clause) query support.
//!
//! Run with:
//!   cargo run --example cte_queries -p pgorm
//!
//! Optional (run queries against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, OrmError, OrmResult, query, sql};
use std::env;

// ─── FromRow structs for typed results ──────────────────────────────────────

#[derive(Debug, FromRow)]
struct UserOrderStats {
    user_id: i64,
    name: String,
    total_orders: i64,
    total_amount: i64,
}

#[derive(Debug, FromRow)]
struct OrgNode {
    id: i64,
    name: String,
    #[orm(column = "parent_id")]
    parent_id: Option<i64>,
    level: i32,
    path: String,
}

#[derive(Debug, FromRow)]
struct CategoryNode {
    id: i64,
    name: String,
    depth: i32,
}

// ─── Pure SQL generation demos (no DB required) ─────────────────────────────

fn demo_simple_cte() -> OrmResult<()> {
    // WITH active_users AS (
    //   SELECT id, name FROM users WHERE status = $1
    // )
    // SELECT * FROM active_users
    let q = sql("")
        .with(
            "active_users",
            sql("SELECT id, name FROM users WHERE status = ").bind("active"),
        )?
        .select(sql("SELECT * FROM active_users"));

    println!("[simple CTE]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_multiple_ctes() -> OrmResult<()> {
    // WITH
    //   active_users AS (SELECT id, name FROM users WHERE status = $1),
    //   recent_orders AS (SELECT user_id, amount FROM orders WHERE amount > $2)
    // SELECT u.id, u.name, SUM(o.amount) as total
    // FROM active_users u
    // JOIN recent_orders o ON o.user_id = u.id
    // GROUP BY u.id, u.name
    let q = sql("")
        .with(
            "active_users",
            sql("SELECT id, name FROM users WHERE status = ").bind("active"),
        )?
        .with(
            "recent_orders",
            sql("SELECT user_id, amount FROM orders WHERE amount > ").bind(100_i64),
        )?
        .select(sql(
            "SELECT u.id, u.name, SUM(o.amount) as total \
             FROM active_users u \
             JOIN recent_orders o ON o.user_id = u.id \
             GROUP BY u.id, u.name",
        ));

    println!("[multiple CTEs]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_recursive_cte() -> OrmResult<()> {
    // WITH RECURSIVE org_tree AS (
    //   SELECT id, name, parent_id, 0 as level, name::text as path
    //   FROM employees WHERE parent_id IS NULL
    //   UNION ALL
    //   SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name
    //   FROM employees e JOIN org_tree t ON e.parent_id = t.id
    // )
    // SELECT * FROM org_tree ORDER BY path
    let q = sql("")
        .with_recursive(
            "org_tree",
            sql("SELECT id, name, parent_id, 0 as level, name::text as path \
                 FROM employees WHERE parent_id IS NULL"),
            sql("SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name \
                 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
        )?
        .select(sql("SELECT * FROM org_tree ORDER BY path"));

    println!("[recursive CTE — org tree]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_recursive_cte_with_params() -> OrmResult<()> {
    // WITH RECURSIVE category_tree AS (
    //   SELECT id, name, parent_id, 0 as depth FROM categories WHERE id = $1
    //   UNION ALL
    //   SELECT c.id, c.name, c.parent_id, ct.depth + 1
    //   FROM categories c JOIN category_tree ct ON c.parent_id = ct.id
    //   WHERE ct.depth < $2
    // )
    // SELECT * FROM category_tree
    let q = sql("")
        .with_recursive(
            "category_tree",
            sql("SELECT id, name, parent_id, 0 as depth FROM categories WHERE id = ").bind(1_i64),
            sql("SELECT c.id, c.name, c.parent_id, ct.depth + 1 \
                 FROM categories c JOIN category_tree ct ON c.parent_id = ct.id \
                 WHERE ct.depth < ")
                .bind(5_i32),
        )?
        .select_from("category_tree")?;

    println!("[recursive CTE — category tree with depth limit]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_cte_with_columns() -> OrmResult<()> {
    // WITH monthly_sales(month, total) AS (
    //   SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1
    // )
    // SELECT * FROM monthly_sales WHERE total > $1
    let q = sql("")
        .with_columns(
            "monthly_sales",
            ["month", "total"],
            sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
        )?
        .select(sql("SELECT * FROM monthly_sales WHERE total > ").bind(10000_i64));

    println!("[CTE with column names]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_cte_data_modification() -> OrmResult<()> {
    // WITH deleted_orders AS (
    //   DELETE FROM orders WHERE status = 'cancelled' AND created_at < $1 RETURNING *
    // )
    // INSERT INTO orders_archive SELECT * FROM deleted_orders
    let mut delete_sql = sql("DELETE FROM orders WHERE status = 'cancelled' AND created_at < ");
    delete_sql.push_bind("2024-01-01");
    delete_sql.push(" RETURNING *");

    let q = sql("")
        .with("deleted_orders", delete_sql)?
        .select(sql(
            "INSERT INTO orders_archive SELECT * FROM deleted_orders",
        ));

    println!("[CTE data modification — archive deleted orders]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

fn demo_recursive_union_dedup() -> OrmResult<()> {
    // WITH RECURSIVE reachable AS (
    //   SELECT end_node FROM edges WHERE start_node = $1
    //   UNION
    //   SELECT e.end_node FROM edges e JOIN reachable r ON e.start_node = r.end_node
    // )
    // SELECT * FROM reachable
    let q = sql("")
        .with_recursive_union(
            "reachable",
            sql("SELECT end_node FROM edges WHERE start_node = ").bind(1_i64),
            sql("SELECT e.end_node FROM edges e JOIN reachable r ON e.start_node = r.end_node"),
        )?
        .select_from("reachable")?;

    println!("[recursive CTE — UNION (dedup) graph traversal]");
    println!("  SQL:    {}", q.to_sql());
    println!("  params: {}", q.params_ref().len());
    println!();
    Ok(())
}

// ─── Live DB demo ───────────────────────────────────────────────────────────

async fn demo_live(client: &tokio_postgres::Client) -> OrmResult<()> {
    println!("=== Live DB demo ===\n");

    // ── Setup: users + orders ──

    query("DROP TABLE IF EXISTS orders CASCADE")
        .execute(client)
        .await?;
    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(client)
        .await?;
    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active'
        )",
    )
    .execute(client)
    .await?;
    query(
        "CREATE TABLE orders (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id),
            amount BIGINT NOT NULL
        )",
    )
    .execute(client)
    .await?;

    // Seed
    for name in ["alice", "bob", "carol"] {
        query("INSERT INTO users (name) VALUES ($1)")
            .bind(name)
            .execute(client)
            .await?;
    }
    for (user_id, amount) in [
        (1_i64, 500_i64),
        (1, 300),
        (2, 1200),
        (2, 800),
        (3, 50),
    ] {
        query("INSERT INTO orders (user_id, amount) VALUES ($1, $2)")
            .bind(user_id)
            .bind(amount)
            .execute(client)
            .await?;
    }
    println!("Seeded 3 users and 5 orders.\n");

    // ── Demo 1: Simple CTE — order stats ──

    let stats: Vec<UserOrderStats> = sql("")
        .with(
            "order_stats",
            sql("SELECT user_id, COUNT(*)::bigint as total_orders, SUM(amount)::bigint as total_amount FROM orders GROUP BY user_id"),
        )?
        .select(sql(
            "SELECT os.user_id, u.name, os.total_orders, os.total_amount \
             FROM order_stats os \
             JOIN users u ON u.id = os.user_id \
             ORDER BY os.total_amount DESC",
        ))
        .fetch_all_as(client)
        .await?;

    println!("[live] User order stats (via CTE):");
    for s in &stats {
        println!(
            "       user_id={} name={} orders={} total_amount={}",
            s.user_id, s.name, s.total_orders, s.total_amount
        );
    }
    println!();

    // ── Demo 2: CTE with filter — high-value users ──

    let high_value: Vec<UserOrderStats> = sql("")
        .with(
            "order_stats",
            sql("SELECT user_id, COUNT(*)::bigint as total_orders, SUM(amount)::bigint as total_amount FROM orders GROUP BY user_id"),
        )?
        .select(
            sql("SELECT os.user_id, u.name, os.total_orders, os.total_amount \
                 FROM order_stats os \
                 JOIN users u ON u.id = os.user_id \
                 WHERE os.total_amount > ")
                .bind(500_i64),
        )
        .fetch_all_as(client)
        .await?;

    println!("[live] High-value users (total > 500):");
    for s in &high_value {
        println!(
            "       user_id={} name={} total_amount={}",
            s.user_id, s.name, s.total_amount
        );
    }
    println!();

    // ── Demo 3: Recursive CTE — employee org tree ──

    query("DROP TABLE IF EXISTS employees CASCADE")
        .execute(client)
        .await?;
    query(
        "CREATE TABLE employees (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            parent_id BIGINT REFERENCES employees(id)
        )",
    )
    .execute(client)
    .await?;

    // Build an org tree:
    //   CEO (1)
    //   ├── VP Engineering (2)
    //   │   ├── Team Lead A (4)
    //   │   └── Team Lead B (5)
    //   └── VP Sales (3)
    //       └── Sales Rep (6)
    for (id, name, parent) in [
        (1_i64, "CEO", None),
        (2, "VP Engineering", Some(1_i64)),
        (3, "VP Sales", Some(1)),
        (4, "Team Lead A", Some(2)),
        (5, "Team Lead B", Some(2)),
        (6, "Sales Rep", Some(3)),
    ] {
        query("INSERT INTO employees (id, name, parent_id) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(name)
            .bind(parent)
            .execute(client)
            .await?;
    }

    let org_tree: Vec<OrgNode> = sql("")
        .with_recursive(
            "org_tree",
            sql("SELECT id, name, parent_id, 0 as level, name::text as path \
                 FROM employees WHERE parent_id IS NULL"),
            sql("SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name \
                 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
        )?
        .select(sql("SELECT * FROM org_tree ORDER BY path"))
        .fetch_all_as(client)
        .await?;

    println!("[live] Org tree (recursive CTE):");
    for node in &org_tree {
        let indent = "  ".repeat(node.level as usize);
        println!("       {indent}{} (level {})", node.name, node.level);
    }
    println!();

    // ── Demo 4: Recursive CTE with depth limit ──

    query("DROP TABLE IF EXISTS categories CASCADE")
        .execute(client)
        .await?;
    query(
        "CREATE TABLE categories (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            parent_id BIGINT REFERENCES categories(id)
        )",
    )
    .execute(client)
    .await?;

    for (id, name, parent) in [
        (1_i64, "Electronics", None),
        (2, "Computers", Some(1_i64)),
        (3, "Laptops", Some(2)),
        (4, "Gaming Laptops", Some(3)),
        (5, "Phones", Some(1)),
    ] {
        query("INSERT INTO categories (id, name, parent_id) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(name)
            .bind(parent)
            .execute(client)
            .await?;
    }

    let tree: Vec<CategoryNode> = sql("")
        .with_recursive(
            "category_tree",
            sql("SELECT id, name, 0 as depth FROM categories WHERE id = ").bind(1_i64),
            sql("SELECT c.id, c.name, ct.depth + 1 \
                 FROM categories c JOIN category_tree ct ON c.parent_id = ct.id \
                 WHERE ct.depth < ")
                .bind(2_i32),
        )?
        .select_from("category_tree")?
        .fetch_all_as(client)
        .await?;

    println!("[live] Category tree (depth limit = 2):");
    for cat in &tree {
        let indent = "  ".repeat(cat.depth as usize);
        println!("       {indent}{} (depth {})", cat.name, cat.depth);
    }
    println!();

    Ok(())
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    println!("=== CTE Query Examples ===\n");

    // SQL generation demos (no DB needed)
    demo_simple_cte()?;
    demo_multiple_ctes()?;
    demo_recursive_cte()?;
    demo_recursive_cte_with_params()?;
    demo_cte_with_columns()?;
    demo_cte_data_modification()?;
    demo_recursive_union_dedup()?;

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
