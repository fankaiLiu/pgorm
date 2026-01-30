//! Example demonstrating eager loading (batch preload) for relations.
//!
//! Run with:
//!   cargo run --example eager_loading -p pgorm
//!
//! Set DATABASE_URL in `.env` or environment variable:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

mod common;

use common::setup_users_posts_schema;
use pgorm::{FromRow, GenericClient, Model, ModelPk as _, OrmError, create_pool, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
#[orm(belongs_to(User, foreign_key = "editor_id", as = "editor"))]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    editor_id: Option<i64>,
    title: String,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");
    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    setup_users_posts_schema(&client).await?;
    seed_data(&client).await?;

    // Base query (no eager loading yet)
    let users = User::select_all(&client).await?;
    let posts = Post::select_all(&client).await?;

    println!("\n== Base ==");
    println!("users: {}", users.len());
    println!("posts: {}", posts.len());

    // ----------------------------
    // Map style: has_many
    // ----------------------------
    println!("\n== Map style (has_many) ==");
    let posts_by_user = User::load_posts_map_with(&client, &users, |q| {
        q.push(" ORDER BY id DESC");
    })
    .await?;

    for u in &users {
        let count = posts_by_user.get(u.pk()).map(|v| v.len()).unwrap_or(0);
        println!("- user {} ({}) posts={}", u.id, u.name, count);
    }

    // ----------------------------
    // Attach style: has_many
    // ----------------------------
    println!("\n== Attach style (has_many) ==");
    let users_loaded = User::load_posts(&client, users.clone()).await?;
    for u in &users_loaded {
        println!("- user {} posts={}", u.id, u.rel.len());
    }

    // ----------------------------
    // Attach style: belongs_to (default Option)
    // ----------------------------
    println!("\n== Attach style (belongs_to) ==");
    let posts_with_author = Post::load_author(&client, posts.clone()).await?;
    for p in &posts_with_author {
        let author = p
            .rel
            .as_ref()
            .map(|u| u.name.as_str())
            .unwrap_or("(missing)");
        println!("- post {} title={:?} author={}", p.id, p.title, author);
    }

    // Optional belongs_to: editor_id is nullable → default attach is Option<User>.
    let posts_with_editor = Post::load_editor(&client, posts.clone()).await?;
    for p in &posts_with_editor {
        let editor = p.rel.as_ref().map(|u| u.name.as_str()).unwrap_or("(null)");
        println!("- post {} editor={}", p.id, editor);
    }

    // Strict variant: require every post has an editor.
    println!("\n== belongs_to strict (expected error if any editor_id is NULL) ==");
    match Post::load_editor_strict(&client, posts.clone()).await {
        Ok(_) => println!("unexpected: all posts have editor_id"),
        Err(e) => println!("expected error: {e}"),
    }

    Ok(())
}

async fn seed_data(conn: &impl GenericClient) -> Result<(), OrmError> {
    let alice: User = query("INSERT INTO users (name) VALUES ($1) RETURNING id, name")
        .bind("alice")
        .fetch_one_as(conn)
        .await?;
    let bob: User = query("INSERT INTO users (name) VALUES ($1) RETURNING id, name")
        .bind("bob")
        .fetch_one_as(conn)
        .await?;

    let _p1: Post = query(
        "INSERT INTO posts (user_id, editor_id, title)
         VALUES ($1, $2, $3)
         RETURNING id, user_id, editor_id, title",
    )
    .bind(alice.id)
    .bind(Some(bob.id))
    .bind("Hello, pgorm")
    .fetch_one_as(conn)
    .await?;

    let _p2: Post = query(
        "INSERT INTO posts (user_id, editor_id, title)
         VALUES ($1, $2, $3)
         RETURNING id, user_id, editor_id, title",
    )
    .bind(alice.id)
    .bind(None::<i64>)
    .bind("Editor is NULL (to demo strict)")
    .fetch_one_as(conn)
    .await?;

    let _p3: Post = query(
        "INSERT INTO posts (user_id, editor_id, title)
         VALUES ($1, $2, $3)
         RETURNING id, user_id, editor_id, title",
    )
    .bind(bob.id)
    .bind(Some(bob.id))
    .bind("Second author")
    .fetch_one_as(conn)
    .await?;

    Ok(())
}
