//! SQL Builder example for pgorm
//!
//! Run with: cargo run --example builder -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::builder::{DeleteBuilder, InsertBuilder, QueryBuilder, UpdateBuilder};
use pgorm::{create_pool, FromRow, Model, MutationBuilder, OrmError, SqlBuilder};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price: i32,
    category: Option<String>,
    in_stock: bool,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Read DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup table
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price INTEGER NOT NULL,
                category TEXT,
                in_stock BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute("DELETE FROM products", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    // ============================================
    // InsertBuilder
    // ============================================
    println!("=== InsertBuilder ===");

    // Simple insert with RETURNING
    let product: Product = InsertBuilder::new("products")
        .set("name", "Laptop")
        .set("price", 999)
        .set("category", Some("Electronics"))
        .set("in_stock", true)
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("Inserted: {:?}", product);

    // Insert with optional value (None will skip the column)
    let _: Product = InsertBuilder::new("products")
        .set("name", "Phone")
        .set("price", 599)
        .set_opt("category", None::<String>) // Will be skipped (NULL via default)
        .set("in_stock", true)
        .returning("*")
        .query_one_as(&client)
        .await?;

    // Insert with default value (provide default if None)
    let product: Product = InsertBuilder::new("products")
        .set("name", "Tablet")
        .set("price", 399)
        .set_default("category", Some("Electronics"), "General")
        .set("in_stock", true)
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("Inserted with category: {:?}", product);

    // Insert with ON CONFLICT
    let product: Product = InsertBuilder::new("products")
        .set("name", "Keyboard")
        .set("price", 99)
        .set("category", Some("Accessories"))
        .set("in_stock", true)
        .on_conflict("(name)")
        .do_nothing()
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("ON CONFLICT DO NOTHING: {:?}", product);

    // ============================================
    // QueryBuilder (SELECT)
    // ============================================
    println!("\n=== QueryBuilder (SELECT) ===");

    // Simple select - QueryBuilder::new() takes table name
    let products: Vec<Product> = QueryBuilder::new("products")
        .select("*")
        .query_as(&client)
        .await?;

    println!("All products: {} items", products.len());

    // Select with WHERE conditions
    let electronics: Vec<Product> = QueryBuilder::new("products")
        .select("*")
        .and_eq("category", "Electronics")
        .and_gte("price", 400)
        .order_by("price DESC")
        .query_as(&client)
        .await?;

    println!("Electronics >= $400: {:?}", electronics);

    // Select with multiple conditions
    let cheap_in_stock: Vec<Product> = QueryBuilder::new("products")
        .select("id, name, price, category, in_stock")
        .and_lt("price", 500)
        .and_eq("in_stock", true)
        .order_by("price ASC")
        .limit(10)
        .query_as(&client)
        .await?;

    println!("Cheap in-stock items: {:?}", cheap_in_stock);

    // Select with IN clause
    let specific: Vec<Product> = QueryBuilder::new("products")
        .select("*")
        .and_in("name", vec!["Laptop", "Phone"])
        .query_as(&client)
        .await?;

    println!("Specific products: {:?}", specific);

    // Count query
    let count = QueryBuilder::new("products")
        .and_eq("in_stock", true)
        .count(&client)
        .await?;

    println!("In-stock count: {}", count);

    // ============================================
    // UpdateBuilder
    // ============================================
    println!("\n=== UpdateBuilder ===");

    // Update single row
    let updated: Product = UpdateBuilder::new("products")
        .set("price", 949)
        .and_eq("name", "Laptop")
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("Updated price: {:?}", updated);

    // Update multiple fields
    let updated: Product = UpdateBuilder::new("products")
        .set("price", 549)
        .set("category", Some("Mobile"))
        .and_eq("name", "Phone")
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("Updated multiple fields: {:?}", updated);

    // Update with raw SQL expression
    let updated: Product = UpdateBuilder::new("products")
        .set_raw("price", "CAST(price * 0.9 AS INTEGER)") // 10% discount
        .and_eq("name", "Tablet")
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("After 10% discount: {:?}", updated);

    // ============================================
    // DeleteBuilder
    // ============================================
    println!("\n=== DeleteBuilder ===");

    // Insert a product to delete
    InsertBuilder::new("products")
        .set("name", "ToDelete")
        .set("price", 1)
        .set("in_stock", false)
        .execute(&client)
        .await?;

    // Delete with condition
    let deleted_count = DeleteBuilder::new("products")
        .and_eq("name", "ToDelete")
        .execute(&client)
        .await?;

    println!("Deleted {} row(s)", deleted_count);

    // Delete with RETURNING
    InsertBuilder::new("products")
        .set("name", "AlsoDelete")
        .set("price", 1)
        .set("in_stock", false)
        .execute(&client)
        .await?;

    let deleted: Product = DeleteBuilder::new("products")
        .and_eq("name", "AlsoDelete")
        .returning("*")
        .query_one_as(&client)
        .await?;

    println!("Deleted with RETURNING: {:?}", deleted);

    // Final count
    let final_count = QueryBuilder::new("products").count(&client).await?;

    println!("\nFinal products count: {}", final_count);

    Ok(())
}
