# PostgreSQL 特有类型支持设计与计划

状态：Draft
相关代码：`crates/pgorm/src/types/` / `crates/pgorm-derive`
最后更新：2026-02-05

## 背景

PostgreSQL 提供了丰富的原生类型，超越了标准 SQL。当前 `pgorm` 已支持常见类型（`uuid`、`jsonb`、`inet`、`timestamp` 等），但以下类型尚未完整支持：

1. **ENUM**：用户自定义枚举类型
2. **Range**：范围类型（`int4range`、`tstzrange` 等）
3. **Composite**：复合类型（用户自定义结构）

这些类型在业务系统中有广泛应用，缺乏支持会导致用户回退到字符串映射或手写 SQL。

## 目标 / 非目标

### 目标

1. **ENUM**：支持 Rust enum 与 PostgreSQL ENUM 的双向映射。
2. **Range**：支持范围类型的查询和插入。
3. **Composite**：支持复合类型的基本映射。
4. 提供 `PgType` 实现以支持批量插入（UNNEST）。
5. 提供 derive 宏简化类型定义。

### 非目标

- 从数据库自动生成 Rust 类型定义（可作为 CLI 扩展）。
- 复杂嵌套复合类型。
- Range 的所有操作符（`&&`、`@>`、`<@` 等优先；完整覆盖后续扩展）。

---

## 一、ENUM 类型支持

### 1.1 背景

PostgreSQL ENUM 是强类型枚举：

```sql
CREATE TYPE order_status AS ENUM ('pending', 'processing', 'shipped', 'delivered', 'cancelled');
```

当前用户需要：
1. 手写 `ToSql`/`FromSql` 实现
2. 或使用 `String` 映射并手动转换

### 1.2 方案

提供 `#[derive(PgEnum)]` 宏：

```rust,ignore
use pgorm::prelude::*;

#[derive(PgEnum, Debug, Clone, PartialEq)]
#[orm(pg_type = "order_status")]  // PostgreSQL ENUM 类型名
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

生成代码包括：
- `impl ToSql for OrderStatus`
- `impl<'a> FromSql<'a> for OrderStatus`
- `impl PgType for OrderStatus`（返回 `"order_status[]"`）

### 1.3 使用示例

```rust,ignore
#[derive(Model, InsertModel, FromRow)]
#[orm(table = "orders")]
pub struct Order {
    pub id: i64,
    pub user_id: i64,
    pub status: OrderStatus,  // 直接使用 enum
    pub created_at: DateTime<Utc>,
}

// 插入
let new_order = NewOrder {
    user_id: 1,
    status: OrderStatus::Pending,
};
new_order.insert(&client).await?;

// 查询
let pending: Vec<Order> = pgorm::sql("orders")
    .filter(Condition::eq("status", OrderStatus::Pending)?)
    .fetch_all(&client)
    .await?;

// 批量插入
let orders = vec![
    NewOrder { user_id: 1, status: OrderStatus::Pending },
    NewOrder { user_id: 2, status: OrderStatus::Processing },
];
NewOrder::insert_many(&orders, &client).await?;
```

### 1.4 属性说明

| 属性 | 作用 | 必须 |
|------|------|------|
| `#[orm(pg_type = "...")]` | PostgreSQL ENUM 类型名 | 是 |
| `#[orm(rename = "...")]` | 变体对应的 SQL 值 | 否（默认 snake_case） |

### 1.5 类型转换规则

默认使用 `snake_case`：
- `Pending` -> `"pending"`
- `InProgress` -> `"in_progress"`

可通过 `#[orm(rename = "...")]` 覆盖。

---

## 二、Range 类型支持

### 2.1 背景

PostgreSQL Range 类型用于表示值区间：

```sql
CREATE TABLE events (
    id SERIAL PRIMARY KEY,
    name TEXT,
    during TSTZRANGE  -- 时间范围
);

-- 查询与给定时间重叠的事件
SELECT * FROM events WHERE during && '[2024-01-01, 2024-02-01)'::tstzrange;
```

常见 Range 类型：
- `int4range` / `int8range`：整数范围
- `numrange`：NUMERIC 范围
- `tsrange` / `tstzrange`：时间戳范围
- `daterange`：日期范围

### 2.2 方案

提供泛型 `Range<T>` 类型：

```rust,ignore
use pgorm::types::Range;

/// PostgreSQL Range 类型
#[derive(Debug, Clone, PartialEq)]
pub struct Range<T> {
    pub lower: Option<Bound<T>>,
    pub upper: Option<Bound<T>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound<T> {
    Inclusive(T),
    Exclusive(T),
}

impl<T> Range<T> {
    pub fn new(lower: Option<Bound<T>>, upper: Option<Bound<T>>) -> Self;
    pub fn empty() -> Self;
    pub fn inclusive(lower: T, upper: T) -> Self;   // [lower, upper]
    pub fn exclusive(lower: T, upper: T) -> Self;   // (lower, upper)
    pub fn lower_inc(lower: T, upper: T) -> Self;   // [lower, upper)
    pub fn upper_inc(lower: T, upper: T) -> Self;   // (lower, upper]
}
```

### 2.3 类型映射

| PostgreSQL | Rust |
|------------|------|
| `int4range` | `Range<i32>` |
| `int8range` | `Range<i64>` |
| `numrange` | `Range<Decimal>` |
| `tsrange` | `Range<NaiveDateTime>` |
| `tstzrange` | `Range<DateTime<Utc>>` |
| `daterange` | `Range<NaiveDate>` |

### 2.4 使用示例

```rust,ignore
use pgorm::types::Range;
use chrono::{DateTime, Utc};

#[derive(Model, InsertModel, FromRow)]
#[orm(table = "events")]
pub struct Event {
    pub id: i64,
    pub name: String,
    pub during: Range<DateTime<Utc>>,
}

// 创建时间范围
let start = Utc::now();
let end = start + Duration::hours(2);
let during = Range::lower_inc(start, end);  // [start, end)

// 插入
let event = NewEvent {
    name: "Meeting".into(),
    during,
};
event.insert(&client).await?;

// 查询重叠的事件
let overlapping: Vec<Event> = pgorm::sql("events")
    .filter(Condition::overlaps("during", query_range)?)
    .fetch_all(&client)
    .await?;
```

### 2.5 Range 操作符

扩展 `Condition` 支持 Range 操作符：

| 操作符 | 含义 | Condition 方法 |
|--------|------|----------------|
| `&&` | 重叠 | `Condition::overlaps()` (已实现) |
| `@>` | 包含 | `Condition::contains()` (已实现) |
| `<@` | 被包含 | `Condition::contained_by()` (已实现) |
| `<<` | 严格左侧 | `Condition::range_left_of()` |
| `>>` | 严格右侧 | `Condition::range_right_of()` |
| `-|-` | 相邻 | `Condition::range_adjacent()` |

---

## 三、Composite 类型支持

### 3.1 背景

PostgreSQL Composite 类型是用户定义的结构：

```sql
CREATE TYPE address AS (
    street TEXT,
    city TEXT,
    zip_code TEXT,
    country TEXT
);

CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name TEXT,
    home_address address
);
```

### 3.2 方案

提供 `#[derive(PgComposite)]` 宏：

```rust,ignore
use pgorm::prelude::*;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
    pub country: String,
}
```

生成：
- `impl ToSql for Address`
- `impl<'a> FromSql<'a> for Address`
- `impl PgType for Address`

### 3.3 使用示例

```rust,ignore
#[derive(Model, InsertModel, FromRow)]
#[orm(table = "users")]
pub struct User {
    pub id: i64,
    pub name: String,
    pub home_address: Address,  // 复合类型
}

// 插入
let user = NewUser {
    name: "Alice".into(),
    home_address: Address {
        street: "123 Main St".into(),
        city: "San Francisco".into(),
        zip_code: "94105".into(),
        country: "USA".into(),
    },
};
user.insert(&client).await?;

// 查询
let user: User = pgorm::query("SELECT * FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

println!("Address: {}, {}", user.home_address.city, user.home_address.country);
```

### 3.4 访问复合类型字段

PostgreSQL 允许在查询中访问复合类型的字段：

```rust,ignore
// SELECT * FROM users WHERE (home_address).city = 'San Francisco'
let sf_users: Vec<User> = pgorm::sql("users")
    .filter(Condition::eq("(home_address).city", "San Francisco")?)
    .fetch_all(&client)
    .await?;
```

### 3.5 限制

- 不支持嵌套复合类型（复合类型内部再包含复合类型）
- 不支持复合类型数组（`address[]`）的 M1 阶段

---

## API 汇总

### derive 宏

```rust,ignore
#[derive(PgEnum)]      // ENUM 类型
#[derive(PgComposite)] // 复合类型
```

### Range 类型

```rust,ignore
use pgorm::types::{Range, Bound};

Range::<i32>::inclusive(1, 10);      // [1, 10]
Range::<DateTime<Utc>>::lower_inc(start, end);  // [start, end)
```

### PgType 实现

| 类型 | `pg_array_type()` 返回值 |
|------|--------------------------|
| `OrderStatus` (enum) | `"order_status[]"` |
| `Range<i32>` | `"int4range[]"` |
| `Range<i64>` | `"int8range[]"` |
| `Range<DateTime<Utc>>` | `"tstzrange[]"` |
| `Address` (composite) | `"address[]"` |

---

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| derive 宏 vs 手动实现 | 减少样板代码 | 增加宏复杂度 | **derive 宏** |
| 泛型 Range vs 具体类型 | 代码复用 | 类型推断复杂 | **泛型 + 类型别名** |
| 支持嵌套 vs 只支持扁平 | 更完整 | 实现复杂 | **M1 只支持扁平** |

---

## 兼容性与迁移

- 纯新增 API，不影响现有类型支持。
- 用户可以继续使用 `String` 映射或手写 `ToSql`/`FromSql`。

---

## 里程碑 / TODO

### M1（ENUM 支持）

- [ ] `#[derive(PgEnum)]` 宏
- [ ] `ToSql` / `FromSql` 生成
- [ ] `PgType` 实现
- [ ] `#[orm(rename = "...")]` 属性
- [ ] 单元测试

### M2（Range 支持）

- [ ] `Range<T>` 泛型类型
- [ ] 常用 Range 类型的 `ToSql` / `FromSql`
- [ ] Range 操作符扩展（`<<`, `>>`, `-|-`）
- [ ] `PgType` 实现
- [ ] 集成测试

### M3（Composite 支持）

- [ ] `#[derive(PgComposite)]` 宏
- [ ] `ToSql` / `FromSql` 生成
- [ ] `PgType` 实现
- [ ] 集成测试

### M4（文档与示例）

- [ ] `examples/pg_enum`
- [ ] `examples/pg_range`
- [ ] `examples/pg_composite`
- [ ] 中英文文档

---

## Open Questions

1. **ENUM**：是否支持从数据库反向生成 Rust enum？（建议作为 CLI 功能）
2. **Range**：是否提供 `Range::from_sql_literal("[1,10)")` 解析方法？（建议提供）
3. **Composite**：嵌套复合类型是否在 M3 支持？（建议不支持，复杂度高）
4. **通用**：是否提供 feature flag 单独启用这些类型？（建议统一在 `extra_types` feature 下）
