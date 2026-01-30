# Runtime SQL Checking: `PgClient` / `CheckedClient`

When SQL is generated at runtime (especially AI-generated or heavily dynamic SQL), you’ll often hit:

1) referencing non-existent tables/columns  
2) accidentally running dangerous SQL (e.g. `DELETE` without `WHERE`)  

pgorm provides runtime checking as a safety net: validate SQL *before* it is sent to Postgres, based on registered schemas and safety policies.

## 0) Prerequisite: enable the `check` feature

Default features include `check`. If you disabled defaults, enable it explicitly:

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["check"] }
```

## 1) Recommended: `PgClient` (check + monitoring + policies)

`PgClient` is the “batteries-included” wrapper:

- auto-register schemas from `#[derive(Model)]` (via `inventory`)
- validate SQL (WarnOnly / Strict / Disabled)
- runtime safety policies (dangerous DML, etc.)
- stats/logging/slow query detection (configurable)

```rust
use pgorm::{CheckMode, PgClient, PgClientConfig, query};
use std::time::Duration;

let pg = PgClient::with_config(
    &client,
    PgClientConfig::new()
        .check_mode(CheckMode::WarnOnly) // default
        .timeout(Duration::from_secs(30))
        .slow_threshold(Duration::from_millis(100))
        .with_stats(),
);

let n: i64 = query("SELECT COUNT(*) FROM products")
    .tag("products.count")
    .fetch_scalar_one(&pg)
    .await?;
```

### Strict mode: block invalid SQL before execution

```rust
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

// If the SQL references missing tables/columns, this fails before hitting the DB.
let _ = query("SELECT nonexistent FROM products")
    .fetch_all(&pg_strict)
    .await?;
```

> `strict()` affects runtime SQL checking/policies only. It does not change row-count semantics; see [`Fetch Semantics`](/en/guide/fetch-semantics).

## 2) Lightweight option: `CheckedClient` (schema checking only)

If you only want schema validation:

```rust
use pgorm::{CheckedClient, query};

let checked = CheckedClient::new(&client).strict();

let _ = query("SELECT id, name FROM products")
    .fetch_all(&checked)
    .await?;
```

## 3) Runtime safety policies

`PgClientConfig` includes policy knobs such as:

- SELECT without LIMIT
- DELETE without WHERE
- UPDATE without WHERE
- TRUNCATE / DROP TABLE

In production, you’ll typically choose stricter policies (especially when SQL can be generated).

## 4) Runnable example

- `crates/pgorm/examples/pg_client`: registry, check results, Strict/WarnOnly, query stats

## Next

- Next: [`Input Validation: #[orm(input)]`](/en/guide/validation-and-input)
