//! Example demonstrating InsertModel::insert_many* with array-typed columns (e.g. `text[]`).
//!
//! Run with:
//!   cargo run --example insert_many_array -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{FromRow, InsertModel, Model, OrmError, OrmResult, query};
use std::{env, io};
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelType {
    Text,
    Image,
}

impl ModelType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

impl std::str::FromStr for ModelType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "image" => Ok(Self::Image),
            _ => Err(()),
        }
    }
}

impl pgorm::PgType for ModelType {
    fn pg_array_type() -> &'static str {
        // The scalar is stored as TEXT, so the bulk-insert (UNNEST) cast is `text[]`.
        "text[]"
    }
}

impl ToSql for ModelType {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut tokio_postgres::types::private::BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        self.as_str().to_sql(ty, out)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as ToSql>::accepts(ty)
    }

    tokio_postgres::types::to_sql_checked!();
}

impl<'a> FromSql<'a> for ModelType {
    fn from_sql(
        ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        let s = <&str as FromSql>::from_sql(ty, raw)?;
        s.parse().map_err(|_| {
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid ModelType: {s}"),
            )) as _
        })
    }

    fn accepts(ty: &Type) -> bool {
        <&str as FromSql>::accepts(ty)
    }
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "items")]
struct Item {
    #[orm(id)]
    id: i64,
    name: String,
    tags: Vec<String>,           // PG: text[]
    model_types: Vec<ModelType>, // PG: text[]
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "items", returning = "Item")]
struct NewItem {
    name: String,
    tags: Vec<String>,
    model_types: Vec<ModelType>,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS items CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE items (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            tags TEXT[] NOT NULL,
            model_types TEXT[] NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    let rows = vec![
        NewItem {
            name: "a".into(),
            tags: vec!["t1".into(), "t2".into()],
            model_types: vec![ModelType::Text],
        },
        NewItem {
            name: "b".into(),
            tags: vec![],
            model_types: vec![ModelType::Image, ModelType::Text],
        },
    ];

    // Works because:
    // - `ModelType: PgType` returns `text[]` (scalar field cast for UNNEST)
    // - `Vec<T>: PgType` automatically becomes `text[][]` for array columns in insert_many
    let inserted: Vec<Item> = NewItem::insert_many_returning(&client, rows).await?;
    println!("inserted {} item(s):", inserted.len());
    for item in &inserted {
        println!(
            "- id={} name={} tags={:?} model_types={:?}",
            item.id, item.name, item.tags, item.model_types
        );
    }

    Ok(())
}
