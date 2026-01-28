//! Example demonstrating the auto-generated Query struct for dynamic queries
//!
//! Run with: cargo run --example query_struct -p pgorm --features "derive,pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, FromRow, Model, OrmError};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    category_id: i64,
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
        .execute("DROP TABLE IF EXISTS reviews", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS products CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price_cents BIGINT NOT NULL,
                category_id BIGINT NOT NULL,
                in_stock BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Insert test data
    println!("=== Inserting test data ===\n");

    for (name, price, category, in_stock) in [
        ("Laptop Pro", 99999_i64, 1_i64, true),
        ("Gaming Mouse", 2999, 1, true),
        ("Mechanical Keyboard", 7999, 1, false),
        ("4K Monitor", 29999, 1, true),
        ("USB-C Cable", 999, 2, true),
        ("HDMI Cable", 1499, 2, true),
        ("HD Webcam", 4999, 1, false),
        ("Wireless Headphones", 14999, 3, true),
        ("Desktop Speakers", 9999, 3, true),
        ("Studio Microphone", 12999, 3, false),
    ] {
        query("INSERT INTO products (name, price_cents, category_id, in_stock) VALUES ($1, $2, $3, $4)")
            .bind(name)
            .bind(price)
            .bind(category)
            .bind(in_stock)
            .execute(&client)
            .await?;
    }

    println!("Inserted 10 products\n");

    // ============================================
    // Example 1: Equality (eq)
    // ============================================
    println!("=== Query: category_id = 1 ===\n");

    let electronics = Product::query()
        .eq("category_id", 1_i64)
        .find(&client)
        .await?;

    println!("Found {} electronics:", electronics.len());
    for p in &electronics {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    // ============================================
    // Example 2: ILIKE (case-insensitive pattern)
    // ============================================
    println!("\n=== Query: name ILIKE '%mouse%' ===\n");

    let mice = Product::query()
        .ilike("name", "%mouse%")
        .find(&client)
        .await?;

    println!("Found {} products with 'mouse' in name:", mice.len());
    for p in &mice {
        println!("  - {}", p.name);
    }

    // ============================================
    // Example 3: Range queries (gt, lt, gte, lte)
    // ============================================
    println!("\n=== Query: price_cents >= 5000 AND price_cents < 15000 ===\n");

    let mid_range = Product::query()
        .gte("price_cents", 5000_i64)
        .lt("price_cents", 15000_i64)
        .find(&client)
        .await?;

    println!("Found {} mid-range products:", mid_range.len());
    for p in &mid_range {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    // ============================================
    // Example 4: BETWEEN
    // ============================================
    println!("\n=== Query: price_cents BETWEEN 1000 AND 5000 ===\n");

    let budget = Product::query()
        .between("price_cents", 1000_i64, 5000_i64)
        .find(&client)
        .await?;

    println!("Found {} budget products:", budget.len());
    for p in &budget {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    // ============================================
    // Example 5: IN list
    // ============================================
    println!("\n=== Query: category_id IN (1, 3) ===\n");

    let selected = Product::query()
        .in_list("category_id", vec![1_i64, 3])
        .find(&client)
        .await?;

    println!("Found {} products in categories 1 or 3:", selected.len());
    for p in &selected {
        println!("  - {} (category {})", p.name, p.category_id);
    }

    // ============================================
    // Example 6: NOT IN
    // ============================================
    println!("\n=== Query: category_id NOT IN (1) ===\n");

    let non_electronics = Product::query()
        .not_in("category_id", vec![1_i64])
        .find(&client)
        .await?;

    println!("Found {} non-electronics:", non_electronics.len());
    for p in &non_electronics {
        println!("  - {} (category {})", p.name, p.category_id);
    }

    // ============================================
    // Example 7: Multiple conditions
    // ============================================
    println!("\n=== Query: category_id = 1 AND in_stock = true AND price < 10000 ===\n");

    let cheap_in_stock = Product::query()
        .eq("category_id", 1_i64)
        .eq("in_stock", true)
        .lt("price_cents", 10000_i64)
        .find(&client)
        .await?;

    println!("Found {} cheap in-stock electronics:", cheap_in_stock.len());
    for p in &cheap_in_stock {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    // ============================================
    // Example 8: NOT LIKE
    // ============================================
    println!("\n=== Query: name NOT LIKE '%Cable%' ===\n");

    let not_cables = Product::query()
        .not_like("name", "%Cable%")
        .find(&client)
        .await?;

    println!("Found {} non-cable products:", not_cables.len());
    for p in &not_cables {
        println!("  - {}", p.name);
    }

    // ============================================
    // Example 9: Pagination with order
    // ============================================
    println!("\n=== Query: Page 1, 3 per page, ORDER BY price DESC ===\n");

    let page1 = Product::query()
        .order_by("price_cents DESC")
        .per_page(3)
        .page(1)
        .find(&client)
        .await?;

    println!("Page 1 - Top 3 most expensive:");
    for p in &page1 {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    println!("\n=== Query: Page 2 ===\n");

    let page2 = Product::query()
        .order_by("price_cents DESC")
        .per_page(3)
        .page(2)
        .find(&client)
        .await?;

    println!("Page 2:");
    for p in &page2 {
        println!("  - {} (${:.2})", p.name, p.price_display());
    }

    // ============================================
    // Example 10: Count
    // ============================================
    println!("\n=== Query: Count in_stock = true ===\n");

    let in_stock_count = Product::query()
        .eq("in_stock", true)
        .count(&client)
        .await?;
    println!("In-stock products: {}", in_stock_count);

    let out_of_stock_count = Product::query()
        .eq("in_stock", false)
        .count(&client)
        .await?;
    println!("Out-of-stock products: {}", out_of_stock_count);

    // ============================================
    // Example 11: find_one
    // ============================================
    println!("\n=== Query: find_one with ILIKE ===\n");

    let laptop = Product::query()
        .ilike("name", "%laptop%")
        .find_one(&client)
        .await?;
    println!("Found: {} (${:.2})", laptop.name, laptop.price_display());

    // ============================================
    // Example 12: find_one_opt (not found)
    // ============================================
    println!("\n=== Query: find_one_opt (not found) ===\n");

    let not_found = Product::query()
        .eq("name", "NonExistent")
        .find_one_opt(&client)
        .await?;
    println!("Result: {:?}", not_found);

    // ============================================
    // Example 13: Using column constants
    // ============================================
    println!("\n=== Using column constants ===\n");

    let using_const = Product::query()
        .ilike(ProductQuery::name, "%keyboard%")
        .eq(ProductQuery::in_stock, false)
        .find(&client)
        .await?;

    println!("Found {} out-of-stock keyboards:", using_const.len());
    for p in &using_const {
        println!("  - {}", p.name);
    }

    // ============================================
    // Example 14: ne (not equal)
    // ============================================
    println!("\n=== Query: category_id != 2 ===\n");

    let not_cables_cat = Product::query()
        .ne("category_id", 2_i64)
        .find(&client)
        .await?;

    println!("Found {} products not in category 2:", not_cables_cat.len());

    println!("\n=== Done ===");

    Ok(())
}
