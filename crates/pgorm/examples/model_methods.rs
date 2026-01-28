//! Example demonstrating Model::select_all and Model::select_one methods
//!
//! Run with: cargo run --example model_methods -p pgorm --features "derive,pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, Model, OrmError, create_pool, query};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    in_stock: bool,
}

impl Product {
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

    // Setup: Drop and recreate table
    client
        .execute("DROP TABLE IF EXISTS products", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price_cents BIGINT NOT NULL,
                in_stock BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Insert some test data
    println!("=== Inserting test data ===");

    let laptop: Product =
        query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3) RETURNING *")
            .bind("Laptop")
            .bind(99999_i64) // $999.99
            .bind(true)
            .fetch_one_as(&client)
            .await?;
    println!("Inserted: {:?}", laptop);

    query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3)")
        .bind("Mouse")
        .bind(2999_i64) // $29.99
        .bind(true)
        .execute(&client)
        .await?;

    query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3)")
        .bind("Keyboard")
        .bind(7999_i64) // $79.99
        .bind(false)
        .execute(&client)
        .await?;

    println!("Inserted 3 products total\n");

    // ============================================
    // Example 1: select_all - Fetch all records
    // ============================================
    println!("=== Product::select_all ===");

    let all_products = Product::select_all(&client).await?;

    println!("Found {} products:", all_products.len());
    for product in &all_products {
        println!(
            "  - {} (${:.2}) {}",
            product.name,
            product.price_display(),
            if product.in_stock {
                "[In Stock]"
            } else {
                "[Out of Stock]"
            }
        );
    }

    // ============================================
    // Example 2: select_one - Fetch by ID
    // ============================================
    println!("\n=== Product::select_one ===");

    let found = Product::select_one(&client, laptop.id).await?;
    println!("Found product by ID {}: {:?}", laptop.id, found);

    // ============================================
    // Example 3: select_one with non-existent ID
    // ============================================
    println!("\n=== Product::select_one with non-existent ID ===");

    match Product::select_one(&client, 99999).await {
        Ok(product) => println!("Found: {:?}", product),
        Err(OrmError::NotFound(_)) => println!("Product with ID 99999 not found (expected)"),
        Err(e) => println!("Unexpected error: {}", e),
    }

    // ============================================
    // Comparison: Manual query vs Model methods
    // ============================================
    println!("\n=== Comparison: Manual query vs Model methods ===");

    // Manual way (more verbose)
    let sql = format!(
        "SELECT {} FROM {} WHERE {} = $1",
        Product::SELECT_LIST,
        Product::TABLE,
        Product::ID
    );
    let manual: Product = query(&sql).bind(laptop.id).fetch_one_as(&client).await?;
    println!("Manual query result: {:?}", manual);

    // Using select_one (cleaner)
    let convenient = Product::select_one(&client, laptop.id).await?;
    println!("select_one result:   {:?}", convenient);

    println!("\n=== Done ===");

    Ok(())
}
