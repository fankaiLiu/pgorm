# SQL Safety & Checking

pgorm provides multiple safety layers to protect against SQL injection, dangerous queries, and schema mismatches -- especially important when SQL can be dynamically generated or produced by AI.

## 1. Dynamic Identifier Safety (`Ident`)

Column and table names passed to pgorm's builder types (`Condition`, `OrderBy`, `SetExpr`, `Pagination`) are validated through an internal `Ident` type. Only characters matching `[a-zA-Z0-9_]` and qualified names like `schema.table.column` are accepted.

**SQL injection via identifiers is not possible.**

```rust
use pgorm::prelude::*;

// These are safe -- identifiers are validated at construction time
Condition::eq("user_name", value)?;      // OK
OrderBy::new().asc("created_at")?;       // OK
SetExpr::set("status", "inactive")?;     // OK

// These return Err -- invalid characters are rejected
Condition::eq("name; DROP TABLE", v);    // Err
OrderBy::new().asc("col -- comment");    // Err
```

This means you can safely pass user-supplied column names to these builder types and pgorm will reject anything that looks like SQL injection.

## 2. Raw SQL Safety

Functions like `query("...")`, `sql("...")`, and `SetExpr::raw("...")` accept raw SQL strings that are passed directly to PostgreSQL. For these, always use `$1` parameter placeholders for user input. Never use string interpolation.

```rust
// SAFE: parameterized query
query("SELECT * FROM users WHERE id = $1").bind(user_id);

// SAFE: sql() with push_bind (auto-generates $n placeholders)
let mut q = sql("SELECT * FROM users WHERE status = ");
q.push_bind("active");
```

```rust
// UNSAFE: string interpolation -- DO NOT do this
query(&format!("SELECT * FROM users WHERE id = {user_id}"));
```

The rule is simple: if the value comes from user input, it must go through `.bind()` or `.push_bind()`, never into the SQL string itself.

## 3. SQL Safety Policies (`PgClientConfig`)

`PgClient` can enforce runtime policies that catch dangerous query patterns before they reach the database.

### Policy Table

| Policy | Options | Default |
|--------|---------|---------|
| `select_without_limit` | `Allow`, `Warn`, `Error`, `AutoLimit(n)` | `Allow` |
| `delete_without_where` | `Allow`, `Warn`, `Error` | `Allow` |
| `update_without_where` | `Allow`, `Warn`, `Error` | `Allow` |
| `truncate` | `Allow`, `Warn`, `Error` | `Allow` |
| `drop_table` | `Allow`, `Warn`, `Error` | `Allow` |

### Configuration

```rust
use pgorm::{PgClient, PgClientConfig, DangerousDmlPolicy, SelectWithoutLimitPolicy};

// Individual policy configuration
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000))
    .truncate_policy(DangerousDmlPolicy::Error)
    .drop_table_policy(DangerousDmlPolicy::Error));
```

### `strict()` Shorthand

The `strict()` method enables strict SQL checking and sensible safety defaults in one call:

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new().strict());
```

### Policy Behavior

- **`Allow`** -- no restriction, the query executes normally
- **`Warn`** -- logs a warning but still executes the query
- **`Error`** -- returns `OrmError::Validation` before the query reaches the database
- **`AutoLimit(n)`** -- (SELECT only) automatically appends `LIMIT n` if the query has no LIMIT clause

## 4. Runtime SQL Schema Checking

With the `check` feature (enabled by default), `PgClient` validates SQL statements against registered `#[derive(Model)]` schemas at runtime. This catches references to non-existent tables or columns before the query is sent to PostgreSQL.

### Check Modes

- **`Disabled`** -- no checking at all
- **`WarnOnly`** (default) -- logs warnings for unknown tables/columns but still executes the query
- **`Strict`** -- returns an error for unknown tables/columns, preventing execution

### How It Works

Models annotated with `#[derive(Model)]` are automatically registered in a global schema registry via the `inventory` crate. When `PgClient` receives a query, it parses the SQL and validates table/column references against the registry.

```rust
use pgorm::{CheckMode, PgClient, PgClientConfig, query, FromRow, Model};

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    in_stock: bool,
}

// Default: WarnOnly mode
let pg = PgClient::new(&client);

// Explicit mode selection
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());
let pg_warn = PgClient::with_config(&client, PgClientConfig::new().check_mode(CheckMode::WarnOnly));
let pg_off = PgClient::with_config(&client, PgClientConfig::new().no_check());
```

### Strict Mode in Action

```rust
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

// This works -- `id` and `name` exist on `products`
let rows = query("SELECT id, name FROM products")
    .fetch_all(&pg_strict)
    .await?;

// This fails before hitting the DB -- `email` does not exist on `products`
let result = query("SELECT id, email FROM products")
    .fetch_all(&pg_strict)
    .await;
// result is Err(OrmError::Validation(...))

// This fails -- table `orders` is not registered
let result = query("SELECT id FROM orders")
    .fetch_all(&pg_strict)
    .await;
// result is Err(OrmError::Validation(...))
```

### Direct Schema Validation

You can also check SQL directly against the registry without executing it:

```rust
let pg = PgClient::new(&client);

let issues = pg.registry().check_sql("SELECT id, email FROM products");
for issue in &issues {
    println!("{:?}: {}", issue.kind, issue.message);
}
```

### `CheckedClient` -- Lightweight Alternative

If you only need schema validation without monitoring or safety policies, use `CheckedClient`:

```rust
use pgorm::{CheckedClient, query};

let checked = CheckedClient::new(&client).strict();

let _ = query("SELECT id, name FROM products")
    .fetch_all(&checked)
    .await?;
```

## 5. `check_models!` Macro

The `check_models!` macro validates all of a model's generated SQL against a schema registry at once. This is useful for startup validation or CI checks:

```rust
use pgorm::{check_models, SchemaRegistry, Model, FromRow};

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "orders")]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
}

let registry = SchemaRegistry::new();
// ... register tables ...

let results = check_models!(registry, User, Order);
for (name, issues) in &results {
    if issues.is_empty() {
        println!("  {} OK", name);
    } else {
        println!("  {} has {} issue(s)", name, issues.len());
    }
}
```

There is also `assert_models_valid!` which panics if any model has schema issues -- useful for startup guards:

```rust
// Panics with a detailed message if any model fails validation
pgorm::assert_models_valid!(registry, User, Order);
```

## Combining Everything

In a typical production setup, you would combine schema checking, safety policies, monitoring, and statement caching:

```rust
use pgorm::{PgClient, PgClientConfig, CheckMode, DangerousDmlPolicy, SelectWithoutLimitPolicy};
use std::time::Duration;

let pg = PgClient::with_config(&client, PgClientConfig::new()
    .check_mode(CheckMode::WarnOnly)
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_millis(100))
    .with_stats()
    .log_slow_queries(Duration::from_millis(50))
    .statement_cache(128)
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000)));
```

## Runnable Example

See `crates/pgorm/examples/pg_client/main.rs` for a complete example demonstrating the schema registry, check modes (Strict/WarnOnly), query statistics, and safety policies.

## Next

- Next: [Input Validation](/en/guide/validation)
