//! Example demonstrating Model JOIN queries (view pattern)
//!
//! Run with: cargo run --example join_view -p pgorm --features "derive,pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, FromRow, Model, OrmError};
use std::env;

// ============================================
// Base tables (simple models)
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "categories")]
#[allow(dead_code)]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "brands")]
#[allow(dead_code)]
struct Brand {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    category_id: i64,
    brand_id: Option<i64>,
}

// ============================================
// View model with JOINs
// ============================================

/// A view that joins products with their category and brand
#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[orm(join(table = "categories", on = "products.category_id = categories.id", type = "inner"))]
#[orm(join(table = "brands", on = "products.brand_id = brands.id", type = "left"))]
#[allow(dead_code)]
struct ProductView {
    // Fields from products table (main table)
    #[orm(id, table = "products", column = "id")]
    id: i64,
    #[orm(table = "products", column = "name")]
    product_name: String,
    #[orm(table = "products")]
    price_cents: i64,

    // Fields from categories table (joined)
    #[orm(table = "categories", column = "name")]
    category_name: String,

    // Fields from brands table (left joined, nullable)
    #[orm(table = "brands", column = "name")]
    brand_name: Option<String>,
}

impl ProductView {
    fn price_display(&self) -> f64 {
        self.price_cents as f64 / 100.0
    }
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // ============================================
    // Setup: Create tables
    // ============================================
    println!("=== Setting up tables ===\n");

    // Drop tables in correct order
    client.execute("DROP TABLE IF EXISTS reviews", &[]).await.map_err(OrmError::from_db_error)?;
    client.execute("DROP TABLE IF EXISTS products", &[]).await.map_err(OrmError::from_db_error)?;
    client.execute("DROP TABLE IF EXISTS categories", &[]).await.map_err(OrmError::from_db_error)?;
    client.execute("DROP TABLE IF EXISTS brands", &[]).await.map_err(OrmError::from_db_error)?;

    // Create tables
    client
        .execute(
            "CREATE TABLE categories (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE brands (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price_cents BIGINT NOT NULL,
                category_id BIGINT NOT NULL REFERENCES categories(id),
                brand_id BIGINT REFERENCES brands(id)
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // ============================================
    // Insert test data
    // ============================================
    println!("=== Inserting test data ===\n");

    // Categories
    let electronics: Category = query("INSERT INTO categories (name) VALUES ($1) RETURNING *")
        .bind("Electronics")
        .fetch_one_as(&client)
        .await?;

    let accessories: Category = query("INSERT INTO categories (name) VALUES ($1) RETURNING *")
        .bind("Accessories")
        .fetch_one_as(&client)
        .await?;

    // Brands
    let apple: Brand = query("INSERT INTO brands (name) VALUES ($1) RETURNING *")
        .bind("Apple")
        .fetch_one_as(&client)
        .await?;

    let samsung: Brand = query("INSERT INTO brands (name) VALUES ($1) RETURNING *")
        .bind("Samsung")
        .fetch_one_as(&client)
        .await?;

    // Products (some with brand, some without)
    query("INSERT INTO products (name, price_cents, category_id, brand_id) VALUES ($1, $2, $3, $4)")
        .bind("iPhone 15")
        .bind(99900_i64)
        .bind(electronics.id)
        .bind(Some(apple.id))
        .execute(&client)
        .await?;

    query("INSERT INTO products (name, price_cents, category_id, brand_id) VALUES ($1, $2, $3, $4)")
        .bind("Galaxy S24")
        .bind(89900_i64)
        .bind(electronics.id)
        .bind(Some(samsung.id))
        .execute(&client)
        .await?;

    query("INSERT INTO products (name, price_cents, category_id, brand_id) VALUES ($1, $2, $3, $4)")
        .bind("USB-C Cable")
        .bind(1999_i64)
        .bind(accessories.id)
        .bind(None::<i64>) // No brand
        .execute(&client)
        .await?;

    query("INSERT INTO products (name, price_cents, category_id, brand_id) VALUES ($1, $2, $3, $4)")
        .bind("AirPods Pro")
        .bind(24900_i64)
        .bind(accessories.id)
        .bind(Some(apple.id))
        .execute(&client)
        .await?;

    println!("Created {} categories, {} brands, 4 products\n", 2, 2);

    // ============================================
    // Show generated constants
    // ============================================
    println!("=== Generated constants ===\n");
    println!("ProductView::TABLE = \"{}\"", ProductView::TABLE);
    println!("ProductView::SELECT_LIST = \"{}\"", ProductView::SELECT_LIST);
    println!("ProductView::JOIN_CLAUSE = \"{}\"", ProductView::JOIN_CLAUSE);
    println!();

    // ============================================
    // Example 1: select_all with JOINs
    // ============================================
    println!("=== ProductView::select_all() (with JOINs) ===\n");

    let all_products = ProductView::select_all(&client).await?;

    println!("Found {} products with category and brand info:", all_products.len());
    for p in &all_products {
        println!(
            "  - {} (${:.2}) | Category: {} | Brand: {}",
            p.product_name,
            p.price_display(),
            p.category_name,
            p.brand_name.as_deref().unwrap_or("(none)")
        );
    }

    // ============================================
    // Example 2: select_one with JOINs
    // ============================================
    println!("\n=== ProductView::select_one(1) ===\n");

    let product = ProductView::select_one(&client, 1).await?;
    println!("Product ID 1: {:?}", product);

    // ============================================
    // Comparison: Manual query vs View
    // ============================================
    println!("\n=== Comparison: The SQL that gets generated ===\n");

    let sql = format!(
        "SELECT {} FROM {} {}",
        ProductView::SELECT_LIST,
        ProductView::TABLE,
        ProductView::JOIN_CLAUSE
    );
    println!("Generated SQL:\n{}\n", sql);

    println!("=== Done ===");

    Ok(())
}
