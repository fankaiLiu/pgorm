# pgorm Cookbook

Best practices and common patterns for building applications with pgorm.

---

## Table of Contents

- [Pagination: Page vs Keyset](#pagination-page-vs-keyset)
- [Dynamic WHERE / ORDER BY](#dynamic-where--order-by)
- [Transactions & Savepoints](#transactions--savepoints)
- [Optimistic Locking & Retry](#optimistic-locking--retry)
- [Bulk Insert / Upsert Performance](#bulk-insert--upsert-performance)

---

## Pagination: Page vs Keyset

### Page-based (OFFSET/LIMIT)

Simple to implement, good for admin panels or back-office UIs where total count matters.

```rust
use pgorm::prelude::*;

// Page 3, 20 items per page → OFFSET 40 LIMIT 20
let mut q = sql("SELECT * FROM products WHERE in_stock = true");
Pagination::page(3, 20)?.append_to_sql(&mut q);
let products: Vec<Product> = q.fetch_all_as(&client).await?;
```

**Downsides:** Performance degrades on large offsets (PostgreSQL still scans skipped rows). Results can shift if rows are inserted/deleted between pages.

### Keyset-based (Cursor)

Recommended for infinite scroll, APIs, and any large dataset. Stable and index-friendly.

```rust
use pgorm::prelude::*;

// Stable order: created_at DESC, id DESC (tie-breaker)
let mut keyset = Keyset2::desc("created_at", "id")?.limit(20);
let mut where_expr = WhereExpr::and(Vec::new());

// For subsequent pages, pass the last row's cursor values
if let (Some(last_ts), Some(last_id)) = (after_created_at, after_id) {
    keyset = keyset.after(last_ts, last_id);
    where_expr = where_expr.and_with(keyset.into_where_expr()?);
}

// Combine with other filters
if let Some(status) = &filter_status {
    where_expr = where_expr.and_with(Condition::eq("status", status.clone())?.into());
}

let mut q = sql("SELECT id, name, status, created_at FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}
keyset.append_order_by_limit_to_sql(&mut q)?;

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

**When to use which:**

| | Page-based | Keyset |
|---|---|---|
| Best for | Admin UIs, small datasets | APIs, infinite scroll, large datasets |
| Performance | O(offset) — degrades at high pages | O(1) — consistent regardless of position |
| Stability | Rows can shift between pages | Stable cursor, no duplicates |
| Random access | Yes (jump to page N) | No (sequential only) |
| Implementation | `Pagination::page(n, size)` | `Keyset1` / `Keyset2` |

**Tips:**
- Always include a unique tie-breaker column (usually `id`) in keyset pagination
- `Keyset1` is for single-column ordering, `Keyset2` for two-column ordering
- Return cursor values in your API response so the client can request the next page

---

## Dynamic WHERE / ORDER BY

### Safe dynamic identifiers

All column names passed to `Condition`, `OrderBy`, `SetExpr`, etc. are validated through `Ident`. Only `[a-zA-Z0-9_]` and dot-qualified names are accepted.

```rust
use pgorm::prelude::*;

// Building filters from user input (e.g., API query params)
fn build_user_query(params: &QueryParams) -> OrmResult<Sql> {
    let mut where_expr = WhereExpr::and(Vec::new());

    // Each Condition validates the column name via Ident
    if let Some(status) = &params.status {
        where_expr = where_expr.and_with(Condition::eq("status", status.clone())?.into());
    }

    if let Some(search) = &params.search {
        where_expr = where_expr.and_with(
            Condition::ilike("name", format!("%{search}%"))?.into()
        );
    }

    // OR groups
    if !params.roles.is_empty() {
        let mut role_conditions = Vec::new();
        for role in &params.roles {
            role_conditions.push(Condition::eq("role", role.clone())?.into());
        }
        where_expr = where_expr.and_with(WhereExpr::or(role_conditions));
    }

    // Null checks
    if !params.include_deleted {
        where_expr = where_expr.and_with(Condition::is_null("deleted_at")?.into());
    }

    let mut q = sql("SELECT id, name, status, role FROM users");
    if !where_expr.is_trivially_true() {
        q.push(" WHERE ");
        where_expr.append_to_sql(&mut q);
    }

    Ok(q)
}
```

### Safe dynamic ORDER BY

```rust
use pgorm::prelude::*;

// User-specified sort column — validated by Ident
fn apply_sort(q: &mut Sql, sort_by: &str, sort_dir: &str) -> OrmResult<()> {
    let mut order = OrderBy::new();
    order = match sort_dir {
        "desc" => order.desc(sort_by)?,  // column name is validated
        _ => order.asc(sort_by)?,
    };
    order.append_to_sql(q);
    Ok(())
}

// With nulls handling
let order = OrderBy::new()
    .with_nulls("created_at", SortDir::Desc, NullsOrder::Last)?;
order.append_to_sql(&mut q);
```

**Important:** Even though identifiers are validated, always allowlist the columns your API accepts for sorting:

```rust
const ALLOWED_SORT_COLUMNS: &[&str] = &["id", "name", "created_at", "updated_at"];

fn validated_sort_column(input: &str) -> OrmResult<&str> {
    ALLOWED_SORT_COLUMNS
        .iter()
        .find(|&&c| c == input)
        .copied()
        .ok_or_else(|| OrmError::validation(format!("invalid sort column: {input}")))
}
```

---

## Transactions & Savepoints

### Basic transaction

```rust
use pgorm::prelude::*;

pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(100_i64).bind(from_id).execute(&tx).await?;
    query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
        .bind(100_i64).bind(to_id).execute(&tx).await?;
    Ok::<(), OrmError>(())
})?;
```

### Savepoints for partial rollback

Use savepoints when part of a transaction can fail without aborting the whole thing.

```rust
pgorm::transaction!(&mut client, tx, {
    // Main operation — always committed
    query("INSERT INTO orders (user_id, total) VALUES ($1, $2)")
        .bind(user_id).bind(total).execute(&tx).await?;

    // Optional side-effect — rollback if it fails, but keep the order
    let bonus_result: Result<(), OrmError> = pgorm::savepoint!(tx, "apply_bonus", sp, {
        query("UPDATE loyalty_points SET points = points + $1 WHERE user_id = $2")
            .bind(bonus_points).bind(user_id).execute(&sp).await?;
        Ok(())
    });

    if bonus_result.is_err() {
        eprintln!("Bonus points failed, but order is still committed");
    }

    Ok::<(), OrmError>(())
})?;
```

### Nested transactions

Use `nested_transaction!` for composable transactional functions:

```rust
// A function that can be called from any transactional context
async fn create_audit_log(tx: &impl pgorm::GenericClient, action: &str) -> OrmResult<()> {
    query("INSERT INTO audit_log (action, created_at) VALUES ($1, NOW())")
        .bind(action).execute(tx).await?;
    Ok(())
}

pgorm::transaction!(&mut client, tx, {
    query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&tx).await?;

    // nested_transaction! creates an anonymous savepoint
    pgorm::nested_transaction!(tx, inner, {
        create_audit_log(&inner, "user_deleted").await?;
        Ok::<(), OrmError>(())
    })?;

    Ok::<(), OrmError>(())
})?;
```

**Tips:**
- `transaction!` auto-commits on `Ok`, auto-rollbacks on `Err`
- `savepoint!` auto-releases on `Ok`, auto-rollbacks on `Err`
- Always type-annotate the `Ok` value: `Ok::<(), OrmError>(())`
- Savepoints are PostgreSQL-native and have minimal overhead

---

## Optimistic Locking & Retry

### Setup

Add a `version` column to your table and annotate it with `#[orm(version)]`:

```sql
CREATE TABLE articles (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    version INT NOT NULL DEFAULT 0
);
```

```rust
#[derive(Debug, FromRow, Model)]
#[orm(table = "articles")]
struct Article {
    #[orm(id)]
    id: i64,
    title: String,
    body: String,
    version: i32,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]  // auto-checked in WHERE, auto-incremented in SET
    version: i32,
}
```

### Basic usage

```rust
// Read current version
let article = Article::find_by_id(&client, article_id).await?;

// Update with version check
let patch = ArticlePatch {
    title: Some("Updated Title".into()),
    body: None,
    version: article.version,  // pass the version you read
};

match patch.update_by_id_returning(&client, article.id).await {
    Ok(updated) => {
        // Success — version was incremented automatically
        println!("New version: {}", updated.version);
    }
    Err(OrmError::StaleRecord { table, expected_version, .. }) => {
        // Another process modified the record between read and write
        println!("Conflict on {table}, expected version {expected_version}");
    }
    Err(e) => return Err(e),
}
```

### Retry pattern

For write-heavy scenarios, implement a retry loop:

```rust
async fn update_with_retry(
    client: &impl pgorm::GenericClient,
    article_id: i64,
    new_title: String,
    max_retries: u32,
) -> OrmResult<Article> {
    for attempt in 1..=max_retries {
        // Always re-fetch before retry
        let current: Article = query("SELECT * FROM articles WHERE id = $1")
            .bind(article_id)
            .fetch_one_as(&client)
            .await?;

        let patch = ArticlePatch {
            title: Some(new_title.clone()),
            body: None,
            version: current.version,
        };

        match patch.update_by_id_returning(client, article_id).await {
            Ok(updated) => return Ok(updated),
            Err(OrmError::StaleRecord { .. }) if attempt < max_retries => {
                // Optionally add jitter/backoff here
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

### Force update (admin override)

Skip the version check when you need to force an update:

```rust
// Version is still incremented, but no WHERE version=N check
let patch = ArticlePatch {
    title: Some("Admin Override".into()),
    body: None,
    version: 0,  // value is ignored for force updates
};

patch.update_by_id_force(&client, article_id).await?;
// or with RETURNING:
let updated = patch.update_by_id_force_returning(&client, article_id).await?;
```

**Tips:**
- `update_by_ids` (bulk) does NOT support version checking — use it only for admin operations
- The version column must be an integer type (`i32`, `i64`)
- Consider adding jitter to retry loops in high-concurrency scenarios

---

## Bulk Insert / Upsert Performance

### Batch insert with UNNEST

pgorm uses PostgreSQL's `UNNEST` for bulk inserts, which is significantly faster than individual `INSERT` statements or multi-row `VALUES`.

```rust
#[derive(InsertModel)]
#[orm(table = "events", returning = "Event")]
struct NewEvent {
    name: String,
    data: serde_json::Value,
    created_at: chrono::DateTime<chrono::Utc>,
}

// Insert 1000 rows in a single round-trip
let events: Vec<NewEvent> = (0..1000)
    .map(|i| NewEvent {
        name: format!("event_{i}"),
        data: serde_json::json!({"index": i}),
        created_at: chrono::Utc::now(),
    })
    .collect();

// Without RETURNING (fastest — no result parsing)
NewEvent::insert_many(&client, events).await?;

// With RETURNING (parses returned rows)
let inserted = NewEvent::insert_many_returning(&client, events).await?;
```

### Batch upsert (ON CONFLICT)

```rust
#[derive(InsertModel)]
#[orm(
    table = "metrics",
    conflict_target = "name, date",     // composite unique key
    conflict_update = "value, updated_at"  // columns to update on conflict
)]
struct MetricUpsert {
    name: String,
    date: chrono::NaiveDate,
    value: f64,
    updated_at: chrono::DateTime<chrono::Utc>,
}

// Batch upsert — inserts new rows, updates existing ones
MetricUpsert::upsert_many(&client, metrics).await?;
```

### Performance considerations

1. **Batch size:** For very large inserts (100k+ rows), consider batching into chunks of 5,000-10,000. Each `UNNEST` call sends all data in one query, and very large parameter lists can cause memory pressure.

```rust
for chunk in events.chunks(5000) {
    NewEvent::insert_many(&client, chunk.to_vec()).await?;
}
```

2. **Skip RETURNING when possible:** `insert_many` (without `_returning`) is faster because PostgreSQL doesn't need to serialize and send back the inserted rows.

3. **Indexes and constraints:** Temporarily disabling non-essential indexes for bulk loads can improve throughput. Re-enable and `REINDEX` after the load.

4. **COPY vs UNNEST:** For truly massive loads (millions of rows), consider using PostgreSQL's `COPY` protocol directly via `tokio-postgres`. pgorm's `UNNEST` approach is optimal for typical application workloads (hundreds to tens of thousands of rows).

5. **Transaction wrapping:** Bulk operations are already atomic (single statement). Wrapping them in an explicit transaction doesn't improve performance but can be useful for combining with other operations.

---

## General Tips

- **Always use `PgClient`** in production — it adds monitoring, SQL checking, and statement caching with minimal overhead.
- **Use `query()` for hand-written SQL**, `sql()` for dynamic SQL building. Both support `.bind()` and `.tag()`.
- **Tag your queries** with `.tag("module.operation")` for easier debugging in logs and metrics.
- **Enable `tracing` feature** to see actual SQL in your structured logs (target: `pgorm.sql`).
- **Use `PgClientConfig::strict()`** in development/staging to catch schema mismatches early.
