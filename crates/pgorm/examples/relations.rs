//! Example demonstrating Model relationships (has_many and belongs_to)
//!
//! Run with: cargo run --example relations -p pgorm --features "derive,pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, FromRow, Model, OrmError};
use std::env;

// ============================================
// Category - Parent model
// ============================================
#[derive(Debug, FromRow, Model)]
#[orm(table = "categories")]
#[orm(has_many(Product, foreign_key = "category_id", as = "products"))]
#[allow(dead_code)]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

// ============================================
// Product - Child of Category, Parent of Review
// ============================================
#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[orm(belongs_to(Category, foreign_key = "category_id"))]
#[orm(has_many(Review, foreign_key = "product_id"))]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    category_id: i64,
}

impl Product {
    fn price_display(&self) -> f64 {
        self.price_cents as f64 / 100.0
    }
}

// ============================================
// Review - Child of Product
// ============================================
#[derive(Debug, FromRow, Model)]
#[orm(table = "reviews")]
#[orm(belongs_to(Product, foreign_key = "product_id"))]
#[allow(dead_code)]
struct Review {
    #[orm(id)]
    id: i64,
    product_id: i64,
    rating: i32,
    comment: String,
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

    // Drop tables in correct order (children first)
    client
        .execute("DROP TABLE IF EXISTS reviews", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS products", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS categories", &[])
        .await
        .map_err(OrmError::from_db_error)?;

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
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price_cents BIGINT NOT NULL,
                category_id BIGINT NOT NULL REFERENCES categories(id)
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE reviews (
                id BIGSERIAL PRIMARY KEY,
                product_id BIGINT NOT NULL REFERENCES products(id),
                rating INT NOT NULL CHECK (rating >= 1 AND rating <= 5),
                comment TEXT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // ============================================
    // Insert test data
    // ============================================
    println!("=== Inserting test data ===\n");

    // Create categories
    let electronics: Category =
        query("INSERT INTO categories (name) VALUES ($1) RETURNING *")
            .bind("Electronics")
            .fetch_one_as(&client)
            .await?;
    println!("Created category: {:?}", electronics);

    let accessories: Category =
        query("INSERT INTO categories (name) VALUES ($1) RETURNING *")
            .bind("Accessories")
            .fetch_one_as(&client)
            .await?;
    println!("Created category: {:?}", accessories);

    // Create products
    let laptop: Product =
        query("INSERT INTO products (name, price_cents, category_id) VALUES ($1, $2, $3) RETURNING *")
            .bind("Laptop")
            .bind(99999_i64)
            .bind(electronics.id)
            .fetch_one_as(&client)
            .await?;
    println!("Created product: {:?}", laptop);

    let phone: Product =
        query("INSERT INTO products (name, price_cents, category_id) VALUES ($1, $2, $3) RETURNING *")
            .bind("Smartphone")
            .bind(79999_i64)
            .bind(electronics.id)
            .fetch_one_as(&client)
            .await?;
    println!("Created product: {:?}", phone);

    let case: Product =
        query("INSERT INTO products (name, price_cents, category_id) VALUES ($1, $2, $3) RETURNING *")
            .bind("Phone Case")
            .bind(1999_i64)
            .bind(accessories.id)
            .fetch_one_as(&client)
            .await?;
    println!("Created product: {:?}", case);

    // Create reviews
    query("INSERT INTO reviews (product_id, rating, comment) VALUES ($1, $2, $3)")
        .bind(laptop.id)
        .bind(5_i32)
        .bind("Excellent laptop!")
        .execute(&client)
        .await?;

    query("INSERT INTO reviews (product_id, rating, comment) VALUES ($1, $2, $3)")
        .bind(laptop.id)
        .bind(4_i32)
        .bind("Great performance, a bit heavy")
        .execute(&client)
        .await?;

    query("INSERT INTO reviews (product_id, rating, comment) VALUES ($1, $2, $3)")
        .bind(phone.id)
        .bind(5_i32)
        .bind("Best phone ever!")
        .execute(&client)
        .await?;

    println!("\n");

    // ============================================
    // Example 1: has_many - Category -> Products
    // ============================================
    println!("=== has_many: Category.select_products() ===\n");

    let products_in_electronics = electronics.select_products(&client).await?;
    println!(
        "Products in '{}' category ({} items):",
        electronics.name,
        products_in_electronics.len()
    );
    for product in &products_in_electronics {
        println!("  - {} (${:.2})", product.name, product.price_display());
    }

    let products_in_accessories = accessories.select_products(&client).await?;
    println!(
        "\nProducts in '{}' category ({} items):",
        accessories.name,
        products_in_accessories.len()
    );
    for product in &products_in_accessories {
        println!("  - {} (${:.2})", product.name, product.price_display());
    }

    // ============================================
    // Example 2: has_many - Product -> Reviews
    // ============================================
    println!("\n=== has_many: Product.select_reviews() ===\n");

    let laptop_reviews = laptop.select_reviews(&client).await?;
    println!(
        "Reviews for '{}' ({} reviews):",
        laptop.name,
        laptop_reviews.len()
    );
    for review in &laptop_reviews {
        println!("  - {} stars: \"{}\"", review.rating, review.comment);
    }

    let case_reviews = case.select_reviews(&client).await?;
    println!(
        "\nReviews for '{}' ({} reviews):",
        case.name,
        case_reviews.len()
    );
    if case_reviews.is_empty() {
        println!("  (no reviews yet)");
    }

    // ============================================
    // Example 3: belongs_to - Product -> Category
    // ============================================
    println!("\n=== belongs_to: Product.select_category() ===\n");

    let laptop_category = laptop.select_category(&client).await?;
    println!(
        "Product '{}' belongs to category: {}",
        laptop.name, laptop_category.name
    );

    let case_category = case.select_category(&client).await?;
    println!(
        "Product '{}' belongs to category: {}",
        case.name, case_category.name
    );

    // ============================================
    // Example 4: belongs_to - Review -> Product
    // ============================================
    println!("\n=== belongs_to: Review.select_product() ===\n");

    let first_review = laptop_reviews.first().unwrap();
    let reviewed_product = first_review.select_product(&client).await?;
    println!(
        "Review \"{}\" is for product: {}",
        first_review.comment, reviewed_product.name
    );

    // ============================================
    // Example 5: Chained navigation
    // ============================================
    println!("\n=== Chained navigation: Review -> Product -> Category ===\n");

    let review = laptop_reviews.first().unwrap();
    let product = review.select_product(&client).await?;
    let category = product.select_category(&client).await?;
    println!(
        "Review \"{}...\" -> Product '{}' -> Category '{}'",
        &review.comment[..20.min(review.comment.len())],
        product.name,
        category.name
    );

    println!("\n=== Done ===");

    Ok(())
}
