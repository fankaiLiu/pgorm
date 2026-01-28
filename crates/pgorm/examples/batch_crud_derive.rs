//! Example demonstrating batch Insert/Update/Delete + UPSERT from derive macros.
//!
//! Run with:
//!   cargo run --example batch_crud_derive -p pgorm --features "derive,pool"
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
    brand_id: Option<Option<i64>>,
}

#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "ProductView")]
struct ProductUpsert {
    // When an InsertModel has an #[orm(id)] field, UPSERT helpers are generated:
    // - upsert / upsert_returning
    // - upsert_many / upsert_many_returning
    #[orm(id)]
    id: i64,
    name: String,
    category_id: i64,
    brand_id: Option<i64>,
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

    // ============================================
    // Batch insert (UNNEST bulk insert)
    // ============================================
    let inserted: Vec<ProductView> = NewProduct::insert_many_returning(
        &client,
        vec![
            NewProduct {
                name: "Laptop".to_string(),
                category_id: 1,
                brand_id: Some(10),
            },
            NewProduct {
                name: "Mouse".to_string(),
                category_id: 1,
                brand_id: None,
            },
            NewProduct {
                name: "Keyboard".to_string(),
                category_id: 1,
                brand_id: Some(20),
            },
        ],
    )
    .await?;

    println!("insert_many_returning -> {} rows", inserted.len());
    for p in &inserted {
        println!("  inserted: {:?}", p);
    }

    let ids: Vec<i64> = inserted.iter().map(|p| p.id).collect();

    // ============================================
    // Batch update (same patch applied to many rows)
    // ============================================
    let updated: Vec<ProductView> = ProductPatch {
        name: Some("Renamed".to_string()),
        brand_id: Some(None), // set NULL
    }
    .update_by_ids_returning(&client, ids.clone())
    .await?;

    println!("\nupdate_by_ids_returning -> {} rows", updated.len());
    for p in &updated {
        println!("  updated: {:?}", p);
    }

    // ============================================
    // UPSERT: update if id exists, otherwise insert
    // ============================================
    let existing_id = ids[0];
    let upserted_existing: ProductView = ProductUpsert {
        id: existing_id,
        name: "Laptop (upserted)".to_string(),
        category_id: 1,
        brand_id: Some(99),
    }
    .upsert_returning(&client)
    .await?;

    let new_id = ids.iter().max().copied().unwrap_or(0) + 10_000;
    let upserted_new: ProductView = ProductUpsert {
        id: new_id,
        name: "New (upserted)".to_string(),
        category_id: 1,
        brand_id: None,
    }
    .upsert_returning(&client)
    .await?;

    println!("\nupsert_returning (existing): {:?}", upserted_existing);
    println!("upsert_returning (new):      {:?}", upserted_new);

    // ============================================
    // Batch delete
    // ============================================
    let deleted: Vec<ProductView> = ProductView::delete_by_ids_returning(&client, vec![existing_id, new_id]).await?;
    println!("\ndelete_by_ids_returning -> {} rows", deleted.len());
    for p in &deleted {
        println!("  deleted: {:?}", p);
    }

    Ok(())
}

