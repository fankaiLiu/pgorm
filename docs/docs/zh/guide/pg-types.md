# PostgreSQL 类型

pgorm 基于 `tokio-postgres` 构建，继承了其类型系统。本页涵盖内置类型映射、通过派生宏定义的自定义 PostgreSQL 类型、范围类型、JSONB 以及需要功能标志的额外类型。

## 内置类型映射

以下类型开箱即用，可直接在 `query()`、`sql()`、`FromRow` 和 `InsertModel` 中使用：

| Rust 类型 | PostgreSQL 类型 |
|-----------|----------------|
| `bool` | `bool` |
| `i8` | `"char"` |
| `i16` | `smallint` |
| `i32` | `int` |
| `i64` | `bigint` |
| `f32` | `real` |
| `f64` | `double precision` |
| `String` / `&str` | `text`、`varchar`、`char(n)` |
| `Vec<u8>` / `&[u8]` | `bytea` |
| `uuid::Uuid` | `uuid` |
| `chrono::NaiveDate` | `date` |
| `chrono::NaiveTime` | `time` |
| `chrono::NaiveDateTime` | `timestamp` |
| `chrono::DateTime<Utc>` | `timestamptz` |
| `std::net::IpAddr` | `inet` |
| `serde_json::Value` | `json`、`jsonb` |
| `Option<T>` | 可空列 |

## `#[derive(PgEnum)]`

将 Rust 枚举映射到 PostgreSQL ENUM 类型。每个变体映射为一个字符串标签。

### 定义

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

`#[orm(pg_type = "order_status")]` 属性必须与使用 `CREATE TYPE` 创建的 PostgreSQL 类型名称匹配：

```sql
CREATE TYPE order_status AS ENUM (
    'pending', 'processing', 'shipped', 'delivered', 'cancelled'
);
```

### 用法

PgEnum 值可以直接在 `bind()` 中使用，也可以通过 `RowExt::try_get_column` 读取：

```rust
use pgorm::{RowExt, query};

// 使用枚举值插入
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

// 按枚举值查询
let rows = query("SELECT id, status FROM orders WHERE status = $1")
    .bind(OrderStatus::Pending)
    .fetch_all(&client)
    .await?;

// 更新枚举值
query("UPDATE orders SET status = $1 WHERE id = $2")
    .bind(OrderStatus::Delivered)
    .bind(id)
    .execute(&client)
    .await?;
```

## `#[derive(PgComposite)]`

将 Rust 结构体映射到 PostgreSQL 复合类型。

### 定义

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

PostgreSQL 类型必须以匹配的字段名创建：

```sql
CREATE TYPE address AS (
    street TEXT,
    city TEXT,
    zip_code TEXT,
    country TEXT
);
```

### 用法

```rust
use pgorm::{RowExt, query};

let addr = Address {
    street: "123 Main St".into(),
    city: "San Francisco".into(),
    zip_code: "94105".into(),
    country: "USA".into(),
};

// 使用复合类型值插入
let row = query(
    "INSERT INTO contacts (name, home_address) VALUES ($1, $2) RETURNING id, name, home_address"
)
    .bind("Alice")
    .bind(addr)
    .fetch_one(&client)
    .await?;

// 读取复合类型值
let read_addr: Address = row.try_get_column("home_address")?;
println!("{}, {}", read_addr.city, read_addr.country);

// 查询复合类型字段（PostgreSQL 语法）
let rows = query("SELECT id, name FROM contacts WHERE (home_address).city = $1")
    .bind("San Francisco")
    .fetch_all(&client)
    .await?;
```

## `Range<T>`

pgorm 提供了 `Range<T>` 类型，用于 PostgreSQL 范围列（`int4range`、`int8range`、`numrange`、`tsrange`、`tstzrange`、`daterange`）。

### 构造器

```rust
use pgorm::types::Range;

Range::<i32>::inclusive(1, 10);   // [1, 10]  -- 两端包含
Range::<i32>::exclusive(1, 10);  // (1, 10)  -- 两端不包含
Range::<i32>::lower_inc(1, 10);  // [1, 10)  -- 下界包含，上界不包含
Range::<i32>::empty();           // 空范围
Range::<i32>::unbounded();       // (-inf, +inf)
```

### 插入和读取

```rust
use chrono::{DateTime, Duration, Utc};
use pgorm::{RowExt, query};
use pgorm::types::Range;

let now: DateTime<Utc> = Utc::now();

// 插入 tstzrange
query("INSERT INTO events (name, during) VALUES ($1, $2)")
    .bind("Team Meeting")
    .bind(Range::lower_inc(now, now + Duration::hours(2)))
    .execute(&client)
    .await?;

// 读取范围值
let row = query("SELECT during FROM events WHERE name = $1")
    .bind("Team Meeting")
    .fetch_one(&client)
    .await?;
let during: Range<DateTime<Utc>> = row.try_get_column("during")?;

// 插入 daterange
use chrono::NaiveDate;
let today = Utc::now().date_naive();
query("INSERT INTO bookings (room, reserved) VALUES ($1, $2)")
    .bind("Room A")
    .bind(Range::lower_inc(today, today + chrono::Days::new(3)))
    .execute(&client)
    .await?;
```

### 范围条件操作符

pgorm 提供了范围查询的条件辅助方法，可与 `sql()` 构建器配合使用：

```rust
use pgorm::{Condition, sql};
use pgorm::types::Range;

// 重叠（&&）：查找与时间窗口重叠的事件
let query_range = Range::lower_inc(now, now + Duration::hours(1));
let mut q = sql("SELECT id, name FROM events");
q.push(" WHERE ");
Condition::overlaps("during", query_range)?.append_to_sql(&mut q);

// 包含（@>）：查找包含特定时间戳的事件
let mut q = sql("SELECT id, name FROM events");
q.push(" WHERE ");
Condition::contains("during", now)?.append_to_sql(&mut q);

// 左侧（<<）：范围严格在另一个范围左侧
Condition::range_left_of("r", range)?;

// 右侧（>>）：范围严格在另一个范围右侧
Condition::range_right_of("r", range)?;

// 相邻（-|-）：范围相邻（无间隙，无重叠）
Condition::range_adjacent("r", range)?;
```

## JSONB

pgorm 支持两种 JSONB 风格：使用 `Json<T>` 的强类型方式和使用 `serde_json::Value` 的动态方式。

### 强类型：`Json<T>`

```rust
use pgorm::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
    active: bool,
}

// 插入强类型 JSONB
let typed_meta = Json(Meta {
    tags: vec!["admin".into(), "staff".into()],
    active: true,
});
let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
    .bind(typed_meta)
    .fetch_one(&client)
    .await?;

// 读取强类型 JSONB
let meta: Json<Meta> = row.try_get_column("meta")?;
println!("tags: {:?}", meta.0.tags);
```

### 动态：`serde_json::Value`

```rust
// 插入动态 JSONB
let dynamic_meta = serde_json::json!({"theme": "dark", "beta": true});
let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
    .bind(dynamic_meta)
    .fetch_one(&client)
    .await?;

// 读取动态 JSONB
let meta: serde_json::Value = row.try_get_column("meta")?;

// 查询 JSONB 内部
let theme: Option<String> = query("SELECT meta->>'theme' FROM users WHERE id = $1")
    .bind(id)
    .fetch_scalar_opt(&client)
    .await?;
```

## INET（IP 地址）

将 PostgreSQL `inet` 列映射到 `std::net::IpAddr`：

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct AuditLog {
    id: i64,
    ip_address: Option<std::net::IpAddr>,
}
```

过滤：

```rust
let ip: std::net::IpAddr = "1.2.3.4".parse()?;
let rows: Vec<AuditLog> = query("SELECT id, ip_address FROM audit_logs WHERE ip_address = $1")
    .bind(ip)
    .fetch_all_as(&client)
    .await?;
```

## 需要功能标志的额外类型

部分类型需要启用可选功能。启用后，pgorm 会为 UNNEST 批量写入添加 `PgType` 实现，并启用对应的 `tokio-postgres` `with-*` 功能。

| Rust 类型 | PostgreSQL 类型 | pgorm 功能标志 | 备注 |
|-----------|----------------|---------------|------|
| `rust_decimal::Decimal` | `numeric` | `rust_decimal` | 还需将 `rust_decimal` 作为直接依赖添加 |
| `time::Date` / `Time` / `PrimitiveDateTime` / `OffsetDateTime` | `date` / `time` / `timestamp` / `timestamptz` | `time` | 启用 `tokio-postgres/with-time-0_3` |
| `cidr::IpCidr` / `cidr::IpInet` | `cidr` / `inet` | `cidr` | 启用 `tokio-postgres/with-cidr-0_3` |
| `geo_types::Point<f64>` / `Rect<f64>` / `LineString<f64>` | `point` / `box` / `path` | `geo_types` | 启用 `tokio-postgres/with-geo-types-0_7` |
| `eui48::MacAddress` | `macaddr` | `eui48` | 启用 `tokio-postgres/with-eui48-1` |
| `bit_vec::BitVec` | `bit` / `varbit` | `bit_vec` | 启用 `tokio-postgres/with-bit-vec-0_8` |

一次性启用所有额外类型：

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["extra_types"] }
```

注意：即使启用了 pgorm 功能标志，你仍然需要将对应的类型 crate（`time`、`cidr`、`geo-types` 等）作为直接依赖添加，才能在代码中引用这些类型。

## 使用 `RowExt` 手动访问

当你使用原始行而非 `FromRow` 结构体时，可以使用 `RowExt::try_get_column` 进行类型化访问，获得一致的错误处理：

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, status FROM orders WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let status: OrderStatus = row.try_get_column("status")?;
```

这适用于所有支持的类型，包括 `PgEnum`、`PgComposite`、`Range<T>`、`Json<T>` 和 `serde_json::Value`。

---

下一步：[写入：插入、更新、Upsert](/zh/guide/writes)
