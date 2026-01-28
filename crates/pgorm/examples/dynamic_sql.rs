//! Dynamic SQL composition example
//!
//! Run with: cargo run --example dynamic_sql -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, sql, FromRow, OrmError};
use std::env;

#[derive(Debug, FromRow)]
#[allow(dead_code)]
struct Task {
    id: i64,
    title: String,
    status: String,
    priority: i32,
    assignee: Option<String>,
}

/// Search parameters - all optional
struct TaskFilter {
    status: Option<String>,
    min_priority: Option<i32>,
    assignee: Option<String>,
    title_contains: Option<String>,
}

/// Build a dynamic query based on filter parameters
async fn search_tasks<C: pgorm::GenericClient>(
    client: &C,
    filter: &TaskFilter,
) -> Result<Vec<Task>, OrmError> {
    // Start with base query - sql() will auto-generate $1, $2, etc.
    let mut q = sql("SELECT id, title, status, priority, assignee FROM tasks WHERE 1=1");

    // Add conditions only if filter values are present
    // Note: push_bind takes owned values, so we clone the strings
    if let Some(ref status) = filter.status {
        q.push(" AND status = ").push_bind(status.clone());
    }

    if let Some(min_priority) = filter.min_priority {
        q.push(" AND priority >= ").push_bind(min_priority);
    }

    if let Some(ref assignee) = filter.assignee {
        q.push(" AND assignee = ").push_bind(assignee.clone());
    }

    if let Some(ref title) = filter.title_contains {
        // ILIKE for case-insensitive search
        q.push(" AND title ILIKE ")
            .push_bind(format!("%{}%", title));
    }

    q.push(" ORDER BY priority DESC, id");

    q.fetch_all_as(client).await
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Read DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS tasks (
                id BIGSERIAL PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER NOT NULL DEFAULT 0,
                assignee TEXT
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute("DELETE FROM tasks", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    // Insert sample data
    let tasks = vec![
        ("Fix login bug", "in_progress", 3, Some("alice")),
        ("Add dark mode", "pending", 1, Some("bob")),
        ("Update documentation", "pending", 2, None),
        ("Refactor database layer", "in_progress", 2, Some("alice")),
        ("Fix memory leak", "done", 3, Some("charlie")),
        ("Add unit tests", "pending", 1, Some("bob")),
    ];

    for (title, status, priority, assignee) in tasks {
        let mut q = sql("INSERT INTO tasks (title, status, priority, assignee) VALUES (");
        q.push_bind(title)
            .push(", ")
            .push_bind(status)
            .push(", ")
            .push_bind(priority)
            .push(", ")
            .push_bind(assignee)
            .push(")");
        q.execute(&client).await?;
    }

    println!("=== Dynamic SQL Examples ===\n");

    // ============================================
    // Example 1: No filters
    // ============================================
    println!("All tasks:");
    let filter = TaskFilter {
        status: None,
        min_priority: None,
        assignee: None,
        title_contains: None,
    };
    let tasks = search_tasks(&client, &filter).await?;
    for task in &tasks {
        println!("  [{:?}] {} (priority: {})", task.status, task.title, task.priority);
    }

    // ============================================
    // Example 2: Filter by status
    // ============================================
    println!("\nPending tasks:");
    let filter = TaskFilter {
        status: Some("pending".to_string()),
        min_priority: None,
        assignee: None,
        title_contains: None,
    };
    let tasks = search_tasks(&client, &filter).await?;
    for task in &tasks {
        println!("  {} (priority: {})", task.title, task.priority);
    }

    // ============================================
    // Example 3: Filter by assignee and priority
    // ============================================
    println!("\nAlice's high priority tasks (>= 2):");
    let filter = TaskFilter {
        status: None,
        min_priority: Some(2),
        assignee: Some("alice".to_string()),
        title_contains: None,
    };
    let tasks = search_tasks(&client, &filter).await?;
    for task in &tasks {
        println!(
            "  {} [{}] (priority: {})",
            task.title, task.status, task.priority
        );
    }

    // ============================================
    // Example 4: Search by title
    // ============================================
    println!("\nTasks containing 'fix':");
    let filter = TaskFilter {
        status: None,
        min_priority: None,
        assignee: None,
        title_contains: Some("fix".to_string()),
    };
    let tasks = search_tasks(&client, &filter).await?;
    for task in &tasks {
        println!(
            "  {} [{}] (assignee: {:?})",
            task.title, task.status, task.assignee
        );
    }

    // ============================================
    // Example 5: Combined filters
    // ============================================
    println!("\nIn-progress tasks with priority >= 2:");
    let filter = TaskFilter {
        status: Some("in_progress".to_string()),
        min_priority: Some(2),
        assignee: None,
        title_contains: None,
    };
    let tasks = search_tasks(&client, &filter).await?;
    for task in &tasks {
        println!(
            "  {} (priority: {}, assignee: {:?})",
            task.title, task.priority, task.assignee
        );
    }

    // ============================================
    // Example 6: Using push_bind_list for IN clause
    // ============================================
    println!("\nTasks with status in ['pending', 'in_progress']:");
    let mut q = sql("SELECT id, title, status, priority, assignee FROM tasks WHERE status IN (");
    q.push_bind_list(vec!["pending", "in_progress"]);
    q.push(") ORDER BY priority DESC");

    let tasks: Vec<Task> = q.fetch_all_as(&client).await?;
    for task in &tasks {
        println!("  {} [{}]", task.title, task.status);
    }

    // ============================================
    // Example 7: Combining sql() queries
    // ============================================
    println!("\nCombining queries with push_sql:");
    let mut base = sql("SELECT id, title, status, priority, assignee FROM tasks");

    let mut conditions = sql(" WHERE priority > ");
    conditions.push_bind(1);
    conditions.push(" AND status != ").push_bind("done");

    base.push_sql(conditions);
    base.push(" ORDER BY id");

    let tasks: Vec<Task> = base.fetch_all_as(&client).await?;
    for task in &tasks {
        println!("  {} [{}] (priority: {})", task.title, task.status, task.priority);
    }

    Ok(())
}
