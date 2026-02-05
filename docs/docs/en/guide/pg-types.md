# PostgreSQL Types

pgorm is built on `tokio-postgres` and inherits its type system. This page covers built-in type mappings, custom PostgreSQL types via derive macros, range types, JSONB, and feature-gated extra types.

## Built-in Type Mapping

These types work out of the box with `query()`, `sql()`, `FromRow`, and `InsertModel`:

| Rust type | PostgreSQL type |
|-----------|-----------------|
| `bool` | `bool` |
| `i8` | `"char"` |
| `i16` | `smallint` |
| `i32` | `int` |
| `i64` | `bigint` |
| `f32` | `real` |
| `f64` | `double precision` |
| `String` / `&str` | `text`, `varchar`, `char(n)` |
| `Vec<u8>` / `&[u8]` | `bytea` |
| `uuid::Uuid` | `uuid` |
| `chrono::NaiveDate` | `date` |
| `chrono::NaiveTime` | `time` |
| `chrono::NaiveDateTime` | `timestamp` |
| `chrono::DateTime<Utc>` | `timestamptz` |
| `std::net::IpAddr` | `inet` |
| `serde_json::Value` | `json`, `jsonb` |
| `Option<T>` | nullable column |

## `#[derive(PgEnum)]`

Maps a Rust enum to a PostgreSQL ENUM type. Each variant is mapped to a string label.

### Definition

```rust
use pgorm::PgEnum;

#[derive(PgEnum, Debug, Clone, PartialEq)]
#[orm(pg_type = "order_status")]
pub enum OrderStatus {
    #[orm(rename = "pending")]
    Pending,
    #[orm(rename = "processing")]
    Processing,
    #[orm(rename = "shipped")]
    Shipped,
    #[orm(rename = "delivered")]
    Delivered,
    #[orm(rename = "cancelled")]
    Cancelled,
}
```

The `#[orm(pg_type = "order_status")]` attribute must match the name of the PostgreSQL type created with `CREATE TYPE`:

```sql
CREATE TYPE order_status AS ENUM (
    'pending', 'processing', 'shipped', 'delivered', 'cancelled'
);
```

### Usage

PgEnum values can be used directly in `bind()` and read back with `RowExt::try_get_column`:

```rust
use pgorm::{RowExt, query};

// Insert with enum value
let row = query(
    "INSERT INTO orders (user_id, status, total) VALUES ($1, $2, $3) RETURNING id, status"
)
    .bind(1_i64)
    .bind(OrderStatus::Pending)
    .bind(99.99_f64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let status: OrderStatus = row.try_get_column("status")?;

// Query by enum value
let rows = query("SELECT id, status FROM orders WHERE status = $1")
    .bind(OrderStatus::Pending)
    .fetch_all(&client)
    .await?;

// Update enum value
query("UPDATE orders SET status = $1 WHERE id = $2")
    .bind(OrderStatus::Delivered)
    .bind(id)
    .execute(&client)
    .await?;
```

## `#[derive(PgComposite)]`

Maps a Rust struct to a PostgreSQL composite type.

### Definition

```rust
use pgorm::PgComposite;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
    pub country: String,
}
```

The PostgreSQL type must be created with matching field names:

```sql
CREATE TYPE address AS (
    street TEXT,
    city TEXT,
    zip_code TEXT,
    country TEXT
);
```

### Usage

```rust
use pgorm::{RowExt, query};

let addr = Address {
    street: "123 Main St".into(),
    city: "San Francisco".into(),
    zip_code: "94105".into(),
    country: "USA".into(),
};

// Insert with composite value
let row = query(
    "INSERT INTO contacts (name, home_address) VALUES ($1, $2) RETURNING id, name, home_address"
)
    .bind("Alice")
    .bind(addr)
    .fetch_one(&client)
    .await?;

// Read composite value
let read_addr: Address = row.try_get_column("home_address")?;
println!("{}, {}", read_addr.city, read_addr.country);

// Query on composite field (PostgreSQL syntax)
let rows = query("SELECT id, name FROM contacts WHERE (home_address).city = $1")
    .bind("San Francisco")
    .fetch_all(&client)
    .await?;
```

## `Range<T>`

pgorm provides a `Range<T>` type for PostgreSQL range columns (`int4range`, `int8range`, `numrange`, `tsrange`, `tstzrange`, `daterange`).

### Constructors

```rust
use pgorm::types::Range;

Range::<i32>::inclusive(1, 10);   // [1, 10]  -- both bounds inclusive
Range::<i32>::exclusive(1, 10);  // (1, 10)  -- both bounds exclusive
Range::<i32>::lower_inc(1, 10);  // [1, 10)  -- lower inclusive, upper exclusive
Range::<i32>::empty();           // empty range
Range::<i32>::unbounded();       // (-inf, +inf)
```

### Insert and Read

```rust
use chrono::{DateTime, Duration, Utc};
use pgorm::{RowExt, query};
use pgorm::types::Range;

let now: DateTime<Utc> = Utc::now();

// Insert a tstzrange
query("INSERT INTO events (name, during) VALUES ($1, $2)")
    .bind("Team Meeting")
    .bind(Range::lower_inc(now, now + Duration::hours(2)))
    .execute(&client)
    .await?;

// Read back a range
let row = query("SELECT during FROM events WHERE name = $1")
    .bind("Team Meeting")
    .fetch_one(&client)
    .await?;
let during: Range<DateTime<Utc>> = row.try_get_column("during")?;

// Insert a daterange
use chrono::NaiveDate;
let today = Utc::now().date_naive();
query("INSERT INTO bookings (room, reserved) VALUES ($1, $2)")
    .bind("Room A")
    .bind(Range::lower_inc(today, today + chrono::Days::new(3)))
    .execute(&client)
    .await?;
```

### Range Condition Operators

pgorm provides condition helpers for range queries, usable with the `sql()` builder:

```rust
use pgorm::{Condition, sql};
use pgorm::types::Range;

// Overlap (&&): find events that overlap with a time window
let query_range = Range::lower_inc(now, now + Duration::hours(1));
let mut q = sql("SELECT id, name FROM events");
q.push(" WHERE ");
Condition::overlaps("during", query_range)?.append_to_sql(&mut q);

// Contains (@>): find events containing a specific timestamp
let mut q = sql("SELECT id, name FROM events");
q.push(" WHERE ");
Condition::contains("during", now)?.append_to_sql(&mut q);

// Left of (<<): range is strictly left of another range
Condition::range_left_of("r", range)?;

// Right of (>>): range is strictly right of another range
Condition::range_right_of("r", range)?;

// Adjacent (-|-): ranges are adjacent (no gap, no overlap)
Condition::range_adjacent("r", range)?;
```

## JSONB

pgorm supports JSONB in two styles: strongly-typed with `Json<T>` and dynamic with `serde_json::Value`.

### Strongly-Typed: `Json<T>`

```rust
use pgorm::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
    active: bool,
}

// Insert typed JSONB
let typed_meta = Json(Meta {
    tags: vec!["admin".into(), "staff".into()],
    active: true,
});
let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
    .bind(typed_meta)
    .fetch_one(&client)
    .await?;

// Read typed JSONB
let meta: Json<Meta> = row.try_get_column("meta")?;
println!("tags: {:?}", meta.0.tags);
```

### Dynamic: `serde_json::Value`

```rust
// Insert dynamic JSONB
let dynamic_meta = serde_json::json!({"theme": "dark", "beta": true});
let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
    .bind(dynamic_meta)
    .fetch_one(&client)
    .await?;

// Read dynamic JSONB
let meta: serde_json::Value = row.try_get_column("meta")?;

// Query inside JSONB
let theme: Option<String> = query("SELECT meta->>'theme' FROM users WHERE id = $1")
    .bind(id)
    .fetch_scalar_opt(&client)
    .await?;
```

## INET (IP Address)

Map PostgreSQL `inet` columns to `std::net::IpAddr`:

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct AuditLog {
    id: i64,
    ip_address: Option<std::net::IpAddr>,
}
```

Filtering:

```rust
let ip: std::net::IpAddr = "1.2.3.4".parse()?;
let rows: Vec<AuditLog> = query("SELECT id, ip_address FROM audit_logs WHERE ip_address = $1")
    .bind(ip)
    .fetch_all_as(&client)
    .await?;
```

## Feature-Gated Extra Types

Some types require enabling optional features. When enabled, pgorm adds `PgType` implementations for UNNEST bulk writes and enables the corresponding `tokio-postgres` `with-*` features.

| Rust type | PostgreSQL type | pgorm feature | Notes |
|-----------|-----------------|---------------|-------|
| `rust_decimal::Decimal` | `numeric` | `rust_decimal` | Also add `rust_decimal` as a direct dependency |
| `time::Date` / `Time` / `PrimitiveDateTime` / `OffsetDateTime` | `date` / `time` / `timestamp` / `timestamptz` | `time` | Enables `tokio-postgres/with-time-0_3` |
| `cidr::IpCidr` / `cidr::IpInet` | `cidr` / `inet` | `cidr` | Enables `tokio-postgres/with-cidr-0_3` |
| `geo_types::Point<f64>` / `Rect<f64>` / `LineString<f64>` | `point` / `box` / `path` | `geo_types` | Enables `tokio-postgres/with-geo-types-0_7` |
| `eui48::MacAddress` | `macaddr` | `eui48` | Enables `tokio-postgres/with-eui48-1` |
| `bit_vec::BitVec` | `bit` / `varbit` | `bit_vec` | Enables `tokio-postgres/with-bit-vec-0_8` |

Enable all at once:

```toml
[dependencies]
pgorm = { version = "0.2.0", features = ["extra_types"] }
```

Note: even if you enable a pgorm feature, you still need to add the corresponding type crate (`time`, `cidr`, `geo-types`, etc.) as a direct dependency to reference those types in your code.

## `RowExt` for Manual Access

When you work with raw rows rather than `FromRow` structs, use `RowExt::try_get_column` for typed access with consistent error handling:

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, status FROM orders WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let status: OrderStatus = row.try_get_column("status")?;
```

This works with all supported types including `PgEnum`, `PgComposite`, `Range<T>`, `Json<T>`, and `serde_json::Value`.

---

Next: [Writes: Insert, Update, Upsert](/en/guide/writes)
