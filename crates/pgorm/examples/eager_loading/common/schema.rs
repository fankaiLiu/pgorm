//! Database schema setup helpers for examples

#![allow(dead_code)]

use pgorm::{GenericClient, OrmError, query};

/// Setup schema for users table (used in changeset_input example)
pub async fn setup_users_schema(conn: &impl GenericClient) -> Result<(), OrmError> {
    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(conn)
        .await?;

    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            age INT,
            external_id UUID NOT NULL,
            homepage TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )",
    )
    .execute(conn)
    .await?;

    Ok(())
}

/// Setup schema for users and posts tables (used in eager_loading example)
pub async fn setup_users_posts_schema(conn: &impl GenericClient) -> Result<(), OrmError> {
    query("DROP TABLE IF EXISTS posts CASCADE")
        .execute(conn)
        .await?;
    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(conn)
        .await?;

    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL
        )",
    )
    .execute(conn)
    .await?;

    query(
        "CREATE TABLE posts (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id),
            editor_id BIGINT REFERENCES users(id),
            title TEXT NOT NULL
        )",
    )
    .execute(conn)
    .await?;

    Ok(())
}

/// Setup schema for products table (used in update_model example)
pub async fn setup_products_schema(conn: &impl GenericClient) -> Result<(), OrmError> {
    query("DROP TABLE IF EXISTS products CASCADE")
        .execute(conn)
        .await?;

    query(
        "CREATE TABLE products (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            price_cents BIGINT NOT NULL,
            in_stock BOOLEAN NOT NULL DEFAULT true
        )",
    )
    .execute(conn)
    .await?;

    Ok(())
}

/// Setup schema for products and categories tables (used in pg_client example)
pub async fn setup_products_categories_schema(conn: &impl GenericClient) -> Result<(), OrmError> {
    query("DROP TABLE IF EXISTS products CASCADE")
        .execute(conn)
        .await?;
    query("DROP TABLE IF EXISTS categories CASCADE")
        .execute(conn)
        .await?;

    query(
        "CREATE TABLE categories (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL
        )",
    )
    .execute(conn)
    .await?;

    query(
        "CREATE TABLE products (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            price_cents BIGINT NOT NULL,
            in_stock BOOLEAN NOT NULL DEFAULT true
        )",
    )
    .execute(conn)
    .await?;

    Ok(())
}
