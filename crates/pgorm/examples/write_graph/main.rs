//! Example demonstrating multi-table write graph (insert_graph_report).
//!
//! Run with:
//!   cargo run --example write_graph -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, InsertModel, Model, OrmError, OrmResult, WriteReport, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "categories")]
#[allow(dead_code)]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "categories", returning = "Category")]
struct NewCategory {
    name: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_details")]
struct NewProductDetail {
    // Filled by graph via has_one.
    product_id: Option<uuid::Uuid>,
    description: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_tags")]
struct NewProductTag {
    // Filled by graph via has_many.
    product_id: Option<uuid::Uuid>,
    tag: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "audit_logs")]
struct NewAuditLog {
    // In this example, we know product_id upfront (UUID), so we can set it ourselves.
    product_id: uuid::Uuid,
    action: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(graph_root_id_field = "id")]
#[orm(belongs_to(
    NewCategory,
    field = "category",
    set_fk_field = "category_id",
    mode = "insert_returning",
    required = false
))]
#[orm(has_one(NewProductDetail, field = "detail", fk_field = "product_id", mode = "insert"))]
#[orm(has_many(NewProductTag, field = "tags", fk_field = "product_id", mode = "insert"))]
#[orm(after_insert(NewAuditLog, field = "audit", mode = "insert"))]
struct NewProductGraph {
    // UUID root id (known before insert).
    id: uuid::Uuid,

    name: String,
    category_id: Option<i64>,

    // Graph fields (not inserted into products).
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
    audit: Option<NewAuditLog>,
}

fn print_report(report: &WriteReport<Product>) {
    println!("affected = {}", report.affected);
    println!("steps:");
    for s in &report.steps {
        println!("- {} affected={}", s.tag, s.affected);
    }
    println!("root: {:?}", report.root.as_ref().map(|p| (&p.id, &p.name, p.category_id)));
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (mut client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    // Clean slate.
    query("DROP TABLE IF EXISTS audit_logs CASCADE").execute(&client).await?;
    query("DROP TABLE IF EXISTS product_tags CASCADE").execute(&client).await?;
    query("DROP TABLE IF EXISTS product_details CASCADE").execute(&client).await?;
    query("DROP TABLE IF EXISTS products CASCADE").execute(&client).await?;
    query("DROP TABLE IF EXISTS categories CASCADE").execute(&client).await?;

    query(
        "CREATE TABLE categories (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        )",
    )
    .execute(&client)
    .await?;

    query(
        "CREATE TABLE products (
            id UUID PRIMARY KEY,
            name TEXT NOT NULL,
            category_id BIGINT REFERENCES categories(id),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )",
    )
    .execute(&client)
    .await?;

    query(
        "CREATE TABLE product_details (
            product_id UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
            description TEXT NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    query(
        "CREATE TABLE product_tags (
            id BIGSERIAL PRIMARY KEY,
            product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            tag TEXT NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    query(
        "CREATE TABLE audit_logs (
            id BIGSERIAL PRIMARY KEY,
            product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            action TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )",
    )
    .execute(&client)
    .await?;

    let product_id = uuid::Uuid::new_v4();

    let report = pgorm::transaction!(&mut client, tx, {
        let new_product = NewProductGraph {
            id: product_id,
            name: "Graph Product".into(),
            category_id: None,
            category: Some(NewCategory {
                name: "Category A".into(),
            }),
            detail: Some(NewProductDetail {
                product_id: None,
                description: "Inserted via has_one".into(),
            }),
            tags: Some(vec![
                NewProductTag {
                    product_id: None,
                    tag: "tag-1".into(),
                },
                NewProductTag {
                    product_id: None,
                    tag: "tag-2".into(),
                },
            ]),
            audit: Some(NewAuditLog {
                product_id,
                action: "CREATE_PRODUCT".into(),
            }),
        };

        let report = new_product.insert_graph_report(&tx).await?;
        Ok(report)
    })?;

    print_report(&report);

    // Quick verification queries.
    let tags_count: i64 = pgorm::query("SELECT COUNT(*) FROM product_tags WHERE product_id = $1")
        .bind(product_id)
        .fetch_scalar_one(&client)
        .await?;
    let audit_count: i64 = pgorm::query("SELECT COUNT(*) FROM audit_logs WHERE product_id = $1")
        .bind(product_id)
        .fetch_scalar_one(&client)
        .await?;
    println!("\nproduct_tags count = {tags_count}, audit_logs count = {audit_count}");

    Ok(())
}
