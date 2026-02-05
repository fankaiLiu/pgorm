//! Database schema setup helpers for optimistic locking example

#![allow(dead_code)]

use pgorm::{GenericClient, OrmError, query};

/// Setup schema for articles table with a version column
pub async fn setup_articles_schema(conn: &impl GenericClient) -> Result<(), OrmError> {
    query("DROP TABLE IF EXISTS articles CASCADE")
        .execute(conn)
        .await?;

    query(
        "CREATE TABLE articles (
            id BIGSERIAL PRIMARY KEY,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            version INT NOT NULL DEFAULT 0
        )",
    )
    .execute(conn)
    .await?;

    Ok(())
}
