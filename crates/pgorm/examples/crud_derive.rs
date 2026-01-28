//! Example demonstrating InsertModel / UpdateModel + ViewModel (Model) returning.
//!
//! Run with:
//!   cargo run --example crud_derive -p pgorm --features "derive,pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, InsertModel, Model, OrmError, UpdateModel, create_pool, query};
use std::env;

// ============================================
// Base tables
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
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    category_id: i64,
    brand_id: Option<i64>,
}

// ============================================
// View model (JOIN view)
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[orm(join(
    table = "categories",
    on = "products.category_id = categories.id",
    type = "inner"
))]
#[allow(dead_code)]
struct ProductView {
    // products
    #[orm(id, table = "products", column = "id")]
    id: i64,
    #[orm(table = "products", column = "name")]
    product_name: String,
    #[orm(table = "products", column = "brand_id")]
    brand_id: Option<i64>,
    // categories
    #[orm(table = "categories", column = "name")]
    category_name: String,
}

// ============================================
// Write models
// ============================================

#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "ProductView")]
struct NewProduct {
    name: String,
    category_id: i64,
    brand_id: Option<i64>,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "products", returning = "ProductView")]
struct ProductPatch {
    name: Option<String>,
    // Demonstrates nullable patch semantics:
    // - None => skip update
    // - Some(None) => set to NULL
    // - Some(Some(v)) => set to v
    brand_id: Option<Option<i64>>,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup
    client
        .execute("DROP TABLE IF EXISTS products", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS categories", &[])
        .await
        .map_err(OrmError::from_db_error)?;

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
                category_id BIGINT NOT NULL REFERENCES categories(id),
                brand_id BIGINT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Seed categories
    query("INSERT INTO categories (name) VALUES ($1)")
        .bind("Electronics")
        .execute(&client)
        .await?;

    // Insert returning a JOIN view model
    let created: ProductView = NewProduct {
        name: "Laptop".to_string(),
        category_id: 1,
        brand_id: Some(10),
    }
    .insert_returning(&client)
    .await?;

    println!("Created: {:?}", created);

    // Update returning a JOIN view model
    let updated: ProductView = ProductPatch {
        name: Some("Laptop Pro".to_string()),
        brand_id: Some(None), // set NULL
    }
    .update_by_id_returning(&client, created.id)
    .await?;

    println!("Updated: {:?}", updated);

    // Delete
    let deleted: ProductView = ProductView::delete_by_id_returning(&client, created.id).await?;
    println!("Deleted: {:?}", deleted);

    Ok(())
}
